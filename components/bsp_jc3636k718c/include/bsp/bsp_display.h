/*
 * Display: ST77916 360x360 panel on QSPI, with the LVGL port attached.
 *
 * Bring-up order is not negotiable:
 *
 *   1. spi_bus_initialize            (QSPI on data0..data3)
 *   2. esp_lcd_new_panel_io_spi      (quad_mode, lcd_cmd_bits = 32)
 *   3. esp_lcd_new_panel_st77916     (flags.use_qspi_interface = 1)
 *   4. reset / init / disp_on_off
 *   5. ledc backlight
 *   6. lvgl_port_init
 *   7. lvgl_port_add_disp
 *
 * bsp_display_init() performs all seven. Touch and the encoder are added
 * afterwards by their own modules, because esp_lvgl_port needs the display to
 * exist before an input device can be bound to it.
 *
 * LVGL is not thread safe. Every call that touches a widget must be wrapped in
 * bsp_display_lock()/bsp_display_unlock().
 */
#pragma once

#include <stdbool.h>
#include <stdint.h>

#include "esp_err.h"
#include "esp_lcd_types.h"
#include "lvgl.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Bring up the panel and the LVGL port. Safe to call once from app_main.
 *
 * On success, LVGL is running in its own task and the screen shows whatever
 * the application draws into the active screen.
 */
esp_err_t bsp_display_init(void);

/* Undo bsp_display_init(). Rarely needed outside of tests. */
esp_err_t bsp_display_deinit(void);

/* The display registered with LVGL, or NULL before bsp_display_init(). */
lv_display_t *bsp_display_get_lvgl_display(void);

/* The active screen of the above, for convenience: lv_screen_active() is only
 * valid between lock/unlock, so this must be called under the lock too. */
lv_obj_t *bsp_display_get_screen(void);

/* Underlying ESP-IDF handles, for code that needs to call esp_lcd directly. */
esp_lcd_panel_handle_t    bsp_display_get_panel(void);
esp_lcd_panel_io_handle_t bsp_display_get_io(void);

/* -------------------------------------------------------------------------- */
/* LVGL mutex                                                                 */
/* -------------------------------------------------------------------------- */

/*
 * Take the LVGL lock. `timeout_ms` = 0 waits forever; a finite timeout returns
 * false when it expires.
 *
 * The LVGL task holds this lock whenever it renders or reads input, so any
 * application code that creates, modifies or deletes widgets — including from
 * an event callback that already runs inside the LVGL task — must hold it.
 * Calling it twice from the same thread deadlocks; do not nest.
 */
bool bsp_display_lock(uint32_t timeout_ms);

void bsp_display_unlock(void);

/* -------------------------------------------------------------------------- */
/* Backlight                                                                  */
/* -------------------------------------------------------------------------- */

/* 0..100. Values are clamped. Returns the value actually applied.
 *
 * Before the first frame has been drawn the panel is deliberately dark, and a call
 * here only REMEMBERS the level: the panel is lit by the first LVGL refresh, over a
 * 200 ms fade, or by a three-second fallback if no frame ever arrives. That is what
 * keeps boot from showing a lit, blank screen, and it is why this may be called at
 * any point in bring-up. The getter reports the level that was asked for either way. */
int bsp_display_brightness_set(int percent);

int bsp_display_brightness_get(void);

esp_err_t bsp_display_backlight_on(void);
esp_err_t bsp_display_backlight_off(void);

/*
 * Light the panel at the level already recorded, over the 200 ms fade. The application
 * calls this once its screen is built: before that the panel is dark on purpose, so
 * that boot cannot show a lit, blank screen. Safe to call more than once, and safe to
 * skip — a three-second fallback lights the panel anyway.
 */
void bsp_display_backlight_show(void);

/*
 * Screen orientation, 0..3, rotating clockwise.
 *
 * The panel is square, so this changes which way "up" is for the UI rather
 * than the aspect ratio.
 *
 * Takes the LVGL lock internally — do not call it while already holding the
 * lock, and do not call it from an LVGL event callback (those already run
 * inside the LVGL task).
 */
esp_err_t bsp_display_rotation_set(uint8_t rotation);

/* -------------------------------------------------------------------------- */
/* Raw drawing (no LVGL)                                                      */
/* -------------------------------------------------------------------------- */

/*
 * Hand the panel to the application. LVGL stays attached (touch and encoder are
 * LVGL input devices and need it) but its timers are disabled with lvgl_port_stop()
 * (under the LVGL lock), so it never renders or flushes again. The LVGL TASK keeps
 * running (1 kHz loop, takes the LVGL lock, reads input devices); the touch and
 * encoder samplers do not depend on it. The io "colour transfer done" callback is
 * replaced by one that releases bsp_display_draw_rect(). Call once, after bsp_init(),
 * from one task. The panel keeps whatever LVGL last drew (a black screen) until the
 * application draws over it.
 */
esp_err_t bsp_display_raw_begin(void);

/*
 * Blocking write of a w x h block at (x, y). `rgb565` holds the pixels row by row
 * in PANEL byte order (high byte first in memory) and must be DMA-capable internal
 * RAM. w * h * 2 must not exceed the SPI max transfer (BSP_LCD_H_RES *
 * CONFIG_BSP_LCD_BUFFER_LINES * 2 bytes). Returns when the transfer has finished,
 * so the buffer can be reused at once. One caller at a time, after
 * bsp_display_raw_begin().
 *
 * ESP_ERR_TIMEOUT means the transfer did not finish in 100 ms and may still be reading
 * `rgb565`: do not reuse or free that buffer. Later calls return ESP_ERR_INVALID_STATE
 * until the late completion arrives.
 */
esp_err_t bsp_display_draw_rect(int x, int y, int w, int h, const uint16_t *rgb565);

/* Largest pixel count one bsp_display_draw_rect() call accepts. */
uint32_t bsp_display_raw_max_pixels(void);

#ifdef __cplusplus
}
#endif
