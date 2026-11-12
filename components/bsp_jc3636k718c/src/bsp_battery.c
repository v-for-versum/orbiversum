/*
 * Battery sense — see bsp_battery.h for what the board can and cannot say.
 */

#include "bsp/bsp_battery.h"

#include <stddef.h>

#include "esp_adc/adc_cali.h"
#include "esp_adc/adc_cali_scheme.h"
#include "esp_adc/adc_oneshot.h"
#include "esp_log.h"
#include "esp_timer.h"

#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"

#include "bsp/bsp_pins.h"

static const char *TAG = "bsp_battery";

/* One reading is a burst average: a single conversion of this pin wanders by tens of
 * millivolts, and there is nothing to gain by trusting any one of them. */
#define BATTERY_BURST       16

/* How long a reading is kept before the pin is touched again. Two reasons, both in the
 * header: the number should not dance, and the ADC is read from the UI task. */
#define BATTERY_CACHE_MS    5000

/*
 * The weight of one new reading in the smoothed value, as a divisor. Five seconds per
 * reading over sixteen of them is a time constant of about eighty seconds, which is far
 * faster than the thing being measured — a cell moves over hours — and far slower than
 * the noise, which was measured at seventy millivolts between reads.
 */
#define BATTERY_SMOOTH_DIV  16

/* What the pin can reach at 12 dB attenuation, for the case where this chip has no
 * calibration scheme to offer. Close enough to keep the percentage sane; the calibrated
 * path is the one every ESP32-S3 takes. */
#define BATTERY_UNCAL_FULL_MV   3100

/*
 * Whether the instrument is on the cable, inferred from the one thing the board can see.
 *
 * There is no pin for this — the charger's STAT drives an LED on the board and nothing else
 * (see the header, and the schematic). What there IS is the charger's topology: an ETA6003
 * is a power-path part, so while USB is attached it holds the cell node at its own regulated
 * voltage, and unplugging hands the node straight back to the cell, which under this
 * instrument's own load sits hundreds of millivolts lower.
 *
 * That step is large — about 380 mV at the cell, and 200 mV at the pin between the readings
 * taken from this board — and it happens in an instant. So this is decided on the RAW burst
 * average, NOT on the smoothed value the percentage uses: the readout wants to be calm, the
 * flag wants to be quick, and holding the flag to the same eighty-second smoothing was
 * measured as minutes of lag in both directions — it did not drop for a long while after the
 * cable came out, and it did not come back until the smoothed value had crawled up to 96%.
 * A reading is five seconds apart, so on the raw value this flips within one.
 *
 * Two thresholds rather than one, because a raw reading wanders by some 70 mV of its own and
 * a single edge would chatter against that. The band between the two is wider than that noise
 * on purpose, and inside it the flag keeps whatever it had — which is the honest answer when
 * the two situations are not far enough apart to be told apart.
 *
 * What this cannot do: a cell that is full, at rest and off the cable sits above the upper
 * threshold and reads as USB. With the instrument playing, the load pulls it down within
 * minutes. Treat it as a label on a readout, never as an input to anything that decides.
 */
#define BATTERY_USB_ON_PIN_MV   2180   /* cell about 4142 mV: the charger is holding the node */
#define BATTERY_USB_OFF_PIN_MV  2060   /* cell about 3914 mV: the cell is running it alone    */

static adc_oneshot_unit_handle_t s_adc;
static adc_cali_handle_t         s_cali;
static adc_unit_t                s_unit;
static adc_channel_t             s_chan;
static bool                      s_ready;

/*
 * Two tasks ask for this: the UI, for the label on the transport screen, and the
 * application's statistics line. The ADC unit is not shared between them by itself, and the
 * cached value underneath is shared state, so both go through here. The audio task never
 * touches this, so nothing on the sound path can ever wait on it.
 */
static SemaphoreHandle_t s_lock;

static int     s_pin_mv;
static int     s_smoothed_mv;
static int     s_percent = -1;
static int64_t s_read_us;
static bool    s_on_usb;

/*
 * A Li-ion discharge curve at the cell, coarsened to the points that carry the shape.
 *
 * The middle is the point of the table: 4.2 V to 4.0 V is a fifth of the charge and 3.9 V
 * to 3.7 V is half of it, so a single linear map from 3.3 V to 4.2 V would spend most of
 * its travel where the cell barely moves and almost none of it where it moves fastest.
 */
static const struct {
    int mv;
    int pct;
} BATTERY_CURVE[] = {
    { 4150, 100 }, { 4050, 90 }, { 3980, 80 }, { 3920, 70 },
    { 3870, 60 },  { 3830, 50 }, { 3800, 40 }, { 3770, 30 },
    { 3740, 20 },  { 3690, 10 }, { 3600,  5 }, { 3300,  0 },
};

static bool battery_calibrate(adc_unit_t unit, adc_channel_t chan)
{
#if ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
    {
        adc_cali_curve_fitting_config_t cfg = {
            .unit_id  = unit,
            .chan     = chan,
            .atten    = ADC_ATTEN_DB_12,
            .bitwidth = ADC_BITWIDTH_DEFAULT,
        };
        if (adc_cali_create_scheme_curve_fitting(&cfg, &s_cali) == ESP_OK) {
            return true;
        }
    }
#endif
#if ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
    {
        adc_cali_line_fitting_config_t cfg = {
            .unit_id  = unit,
            .atten    = ADC_ATTEN_DB_12,
            .bitwidth = ADC_BITWIDTH_DEFAULT,
        };
        if (adc_cali_create_scheme_line_fitting(&cfg, &s_cali) == ESP_OK) {
            return true;
        }
    }
#endif
    return false;
}

static int battery_percent_for(int cell_mv)
{
    const int last = (int)(sizeof(BATTERY_CURVE) / sizeof(BATTERY_CURVE[0])) - 1;
    if (cell_mv >= BATTERY_CURVE[0].mv) {
        return 100;
    }
    if (cell_mv <= BATTERY_CURVE[last].mv) {
        return 0;
    }

    for (int i = 0; i < last; i++) {
        const int hi = BATTERY_CURVE[i].mv;
        const int lo = BATTERY_CURVE[i + 1].mv;
        if (cell_mv <= hi && cell_mv >= lo) {
            const int span = hi - lo;
            const int step = hi - cell_mv;
            return BATTERY_CURVE[i].pct
                 - (BATTERY_CURVE[i].pct - BATTERY_CURVE[i + 1].pct) * step / span;
        }
    }
    return 0;
}

esp_err_t bsp_battery_init(void)
{
    adc_unit_t unit;
    adc_channel_t chan;
    esp_err_t err = adc_oneshot_io_to_channel(BSP_BATTERY_ADC, &unit, &chan);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "GPIO%d is not an ADC input: %s", (int)BSP_BATTERY_ADC,
                 esp_err_to_name(err));
        return err;
    }

    const adc_oneshot_unit_init_cfg_t unit_cfg = { .unit_id = unit };
    err = adc_oneshot_new_unit(&unit_cfg, &s_adc);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "no ADC unit: %s", esp_err_to_name(err));
        return err;
    }

    const adc_oneshot_chan_cfg_t chan_cfg = {
        .atten    = ADC_ATTEN_DB_12,
        .bitwidth = ADC_BITWIDTH_DEFAULT,
    };
    err = adc_oneshot_config_channel(s_adc, chan, &chan_cfg);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "no ADC channel: %s", esp_err_to_name(err));
        adc_oneshot_del_unit(s_adc);
        s_adc = NULL;
        return err;
    }

    s_unit = unit;
    s_chan = chan;

    s_lock = xSemaphoreCreateMutex();
    if (s_lock == NULL) {
        ESP_LOGW(TAG, "no lock for the sense line");
        adc_oneshot_del_unit(s_adc);
        s_adc = NULL;
        return ESP_ERR_NO_MEM;
    }

    s_ready = true;

    if (!battery_calibrate(unit, chan)) {
        ESP_LOGW(TAG, "no ADC calibration on this chip; using %.0f mV full scale",
                 (float)BATTERY_UNCAL_FULL_MV);
    }

    /*
     * Read once now and say the whole chain out loud. This line is the one that settles
     * BSP_BATTERY_DIVIDER: the pin voltage is a measurement, the cell voltage is that
     * measurement times a constant somebody chose.
     */
    (void)bsp_battery_pin_mv();
    (void)bsp_battery_percent();
    ESP_LOGI(TAG, "GPIO%d: pin %d mV -> cell %d mV (%d%%) with divider %.2f",
             (int)BSP_BATTERY_ADC, s_pin_mv, bsp_battery_mv(), s_percent,
             (double)BSP_BATTERY_DIVIDER);
    return ESP_OK;
}

bool bsp_battery_is_available(void)
{
    return s_ready;
}

static int battery_burst_mv(void)
{
    int32_t sum = 0;
    int taken = 0;

    for (int i = 0; i < BATTERY_BURST; i++) {
        int raw = 0;
        if (adc_oneshot_read(s_adc, s_chan, &raw) != ESP_OK) {
            continue;
        }
        int mv = 0;
        if (s_cali != NULL && adc_cali_raw_to_voltage(s_cali, raw, &mv) == ESP_OK) {
            sum += mv;
        } else {
            sum += raw * BATTERY_UNCAL_FULL_MV / 4095;
        }
        taken++;
    }

    return (taken > 0) ? (int)(sum / taken) : 0;
}

static void battery_refresh(void)
{
    const int64_t now = esp_timer_get_time();
    if (s_read_us != 0 && (now - s_read_us) < (int64_t)BATTERY_CACHE_MS * 1000) {
        return;
    }
    s_read_us = now;

    const int raw = battery_burst_mv();

    /* A pin that reads nothing is not a flat cell, it is a pin with nothing on it. The
     * last good value is kept rather than averaged towards zero. */
    if (raw <= 0) {
        s_percent = -1;
        return;
    }

    /*
     * Smoothed, because a burst average still lands tens of millivolts away from the last
     * one — measured on the instrument, 2104 to 2275 mV between reads five seconds apart —
     * and near the top of a lithium curve forty millivolts is four percent of charge. The
     * first reading after boot is taken as it is: there is nothing to average with, and the
     * boot line prints exactly what the pin said.
     */
    if (s_smoothed_mv == 0) {
        s_smoothed_mv = raw;
    } else {
        s_smoothed_mv += (raw - s_smoothed_mv) / BATTERY_SMOOTH_DIV;
    }

    s_pin_mv = s_smoothed_mv;
    const int cell_mv = (int)((float)s_pin_mv * BSP_BATTERY_DIVIDER + 0.5f);
    s_percent = battery_percent_for(cell_mv);

    /*
     * The USB flag is decided on the RAW reading, not on the smoothed one the percentage
     * uses, and with hysteresis — see the note above the thresholds. Logged on the
     * transitions, because it is the transitions that say whether it is behaving.
     */
    const bool was_usb = s_on_usb;
    if (s_on_usb) {
        s_on_usb = (raw > BATTERY_USB_OFF_PIN_MV);
    } else {
        s_on_usb = (raw >= BATTERY_USB_ON_PIN_MV);
    }
    if (s_on_usb != was_usb) {
        ESP_LOGI(TAG, "pin %d mV (cell %d mV, %d%%) -> %s", raw, cell_mv, s_percent,
                 s_on_usb ? "on USB, node held by the charger" : "on the cell alone");
    }
}

static void battery_lock(void)
{
    if (s_lock != NULL) {
        xSemaphoreTake(s_lock, portMAX_DELAY);
    }
}

static void battery_unlock(void)
{
    if (s_lock != NULL) {
        xSemaphoreGive(s_lock);
    }
}

int bsp_battery_pin_mv(void)
{
    if (!s_ready) {
        return 0;
    }
    battery_lock();
    battery_refresh();
    const int mv = s_pin_mv;
    battery_unlock();
    return mv;
}

int bsp_battery_mv(void)
{
    if (!s_ready) {
        return 0;
    }
    battery_lock();
    battery_refresh();
    const int mv = (int)((float)s_pin_mv * BSP_BATTERY_DIVIDER + 0.5f);
    battery_unlock();
    return mv;
}

int bsp_battery_percent(void)
{
    if (!s_ready) {
        return -1;
    }
    battery_lock();
    battery_refresh();
    const int pct = s_percent;
    battery_unlock();
    return pct;
}

bool bsp_battery_on_usb(void)
{
    if (!s_ready) {
        return false;
    }
    battery_lock();
    battery_refresh();
    const bool on_usb = s_on_usb;
    battery_unlock();
    return on_usb;
}
