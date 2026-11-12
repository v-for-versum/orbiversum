/*
 * Addressable RGB ring around the knob.
 *
 * Driven through the led_strip component's RMT backend, which lets the CPU
 * get on with rendering while the pulse train shifts out.
 *
 * The data-line GPIO is a menuconfig option (Component config -> Knob BSP ->
 * Peripherals) because it is the one peripheral that differs between board
 * revisions, and because on at least one revision the ring shares GPIO0 with
 * the encoder's push button — see docs/HARDWARE.md. Set it to -1 to compile
 * the ring out completely.
 *
 * Brightness is deliberately capped at CONFIG_BSP_LED_BRIGHTNESS_LIMIT. The
 * ring, the display backlight, the motor and the DAC all draw from the same
 * 5 V rail, and full white on two dozen LEDs is enough to brown the board out
 * mid-frame.
 */
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Returns ESP_ERR_NOT_SUPPORTED when no LED GPIO is configured. */
esp_err_t bsp_led_init(void);
esp_err_t bsp_led_deinit(void);

bool bsp_led_is_available(void);

/* Number of LEDs in the ring, or 0 when unavailable. */
uint16_t bsp_led_count(void);

/*
 * Set how much of the ceiling the ring is allowed to use, 0..255.
 *
 * This is an attenuation, not a brightness: 255 means "nothing taken off" and
 * every colour reaches the strip as written, still bounded by
 * CONFIG_BSP_LED_BRIGHTNESS_LIMIT. The application's own brightness control is
 * expressed here rather than in the colours, so that a whole-ring flash from a
 * subsystem that knows nothing about brightness obeys it too.
 */
void bsp_led_set_brightness(uint8_t scale);

/* The current attenuation, 0..255. */
uint8_t bsp_led_get_brightness(void);

/*
 * Buffer one pixel. Nothing reaches the strip until bsp_led_refresh(), so a
 * whole animation frame can be built up and pushed in one burst.
 *
 * `index` is clamped-range checked; out-of-range writes return
 * ESP_ERR_INVALID_ARG rather than wrapping.
 */
esp_err_t bsp_led_set(uint16_t index, uint8_t r, uint8_t g, uint8_t b);

/* Buffer the same colour for every LED. */
esp_err_t bsp_led_set_all(uint8_t r, uint8_t g, uint8_t b);

/* Convenience wrapper: hue 0..359, sat/val 0..255. */
esp_err_t bsp_led_set_all_hsv(uint16_t hue, uint8_t sat, uint8_t val);

/* Push the buffer out to the strip. */
esp_err_t bsp_led_refresh(void);

/* Buffer black for every LED and immediately refresh. */
esp_err_t bsp_led_clear(void);

#ifdef __cplusplus
}
#endif
