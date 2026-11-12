/*
 * microSD over SDMMC.
 *
 * The card detect signal is not routed on this board, so "no card inserted"
 * shows up as a mount failure rather than as a separate pin state.
 */

#include "bsp/bsp_sdcard.h"

#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "esp_vfs_fat.h"
#include "sdmmc_cmd.h"

#include "driver/sdmmc_host.h"
#include "driver/sdmmc_types.h"

#include "bsp/bsp_pins.h"

static const char *TAG = "bsp.sd";

/* Both the mount handle and the card pointer must stay alive for as long as
 * the filesystem is mounted — esp_vfs_fat_sdmmc_unmount() frees the card. */
static sdmmc_card_t *s_card;
static bool          s_mounted;

esp_err_t bsp_sdcard_mount_1bit(void)
{
    if (s_mounted) {
        ESP_LOGW(TAG, "already mounted");
        return ESP_OK;
    }

    esp_vfs_fat_sdmmc_mount_config_t mount_cfg = {
        .format_if_mount_failed = false,
        .max_files              = 8,
        /* 16 KB clusters keep small asset files from wasting space while
         * still giving decent sequential throughput. */
        .allocation_unit_size   = 16 * 1024,
    };

    sdmmc_host_t host = SDMMC_HOST_DEFAULT();
    host.flags        = SDMMC_HOST_FLAG_1BIT;
    host.max_freq_khz = SDMMC_FREQ_HIGHSPEED;

    sdmmc_slot_config_t slot_cfg = SDMMC_SLOT_CONFIG_DEFAULT();
    slot_cfg.width = 1;
    slot_cfg.clk   = BSP_SD_CLK;
    slot_cfg.cmd   = BSP_SD_CMD;
    slot_cfg.d0    = BSP_SD_D0;
    /* Internal pull-ups are required: the board does not carry external ones
     * on the SD lines, and without them the card is unreliable at 40 MHz. */
    slot_cfg.flags |= SDMMC_SLOT_FLAG_INTERNAL_PULLUP;

    ESP_LOGI(TAG, "mounting 1-bit at %s", BSP_SD_MOUNT_POINT);

    esp_err_t err = esp_vfs_fat_sdmmc_mount(BSP_SD_MOUNT_POINT, &host, &slot_cfg,
                                            &mount_cfg, &s_card);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "1-bit mount failed: %s", esp_err_to_name(err));
        s_card = NULL;
        return err;
    }

    s_mounted = true;
    sdmmc_card_print_info(stdout, s_card);
    return ESP_OK;
}

esp_err_t bsp_sdcard_mount(void)
{
    if (s_mounted) {
        ESP_LOGW(TAG, "already mounted");
        return ESP_OK;
    }

    esp_vfs_fat_sdmmc_mount_config_t mount_cfg = {
        .format_if_mount_failed = false,
        .max_files              = 8,
        .allocation_unit_size   = 16 * 1024,
    };

    sdmmc_host_t host = SDMMC_HOST_DEFAULT();
    host.flags        = SDMMC_HOST_FLAG_4BIT;
    host.max_freq_khz = SDMMC_FREQ_HIGHSPEED;

    sdmmc_slot_config_t slot_cfg = SDMMC_SLOT_CONFIG_DEFAULT();
    slot_cfg.width = 4;
    slot_cfg.clk   = BSP_SD_CLK;
    slot_cfg.cmd   = BSP_SD_CMD;
    slot_cfg.d0    = BSP_SD_D0;
    slot_cfg.d1    = BSP_SD_D1;
    slot_cfg.d2    = BSP_SD_D2;
    slot_cfg.d3    = BSP_SD_D3;
    slot_cfg.flags |= SDMMC_SLOT_FLAG_INTERNAL_PULLUP;

    ESP_LOGI(TAG, "mounting 4-bit at %s", BSP_SD_MOUNT_POINT);

    esp_err_t err = esp_vfs_fat_sdmmc_mount(BSP_SD_MOUNT_POINT, &host, &slot_cfg,
                                            &mount_cfg, &s_card);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "4-bit mount failed: %s", esp_err_to_name(err));
        s_card = NULL;
        /* Data lines 1..3 are the usual culprit; try the narrow bus before
         * reporting the card as unreadable. */
        ESP_LOGI(TAG, "retrying in 1-bit mode");
        return bsp_sdcard_mount_1bit();
    }

    s_mounted = true;
    sdmmc_card_print_info(stdout, s_card);
    return ESP_OK;
}

esp_err_t bsp_sdcard_unmount(void)
{
    if (!s_mounted) {
        return ESP_OK;
    }

    esp_err_t err = esp_vfs_fat_sdcard_unmount(BSP_SD_MOUNT_POINT, s_card);
    if (err == ESP_OK) {
        s_card    = NULL;
        s_mounted = false;
    }
    return err;
}

bool bsp_sdcard_is_mounted(void)
{
    return s_mounted;
}

esp_err_t bsp_sdcard_get_space(uint64_t *total_bytes, uint64_t *free_bytes)
{
    if (!s_mounted) {
        return ESP_ERR_INVALID_STATE;
    }
    return esp_vfs_fat_info(BSP_SD_MOUNT_POINT, total_bytes, free_bytes);
}

uint32_t bsp_sdcard_get_capacity_mb(void)
{
    if (!s_mounted || !s_card) {
        return 0;
    }
    return (uint32_t)(((uint64_t)s_card->csd.capacity * s_card->csd.sector_size) /
                      (1024ULL * 1024ULL));
}
