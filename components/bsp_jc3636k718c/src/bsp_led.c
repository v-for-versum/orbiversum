/*
 * Addressable RGB ring — see bsp_led.h.
 */

#include "bsp/bsp_led.h"

#include "esp_check.h"
#include "esp_log.h"

#include "led_strip.h"

static const char *TAG = "bsp.led";

/* RMT resolution. The WS2812 bit period is 1.25 us, so 10 MHz gives 12.5
 * ticks per bit — comfortably inside the part's tolerance. */
#define BSP_LED_RMT_RESOLUTION_HZ (10 * 1000 * 1000)

static led_strip_handle_t s_strip;
static uint16_t           s_count;

/* How much of the ceiling the application allows. 255 = nothing taken off; the
 * ring is at full brightness until something says otherwise. */
static uint8_t            s_scale = 255;

/* -------------------------------------------------------------------------- */
/* Colour helpers                                                             */
/* -------------------------------------------------------------------------- */

/*
 * HSV -> RGB, written out rather than borrowed from LVGL so this file stays
 * usable when the ring is driven before the display is up — for instance as a
 * boot indicator, which is exactly when it is most useful.
 */
static void hsv_to_rgb(uint16_t hue, uint8_t sat, uint8_t val,
                       uint8_t *r, uint8_t *g, uint8_t *b)
{
    hue %= 360;

    if (sat == 0) {
        *r = *g = *b = val;
        return;
    }

    const uint8_t region    = (uint8_t)(hue / 60);       /* 0..5 */
    const uint8_t remainder = (uint8_t)(hue % 60);       /* 0..59  */

    const uint32_t p = ((uint32_t)val * (255u - sat)) / 255u;
    const uint32_t q = ((uint32_t)val * (255u - ((uint32_t)sat * remainder) / 60u)) / 255u;
    const uint32_t t = ((uint32_t)val * (255u - ((uint32_t)sat * (60u - remainder)) / 60u)) / 255u;

    switch (region) {
    case 0:  *r = (uint8_t)val; *g = (uint8_t)t;   *b = (uint8_t)p;   break;
    case 1:  *r = (uint8_t)q;   *g = (uint8_t)val; *b = (uint8_t)p;   break;
    case 2:  *r = (uint8_t)p;   *g = (uint8_t)val; *b = (uint8_t)t;   break;
    case 3:  *r = (uint8_t)p;   *g = (uint8_t)q;   *b = (uint8_t)val; break;
    case 4:  *r = (uint8_t)t;   *g = (uint8_t)p;   *b = (uint8_t)val; break;
    default: *r = (uint8_t)val; *g = (uint8_t)p;   *b = (uint8_t)q;   break;
    }
}

/* Apply the configured ceiling, so no caller can accidentally exceed it. The
 * application's attenuation is applied first and in the same step: two callers
 * multiplying colours by hand is two places to get the rounding wrong. */
static inline uint8_t limit(uint8_t value)
{
    const uint32_t scaled = ((uint32_t)value * s_scale) / 255u;
    return (uint8_t)((scaled * CONFIG_BSP_LED_BRIGHTNESS_LIMIT) / 255u);
}

void bsp_led_set_brightness(uint8_t scale)
{
    s_scale = scale;
}

uint8_t bsp_led_get_brightness(void)
{
    return s_scale;
}

/* -------------------------------------------------------------------------- */
/* Public API                                                                 */
/* -------------------------------------------------------------------------- */

esp_err_t bsp_led_init(void)
{
    if (CONFIG_BSP_LED_GPIO < 0) {
        ESP_LOGI(TAG, "no LED ring configured — skipping");
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (s_strip) {
        return ESP_OK;
    }

    const led_strip_config_t strip_cfg = {
        .strip_gpio_num = CONFIG_BSP_LED_GPIO,
        .max_leds       = CONFIG_BSP_LED_COUNT,
        /* LED_MODEL_WS2812 also fixes the colour order to GRB, which is what
         * this ring uses. */
        .led_model      = LED_MODEL_WS2812,
        .flags = {
            .invert_out = false,
        },
    };

    const led_strip_rmt_config_t rmt_cfg = {
        .clk_src           = RMT_CLK_SRC_DEFAULT,
        .resolution_hz     = BSP_LED_RMT_RESOLUTION_HZ,
        .mem_block_symbols = 64,
        .flags = {
            /* No DMA: a 24-LED frame is 72 bytes, far below the size where
             * DMA setup pays for itself. */
            .with_dma = false,
        },
    };

    esp_err_t err = led_strip_new_rmt_device(&strip_cfg, &rmt_cfg, &s_strip);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "led_strip_new_rmt_device failed: %s", esp_err_to_name(err));
        s_strip = NULL;
        return err;
    }

    s_count = CONFIG_BSP_LED_COUNT;
    led_strip_clear(s_strip);

    ESP_LOGI(TAG, "ring ready: %u LEDs on GPIO %d, brightness capped at %d/255",
             s_count, CONFIG_BSP_LED_GPIO, CONFIG_BSP_LED_BRIGHTNESS_LIMIT);
    return ESP_OK;
}

bool bsp_led_is_available(void)
{
    return s_strip != NULL;
}

uint16_t bsp_led_count(void)
{
    return s_count;
}

esp_err_t bsp_led_set(uint16_t index, uint8_t r, uint8_t g, uint8_t b)
{
    if (!s_strip) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (index >= s_count) {
        return ESP_ERR_INVALID_ARG;
    }
    return led_strip_set_pixel(s_strip, index, limit(r), limit(g), limit(b));
}

esp_err_t bsp_led_set_all(uint8_t r, uint8_t g, uint8_t b)
{
    if (!s_strip) {
        return ESP_ERR_NOT_SUPPORTED;
    }

    const uint8_t lr = limit(r);
    const uint8_t lg = limit(g);
    const uint8_t lb = limit(b);

    for (uint16_t i = 0; i < s_count; i++) {
        esp_err_t err = led_strip_set_pixel(s_strip, i, lr, lg, lb);
        if (err != ESP_OK) {
            return err;
        }
    }
    return ESP_OK;
}

esp_err_t bsp_led_set_all_hsv(uint16_t hue, uint8_t sat, uint8_t val)
{
    uint8_t r, g, b;
    hsv_to_rgb(hue, sat, val, &r, &g, &b);
    return bsp_led_set_all(r, g, b);
}

esp_err_t bsp_led_refresh(void)
{
    if (!s_strip) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return led_strip_refresh(s_strip);
}

esp_err_t bsp_led_clear(void)
{
    if (!s_strip) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return led_strip_clear(s_strip);
}

esp_err_t bsp_led_deinit(void)
{
    if (!s_strip) {
        return ESP_OK;
    }

    led_strip_clear(s_strip);
    esp_err_t err = led_strip_del(s_strip);
    s_strip = NULL;
    s_count = 0;
    return err;
}
