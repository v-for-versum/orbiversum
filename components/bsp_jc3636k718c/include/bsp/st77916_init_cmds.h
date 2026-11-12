/*
 * ST77916 vendor initialisation sequence for the 1.85" 360x360 round panel
 * used on the Guition JC3636K718C.
 *
 * Transcribed from the vendor Arduino demo
 * (Demo_arduino/ST77916_LVGL_DEMO/scr_st77916.h, `lcd_init_cmd[]`), which in
 * turn matches "ST77916_T 1.8'' 360x360 initial code_V1.0_20241015.INI" in the
 * 4-Chip manual folder.
 *
 * WHEN YOU NEED THIS
 *   The esp_lcd_st77916 driver ships its own sequence (vendor_specific_init_
 *   default[]) that targets the same 360x360 panel and is expected to work
 *   here. Use this vendor list only if the screen shows garbage, wrong
 *   geometry, or a shifted image. Enable it from menuconfig:
 *
 *       Component config -> Knob BSP -> Use the vendor LCD init sequence
 *
 *   or by hand: CONFIG_BSP_LCD_USE_VENDOR_INIT=y.
 *
 * Do not "clean this up". The 0x60..0xBF block programs undocumented gamma and
 * frame-rate tables, and reordering it produces a blank or inverted screen.
 *
 * Note the command codes are plain 8-bit values. The driver wraps each one in
 * the 32-bit QSPI write-command opcode itself; do not pre-shift them.
 */
#pragma once

#include <stddef.h>
#include "esp_lcd_st77916.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Vendor sequence, 180 entries. The list ends with 0x11 (sleep out, 120 ms
 * delay) and 0x29 (display on), so `esp_lcd_panel_init()` is enough to bring
 * the panel up.
 *
 * 0x21 (INVON) is part of the sequence: this panel is wired for inverted
 * colour. Do not additionally call esp_lcd_panel_invert_color() — a second
 * inversion produces a photographic negative.
 */
extern const st77916_lcd_init_cmd_t st77916_vendor_init_cmds[];
extern const size_t st77916_vendor_init_cmds_size;

#ifdef __cplusplus
}
#endif
