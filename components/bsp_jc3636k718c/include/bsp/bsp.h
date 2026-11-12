/*
 * Board support package for the Guition JC3636K718C — the 1.85" round knob
 * screen (ESP32-S3R8, 16 MB flash, 8 MB Octal PSRAM).
 *
 * The point of this header is that application code never names a GPIO and
 * never has to know the bring-up order. Everything is reached through the
 * per-peripheral headers included below.
 *
 * TYPICAL app_main
 *
 *     void app_main(void)
 *     {
 *         const bsp_features_t features = BSP_FEATURES_DEFAULT();
 *         ESP_ERROR_CHECK(bsp_init(&features));
 *
 *         // LVGL is now running. Touch it only under the lock.
 *         bsp_display_lock(0);
 *         lv_obj_t *label = lv_label_create(bsp_display_get_screen());
 *         lv_label_set_text(label, "Hello");
 *         lv_obj_center(label);
 *         lv_group_add_obj(bsp_encoder_get_group(), label);
 *         bsp_display_unlock();
 *     }
 *
 * PARTIAL BRING-UP
 *   bsp_init() is a convenience wrapper. Applications that want finer control
 *   can call the per-peripheral functions directly, in this order:
 *
 *       bsp_i2c-free hardware first:  bsp_display_init()
 *       then anything that needs it:  bsp_touch_init(), bsp_encoder_init()
 *       independent peripherals:      bsp_audio_*_init(), bsp_sdcard_mount(),
 *                                     bsp_led_init()
 */
#pragma once

#include <stdbool.h>

#include "esp_err.h"

#include "bsp/bsp_pins.h"
#include "bsp/bsp_display.h"
#include "bsp/bsp_touch.h"
#include "bsp/bsp_encoder.h"
#include "bsp/bsp_encoder_probe.h"
#include "bsp/bsp_audio.h"
#include "bsp/bsp_sdcard.h"
#include "bsp/bsp_led.h"
#include "bsp/bsp_battery.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Which peripherals to bring up. Anything left false is simply skipped, so a
 * stripped-down build costs nothing for hardware it does not use.
 */
typedef struct {
    bool display;    /*!< Panel + backlight + LVGL port. Required by touch and encoder. */
    bool touch;      /*!< CST816S capacitive touch */
    bool encoder;    /*!< Rotary knob, bound to LVGL as an encoder indev */
    bool led_ring;   /*!< Addressable RGB ring */
    bool audio_out;  /*!< PCM5100A I2S DAC */
    bool audio_in;   /*!< PDM microphone */
    bool sdcard;     /*!< microSD, mounted at /sdcard */
    bool battery;    /*!< Cell voltage sense on the battery pin */
} bsp_features_t;

/*
 * Everything most applications need: screen, touch and knob.
 *
 * The LED ring is absent for a different reason: GPIO0 is a strapping pin, and
 * a ring that lights up before the application is ready masks boot problems.
 *
 * The battery sense is absent because it is a readout rather than a peripheral:
 * nothing else depends on it, and an application that does not show a percentage
 * has no reason to spend an ADC unit on one.
 */
#define BSP_FEATURES_DEFAULT()          \
    {                                   \
        .display   = true,              \
        .touch     = true,              \
        .encoder   = true,              \
        .led_ring  = false,             \
        .audio_out = false,             \
        .audio_in  = false,             \
        .sdcard    = false,             \
        .battery   = false,             \
    }

/* Everything the board has to offer. */
#define BSP_FEATURES_ALL()              \
    {                                   \
        .display   = true,              \
        .touch     = true,              \
        .encoder   = true,              \
        .led_ring  = true,              \
        .audio_out = true,              \
        .audio_in  = true,              \
        .sdcard    = true,              \
        .battery   = true,              \
    }

/*
 * Bring up the requested peripherals in dependency order.
 *
 * Returns ESP_OK when every *requested* peripheral came up. Peripheral
 * failures are logged individually and the first one is returned, but the
 * remaining peripherals are still initialised — a knob with a dead SD card is
 * far more useful than one that refuses to boot.
 *
 * Pass NULL for features to use BSP_FEATURES_DEFAULT().
 */
esp_err_t bsp_init(const bsp_features_t *features);

/* Log a one-screen summary: firmware version, chip, memory, and what came up.
 * Call after bsp_init(). */
void bsp_print_info(void);

/* FreeRTOS tick rate, in Hz, as configured for the LVGL tick. */
uint32_t bsp_get_tick_rate_hz(void);

#ifdef __cplusplus
}
#endif
