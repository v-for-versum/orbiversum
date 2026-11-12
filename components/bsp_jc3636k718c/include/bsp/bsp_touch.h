/*
 * Touch: CST816S capacitive controller on I2C, bound to LVGL as a pointer
 * input device.
 *
 * Call bsp_touch_init() after bsp_display_init(): the input device has to be
 * attached to a display that already exists.
 *
 * The controller reports a single point. Gestures (swipe, long press) are not
 * decoded in hardware on this part, so the UI should use LVGL's own gesture
 * handling or, better, the rotary encoder — which is what a knob is for.
 */
#pragma once

#include <stdbool.h>
#include "esp_err.h"
#include "esp_lcd_touch.h"
#include "lvgl.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Probe the controller and register it with LVGL.
 *
 * Returns ESP_ERR_NOT_FOUND when the controller does not answer on the bus,
 * which is the diagnostic you want when the screen works but touches do
 * nothing.
 */
esp_err_t bsp_touch_init(void);

esp_err_t bsp_touch_deinit(void);

bool bsp_touch_is_ready(void);

/* Raw ESP-IDF touch handle, for code that wants to poll coordinates itself. */
esp_lcd_touch_handle_t bsp_touch_get_handle(void);

/* LVGL input device backing the touch panel. */
lv_indev_t *bsp_touch_get_indev(void);

/*
 * Which contact is going on, counted up on every finger that lands.
 *
 * Two events of one release — the gesture and the click — are dispatched microseconds apart
 * in the same call, but whatever runs between them is what matters. A number says whether the
 * click belongs to the contact that swiped, whatever the code in between cost.
 */
uint32_t bsp_touch_contact(void);

/* -------------------------------------------------------------------------- */
/* Runtime calibration                                                        */
/* -------------------------------------------------------------------------- */

/*
 * Axis corrections, applied to the controller's output.
 *
 * Leave them alone unless the panel is mounted rotated relative to the
 * display, which shows up as touches landing mirrored or transposed. The
 * defaults can be preset from menuconfig (Component config -> Knob BSP ->
 * Touch).
 *
 * Each call re-registers the touch device, so treat it as a setup-time
 * operation rather than something to do per frame.
 */
esp_err_t bsp_touch_set_swap_xy(bool swap);
esp_err_t bsp_touch_set_mirror(bool mirror_x, bool mirror_y);

#ifdef __cplusplus
}
#endif
