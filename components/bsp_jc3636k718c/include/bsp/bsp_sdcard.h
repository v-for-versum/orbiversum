/*
 * microSD card over SDMMC.
 *
 * The slot is wired 4-bit (CLK 39, CMD 38, D0..D3 = 40, 41, 48, 47), which is
 * what makes the photo-frame and audio-player examples fast. A 1-bit mode is
 * also offered: if a card or a board revision misbehaves in 4-bit mode, 1-bit
 * is the usual fallback, at roughly a quarter of the throughput.
 *
 * Mounting is optional for the application. Everything the UI needs can also
 * live in the on-board SPIFFS `storage` partition, which is faster and always
 * present.
 */
#pragma once

#include <stdbool.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Mount the card at BSP_SD_MOUNT_POINT ("/sdcard").
 *
 * format_if_mount_failed is deliberately not exposed: silently reformatting a
 * user's card because of a wiring glitch is a good way to lose data. If the
 * card is unformatted, format it on a PC.
 *
 * Returns ESP_ERR_NOT_FOUND when no card is inserted, ESP_FAIL on a mount
 * failure (usually a filesystem the ESP32 cannot read, such as exFAT).
 */
esp_err_t bsp_sdcard_mount(void);

/* Mount using a single data line. Use this if 4-bit mode is unreliable. */
esp_err_t bsp_sdcard_mount_1bit(void);

/* Unmount and release the SDMMC host. */
esp_err_t bsp_sdcard_unmount(void);

bool bsp_sdcard_is_mounted(void);

/* Total and free space in bytes. Returns ESP_ERR_INVALID_STATE if unmounted. */
esp_err_t bsp_sdcard_get_space(uint64_t *total_bytes, uint64_t *free_bytes);

/* Card capacity in MB, or 0 when unmounted. */
uint32_t bsp_sdcard_get_capacity_mb(void);

#ifdef __cplusplus
}
#endif
