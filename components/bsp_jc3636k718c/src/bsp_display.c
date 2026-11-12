/*
 * Display bring-up — ST77916 on QSPI plus the LVGL port.
 *
 * See bsp_display.h for the ordering contract this file implements.
 */

#include "bsp/bsp_display.h"
#include "esp_log.h"

#include "esp_check.h"
#include "esp_log.h"

#include "driver/ledc.h"
#include "driver/spi_master.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"

#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_vendor.h"
#include "esp_lcd_st77916.h"
#include "esp_lvgl_port.h"

#include "bsp/bsp_pins.h"

#if CONFIG_BSP_LCD_USE_VENDOR_INIT
#include "bsp/st77916_init_cmds.h"
#endif

static const char *TAG = "bsp.display";

/* SPI2 is free: the touch controller is on I2C, the SD card on SDMMC. */
#define BSP_LCD_HOST        SPI2_HOST
#define BSP_LCD_BITS_PER_PX 16

#define BSP_BL_LEDC_MODE     LEDC_LOW_SPEED_MODE
#define BSP_BL_LEDC_TIMER    LEDC_TIMER_0
#define BSP_BL_LEDC_CHANNEL  LEDC_CHANNEL_0
#define BSP_BL_LEDC_RES      LEDC_TIMER_10_BIT
#define BSP_BL_LEDC_FREQ_HZ  20000
#define BSP_BL_LEDC_MAX      ((1u << 10) - 1u)

static esp_lcd_panel_io_handle_t s_io;
static esp_lcd_panel_handle_t    s_panel;
static lv_display_t             *s_disp;
static bool                      s_backlight_ready;

/*
 * LVGL's own log had no way out of the chip on this board: the port registers no print
 * callback and LV_LOG_PRINTF is off, so every LVGL warning — and the performance monitor,
 * which in log mode reports the frame rate and the render and flush times once a second —
 * was being formatted and then written to nothing.
 *
 * TRACE and INFO are deliberately dropped rather than carried. The configured level is
 * LV_LOG_LEVEL_USER, which admits everything from TRACE up, so passing the lot through
 * would put a line on the serial port for every display refresh and every timer; the
 * logging would then cost more than the thing being measured. What is wanted here is the
 * warnings and the user level, and the user level carries the performance line alone.
 */
static void lvgl_log_cb(lv_log_level_t level, const char *buf)
{
    switch (level) {
    case LV_LOG_LEVEL_ERROR:
        ESP_LOGE(TAG, "%s", buf);
        break;
    case LV_LOG_LEVEL_WARN:
        ESP_LOGW(TAG, "%s", buf);
        break;
    case LV_LOG_LEVEL_USER:
        ESP_LOGI(TAG, "%s", buf);
        break;
    default:
        break;
    }
}
static int                       s_brightness = -1;

/*
 * The panel is dark until a frame is actually on the glass.
 *
 * backlight_init() starts the LEDC channel at zero for exactly that reason — a lit
 * panel nothing has been drawn on shows white, and white for two seconds looks like a
 * fault — but the code below then defeated it twice over: bsp_display_init() set the
 * default level immediately, and the application's settings loader set the level stored in the settings
 * before the screen was built. Both calls happen before LVGL has drawn anything, so
 * the glass was lit and blank from the moment the panel came up until the first frame,
 * measured at 2059 ms in the boot log.
 *
 * So the duty is gated now. A level asked for before the first frame is REMEMBERED and
 * not applied; the first frame releases it through a short fade. Nothing else changes
 * — the getter still reports the level that was asked for, so a settings screen built
 * before the first frame shows the stored brightness rather than zero.
 */
static bool                      s_panel_up;   /* a frame is on the glass */
static esp_timer_handle_t        s_rise;       /* the fade, one step per BL_RISE_STEP_MS */
static esp_timer_handle_t        s_failsafe;   /* the way out if no frame ever comes */
static int                       s_rise_step;

#define BL_RISE_MS          200   /* the fade, from dark to the stored level */
#define BL_RISE_STEP_MS      20
#define BL_RISE_STEPS       (BL_RISE_MS / BL_RISE_STEP_MS)
#define BL_RISE_FALLBACK_MS 3000  /* no frame by now: light the panel anyway */

/* Write the duty directly, bypassing the gate. */
static void backlight_write(int percent)
{
    if (percent < 0)   { percent = 0; }
    if (percent > 100) { percent = 100; }

    const uint32_t duty = (uint32_t)(((uint64_t)percent * BSP_BL_LEDC_MAX) / 100u);
    ledc_set_duty(BSP_BL_LEDC_MODE, BSP_BL_LEDC_CHANNEL, duty);
    ledc_update_duty(BSP_BL_LEDC_MODE, BSP_BL_LEDC_CHANNEL);
}

static void backlight_rise_cb(void *arg)
{
    (void)arg;

    if (s_rise_step < BL_RISE_STEPS) {
        s_rise_step++;
    }
    if (s_rise_step >= BL_RISE_STEPS) {
        esp_timer_stop(s_rise);
        backlight_write(s_brightness);
        return;
    }
    /* Read the target every step, so a brightness changed during the fade is followed
     * rather than overtaken by it. */
    backlight_write(s_brightness * s_rise_step / BL_RISE_STEPS);
}

/*
 * Light the panel for the first time. Idempotent and callable from any task: it is
 * reached from the LVGL task through the refresh event, and from a timer through the
 * fallback.
 */
static void backlight_release(void)
{
    if (s_panel_up) {
        return;
    }
    s_panel_up = true;

    if (!s_backlight_ready || s_rise == NULL) {
        return;
    }

    ESP_LOGI(TAG, "screen built at %d ms, backlight rising to %d%%",
             (int)(esp_timer_get_time() / 1000), s_brightness);

    s_rise_step = 0;
    esp_timer_stop(s_rise);
    esp_timer_start_periodic(s_rise, BL_RISE_STEP_MS * 1000);
}

/*
 * Light the panel for the first time, at the level already recorded, over the fade.
 * Called by the application once its screen is built — see the note in
 * bsp_display_init() for why this is not hung on a refresh. Idempotent.
 */
void bsp_display_backlight_show(void)
{
    backlight_release();
}

static void backlight_failsafe_cb(void *arg)
{
    (void)arg;
    if (!s_panel_up) {
        ESP_LOGW(TAG, "no frame after %d ms, lighting the panel anyway",
                 BL_RISE_FALLBACK_MS);
    }
    backlight_release();
}

/* -------------------------------------------------------------------------- */
/* Backlight                                                                  */
/* -------------------------------------------------------------------------- */

static esp_err_t backlight_init(void)
{
    const ledc_timer_config_t timer_cfg = {
        .speed_mode      = BSP_BL_LEDC_MODE,
        .duty_resolution = BSP_BL_LEDC_RES,
        .timer_num       = BSP_BL_LEDC_TIMER,
        .freq_hz         = BSP_BL_LEDC_FREQ_HZ,
        .clk_cfg         = LEDC_AUTO_CLK,
    };
    ESP_RETURN_ON_ERROR(ledc_timer_config(&timer_cfg), TAG, "ledc timer");

    const ledc_channel_config_t chan_cfg = {
        .gpio_num   = BSP_LCD_BL,
        .speed_mode = BSP_BL_LEDC_MODE,
        .channel    = BSP_BL_LEDC_CHANNEL,
        .intr_type  = LEDC_INTR_DISABLE,
        .timer_sel  = BSP_BL_LEDC_TIMER,
        /* Start dark, and stay dark until the first frame: a lit-but-blank screen
         * during boot looks like a fault. See the note above s_panel_up. */
        .duty       = 0,
        .hpoint     = 0,
        .flags      = { .output_invert = BSP_LCD_BL_ACTIVE_LEVEL ? 0 : 1 },
    };
    ESP_RETURN_ON_ERROR(ledc_channel_config(&chan_cfg), TAG, "ledc channel");

    s_backlight_ready = true;
    s_brightness = 0;
    return ESP_OK;
}

int bsp_display_brightness_set(int percent)
{
    if (!s_backlight_ready) {
        return -1;
    }

    if (percent < 0)   { percent = 0; }
    if (percent > 100) { percent = 100; }

    s_brightness = percent;

    /* Before the first frame this only records — see the note above s_panel_up. */
    if (s_panel_up) {
        backlight_write(percent);
    }
    return percent;
}

int bsp_display_brightness_get(void)
{
    return s_brightness;
}

esp_err_t bsp_display_backlight_on(void)
{
    if (!s_backlight_ready) {
        return ESP_ERR_INVALID_STATE;
    }
    /* Restore the configured level rather than snapping to full: a UI that
     * wakes at eye-searing brightness at 3 a.m. is a bug. Before the first frame
     * this records the level and the frame lights it; afterwards it is immediate. */
    bsp_display_brightness_set(CONFIG_BSP_LCD_BRIGHTNESS_DEFAULT);
    return ESP_OK;
}

esp_err_t bsp_display_backlight_off(void)
{
    if (!s_backlight_ready) {
        return ESP_ERR_INVALID_STATE;
    }
    if (s_rise) {
        esp_timer_stop(s_rise);
    }
    if (s_failsafe) {
        esp_timer_stop(s_failsafe);
    }
    /* A deliberate off is not a pending fade: a later set must apply at once. */
    s_panel_up = true;
    s_brightness = 0;
    backlight_write(0);
    return ESP_OK;
}

/* -------------------------------------------------------------------------- */
/* LVGL mutex                                                                 */
/* -------------------------------------------------------------------------- */

bool bsp_display_lock(uint32_t timeout_ms)
{
    return lvgl_port_lock(timeout_ms);
}

void bsp_display_unlock(void)
{
    lvgl_port_unlock();
}

/* -------------------------------------------------------------------------- */
/* Accessors                                                                  */
/* -------------------------------------------------------------------------- */

lv_display_t *bsp_display_get_lvgl_display(void)
{
    return s_disp;
}

lv_obj_t *bsp_display_get_screen(void)
{
    if (!s_disp) {
        return NULL;
    }
    return lv_display_get_screen_active(s_disp);
}

esp_lcd_panel_handle_t bsp_display_get_panel(void)
{
    return s_panel;
}

esp_lcd_panel_io_handle_t bsp_display_get_io(void)
{
    return s_io;
}

/* -------------------------------------------------------------------------- */
/* Bring-up                                                                   */
/* -------------------------------------------------------------------------- */

esp_err_t bsp_display_init(void)
{
    if (s_disp) {
        ESP_LOGW(TAG, "display already initialised");
        return ESP_OK;
    }

    const size_t buffer_pixels = (size_t)BSP_LCD_H_RES * CONFIG_BSP_LCD_BUFFER_LINES;

    /* --- 1. QSPI bus ------------------------------------------------------ */
    const spi_bus_config_t bus_cfg = ST77916_PANEL_BUS_QSPI_CONFIG(
        BSP_LCD_PCLK,
        BSP_LCD_DATA0, BSP_LCD_DATA1, BSP_LCD_DATA2, BSP_LCD_DATA3,
        buffer_pixels * sizeof(uint16_t));

    ESP_RETURN_ON_ERROR(spi_bus_initialize(BSP_LCD_HOST, &bus_cfg, SPI_DMA_CH_AUTO),
                        TAG, "spi_bus_initialize");

    /* --- 2. Panel IO ------------------------------------------------------ */
    esp_lcd_panel_io_spi_config_t io_cfg = ST77916_PANEL_IO_QSPI_CONFIG(BSP_LCD_CS, NULL, NULL);
    /* The macro hardcodes 40 MHz; take the configured value instead. */
    io_cfg.pclk_hz = (unsigned int)CONFIG_BSP_LCD_QSPI_FREQ_MHZ * 1000u * 1000u;

    ESP_RETURN_ON_ERROR(
        esp_lcd_new_panel_io_spi((esp_lcd_spi_bus_handle_t)BSP_LCD_HOST, &io_cfg, &s_io),
        TAG, "esp_lcd_new_panel_io_spi");

    /* --- 3. Panel --------------------------------------------------------- */
    const st77916_vendor_config_t vendor_cfg = {
#if CONFIG_BSP_LCD_USE_VENDOR_INIT
        .init_cmds      = st77916_vendor_init_cmds,
        .init_cmds_size = (uint16_t)st77916_vendor_init_cmds_size,
#endif
        .flags = {
            /* Without this the driver talks single-lane SPI and the panel
             * stays dark. */
            .use_qspi_interface = 1,
        },
    };

    const esp_lcd_panel_dev_config_t panel_cfg = {
        .reset_gpio_num = BSP_LCD_RST,
        .rgb_ele_order  = LCD_RGB_ELEMENT_ORDER_RGB,
        .bits_per_pixel = BSP_LCD_BITS_PER_PX,
        .vendor_config  = (void *)&vendor_cfg,
    };

    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_st77916(s_io, &panel_cfg, &s_panel),
                        TAG, "esp_lcd_new_panel_st77916");

    /* --- 4. Panel init ---------------------------------------------------- */
    ESP_RETURN_ON_ERROR(esp_lcd_panel_reset(s_panel), TAG, "panel reset");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_init(s_panel), TAG, "panel init");

    /*
     * No esp_lcd_panel_invert_color() call here on purpose.
     *
     * This panel is wired for inverted colour and both the driver's built-in
     * sequence and the vendor sequence already end with 0x21 (INVON).
     * Inverting a second time produces a photographic negative — a mistake
     * that is easy to make because the vendor Arduino demo calls
     * invertColor(true) as well, which is idempotent rather than additive.
     */

    ESP_RETURN_ON_ERROR(esp_lcd_panel_disp_on_off(s_panel, true), TAG, "panel display on");

    /* --- 5. Backlight ----------------------------------------------------- */
    ESP_RETURN_ON_ERROR(backlight_init(), TAG, "backlight");

    /*
     * The level the panel will come up to, and the two timers that get it there.
     *
     * Recorded here rather than at the end of this function, because the first frame
     * can arrive the moment the display is added to LVGL below: if the default were
     * set after that, a refresh racing this function would find a target of zero and
     * fade the panel up to nothing.
     */
    bsp_display_brightness_set(CONFIG_BSP_LCD_BRIGHTNESS_DEFAULT);

    const esp_timer_create_args_t rise_cfg = {
        .callback = backlight_rise_cb,
        .name     = "bl_rise",
    };
    ESP_RETURN_ON_ERROR(esp_timer_create(&rise_cfg, &s_rise), TAG, "rise timer");

    const esp_timer_create_args_t failsafe_cfg = {
        .callback = backlight_failsafe_cb,
        .name     = "bl_failsafe",
    };
    ESP_RETURN_ON_ERROR(esp_timer_create(&failsafe_cfg, &s_failsafe), TAG, "failsafe timer");
    ESP_RETURN_ON_ERROR(esp_timer_start_once(s_failsafe, (uint64_t)BL_RISE_FALLBACK_MS * 1000),
                        TAG, "failsafe arm");

    /* --- 6. LVGL port ----------------------------------------------------- */
    lvgl_port_cfg_t lvgl_cfg = ESP_LVGL_PORT_INIT_CONFIG();
    /* Richer UIs (charts, nested flex layouts) exceed the 7 KB default. */
    lvgl_cfg.task_stack = 8192;
    /*
     * Pin the UI to core 0. Core 1 is the audio core: the synth's audio task runs
     * there at priority 18 with roughly a 50% duty cycle. Leaving LVGL unpinned
     * would let the scheduler drop it on core 1 and halve the frame rate for no
     * reason, and core 0 shares only with Wi-Fi, whose CPU use is bursty and which
     * the UI does not have to beat.
     *
     * What pinning does NOT do, and what this comment used to claim: a task of lower
     * priority can still delay the audio task, because priority orders the CPUs and
     * not the memory. Both cores read the same PSRAM — the delay, chorus and reverb
     * lines live there, and so do these draw buffers — so a UI that is rendering
     * takes bandwidth away from the audio block. Measured while the percussion was
     * overloaded: a block cost 3200 us with the screen untouched and up to 3899 us
     * with a hand on it, against a 2666 us budget.
     */
    lvgl_cfg.task_affinity = 0;
    ESP_RETURN_ON_ERROR(lvgl_port_init(&lvgl_cfg), TAG, "lvgl_port_init");

    /* After the port, so that anything it logs during its own start-up is still lost — there
     * is nothing to print with yet — and everything from here on is not. */
    lv_log_register_print_cb(lvgl_log_cb);

    /* --- 7. Attach the display to LVGL ------------------------------------ */
    const lvgl_port_display_cfg_t disp_cfg = {
        .io_handle     = s_io,
        .panel_handle  = s_panel,
        .buffer_size   = (uint32_t)buffer_pixels,   /* pixels, not bytes */
        .double_buffer = CONFIG_BSP_LCD_DOUBLE_BUFFER,
        .hres          = BSP_LCD_H_RES,
        .vres          = BSP_LCD_V_RES,
        .monochrome    = false,
        .rotation = {
            .swap_xy  = false,
            .mirror_x = false,
            .mirror_y = false,
        },
#if LVGL_VERSION_MAJOR >= 9
        .color_format  = LV_COLOR_FORMAT_RGB565,
#endif
        .flags = {
            /* PSRAM-backed buffers leave internal RAM for Wi-Fi and audio.
             * The S3's SPI DMA can read PSRAM, unlike the original ESP32. */
            .buff_dma    = CONFIG_BSP_LCD_BUFFER_IN_PSRAM ? 0 : 1,
            .buff_spiram = CONFIG_BSP_LCD_BUFFER_IN_PSRAM ? 1 : 0,
            .swap_bytes  = CONFIG_BSP_LCD_SWAP_BYTES,
        },
    };

    s_disp = lvgl_port_add_disp(&disp_cfg);
    if (!s_disp) {
        ESP_LOGE(TAG, "lvgl_port_add_disp failed");
        return ESP_FAIL;
    }

    /*
     * The screen LVGL starts on is WHITE, and it is what gets refreshed in the window
     * between this function returning and the application's own screen replacing it:
     * measured at 20 ms after this point, with the application's screen arriving 800 ms
     * later. Lighting the panel on that first refresh therefore showed white for the
     * whole of that window — the fault this change exists to remove, only moved. So the
     * release is not hung on a refresh at all: the application calls
     * bsp_display_backlight_show() when its screen is built, and the fallback timer
     * covers a build that never finishes. The default screen is painted black as well,
     * so that even a frame drawn before then cannot be white.
     */
    if (bsp_display_lock(0)) {
        lv_obj_t *scr = lv_screen_active();
        if (scr) {
            lv_obj_set_style_bg_color(scr, lv_color_black(), LV_PART_MAIN);
            lv_obj_set_style_bg_opa(scr, LV_OPA_COVER, LV_PART_MAIN);
        }
        bsp_display_unlock();
    }

    ESP_LOGI(TAG, "ST77916 %dx%d ready: QSPI %d MHz, %d-line %s buffer in %s",
             BSP_LCD_H_RES, BSP_LCD_V_RES, CONFIG_BSP_LCD_QSPI_FREQ_MHZ,
             CONFIG_BSP_LCD_BUFFER_LINES,
             CONFIG_BSP_LCD_DOUBLE_BUFFER ? "double" : "single",
             CONFIG_BSP_LCD_BUFFER_IN_PSRAM ? "PSRAM" : "internal RAM");
    ESP_LOGI(TAG, "init sequence: %s",
             CONFIG_BSP_LCD_USE_VENDOR_INIT ? "vendor (Arduino demo)"
                                            : "driver default");

    /* The backlight level was recorded at step 5 and is waiting for the first frame. */
    return ESP_OK;
}

esp_err_t bsp_display_rotation_set(uint8_t rotation)
{
    if (!s_disp) {
        return ESP_ERR_INVALID_STATE;
    }
    if (rotation > 3) {
        return ESP_ERR_INVALID_ARG;
    }

    /* Takes the lock itself, so it must not be called from code that already
     * holds it. */
    if (!bsp_display_lock(0)) {
        return ESP_FAIL;
    }
    lv_display_set_rotation(s_disp, (lv_display_rotation_t)rotation);
    bsp_display_unlock();

    return ESP_OK;
}

esp_err_t bsp_display_deinit(void)
{
    if (!s_disp) {
        return ESP_OK;
    }

    bsp_display_backlight_off();

    if (s_rise) {
        esp_timer_delete(s_rise);
        s_rise = NULL;
    }
    if (s_failsafe) {
        esp_timer_delete(s_failsafe);
        s_failsafe = NULL;
    }
    s_panel_up = false;

    lvgl_port_remove_disp(s_disp);
    s_disp = NULL;

    if (s_panel) {
        esp_lcd_panel_del(s_panel);
        s_panel = NULL;
    }
    if (s_io) {
        esp_lcd_panel_io_del(s_io);
        s_io = NULL;
    }

    spi_bus_free(BSP_LCD_HOST);
    lvgl_port_deinit();

    s_backlight_ready = false;
    return ESP_OK;
}

/* -------------------------------------------------------------------------- */
/* Raw drawing (no LVGL)                                                      */
/* -------------------------------------------------------------------------- */

static SemaphoreHandle_t s_raw_done;
static bool              s_raw;
/* A draw_rect timed out: its transfer may still be reading the caller's buffer. */
static bool              s_raw_pending;

static bool raw_trans_done_cb(esp_lcd_panel_io_handle_t io,
                              esp_lcd_panel_io_event_data_t *edata, void *user)
{
    (void)io; (void)edata; (void)user;
    BaseType_t woken = pdFALSE;
    xSemaphoreGiveFromISR(s_raw_done, &woken);
    return woken == pdTRUE;
}

esp_err_t bsp_display_raw_begin(void)
{
    if (s_raw) {
        return ESP_OK;
    }
    if (!s_disp || !s_io) {
        return ESP_ERR_INVALID_STATE;
    }
    SemaphoreHandle_t sem = xSemaphoreCreateBinary();
    if (!sem) {
        return ESP_ERR_NO_MEM;
    }

    /* lvgl_port_stop() only disables LVGL's timers (refresh, indev read timers) and
     * the tick: the LVGL task itself keeps looping at 1 kHz, taking the LVGL lock and
     * reading input devices, but with no timer ready it never renders or flushes.
     * Take the lock so the stop cannot land in the middle of a render/flush. */
    if (!bsp_display_lock(1000)) {
        vSemaphoreDelete(sem);
        return ESP_ERR_TIMEOUT;
    }
    esp_err_t err = lvgl_port_stop();
    bsp_display_unlock();
    if (err != ESP_OK) {
        vSemaphoreDelete(sem);
        ESP_LOGE(TAG, "lvgl_port_stop: %s", esp_err_to_name(err));
        return err;
    }

    /* A flush LVGL started just before the stop may still be on the wire; its
     * completion callback is the LVGL one, which is harmless. Let it finish (a 16-row
     * strip takes well under 2 ms) before the callback is swapped, so its completion
     * cannot land on the new callback and release the first draw_rect early. */
    vTaskDelay(pdMS_TO_TICKS(50));

    s_raw_done = sem;
    const esp_lcd_panel_io_callbacks_t cbs = { .on_color_trans_done = raw_trans_done_cb };
    err = esp_lcd_panel_io_register_event_callbacks(s_io, &cbs, NULL);
    if (err != ESP_OK) {
        s_raw_done = NULL;
        vSemaphoreDelete(sem);
        ESP_LOGE(TAG, "register callbacks: %s", esp_err_to_name(err));
        return err;
    }
    s_raw_pending = false;
    s_raw = true;
    ESP_LOGI(TAG, "raw drawing on, LVGL timers stopped (max %u px per write)",
             (unsigned)bsp_display_raw_max_pixels());
    return ESP_OK;
}

uint32_t bsp_display_raw_max_pixels(void)
{
    return (uint32_t)BSP_LCD_H_RES * CONFIG_BSP_LCD_BUFFER_LINES;
}

esp_err_t bsp_display_draw_rect(int x, int y, int w, int h, const uint16_t *rgb565)
{
    if (!s_raw) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!rgb565 || w <= 0 || h <= 0 || x < 0 || y < 0 ||
        x + w > BSP_LCD_H_RES || y + h > BSP_LCD_V_RES ||
        (uint32_t)w * (uint32_t)h > bsp_display_raw_max_pixels()) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_raw_pending) {
        /* An earlier write timed out. Until its (late) completion arrives the DMA may
         * still be reading that caller's buffer, so refuse to start another one. */
        if (xSemaphoreTake(s_raw_done, 0) != pdTRUE) {
            return ESP_ERR_INVALID_STATE;
        }
        s_raw_pending = false;
    } else {
        /* No transfer is in flight: drop any stale completion. */
        (void)xSemaphoreTake(s_raw_done, 0);
    }
    ESP_RETURN_ON_ERROR(esp_lcd_panel_draw_bitmap(s_panel, x, y, x + w, y + h, rgb565),
                        TAG, "draw_bitmap");
    if (xSemaphoreTake(s_raw_done, pdMS_TO_TICKS(100)) != pdTRUE) {
        s_raw_pending = true;
        return ESP_ERR_TIMEOUT;
    }
    return ESP_OK;
}
