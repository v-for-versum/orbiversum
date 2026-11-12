/*
 * Board bring-up orchestration — see bsp.h for the contract.
 */

#include "bsp/bsp.h"

#include "esp_app_desc.h"
#include "esp_chip_info.h"
#include "esp_flash.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_system.h"

#include "freertos/FreeRTOS.h"

#include "bsp_i2c.h"

static const char *TAG = "bsp";

/* Remember the first failure but keep going: a knob with a dead SD card beats
 * a knob that refuses to boot. */
#define BSP_TRY(expr, what)                                     \
    do {                                                        \
        esp_err_t err_ = (expr);                                \
        if (err_ != ESP_OK) {                                   \
            ESP_LOGW(TAG, "%s failed: %s", (what),              \
                     esp_err_to_name(err_));                    \
            if (first_error == ESP_OK) {                        \
                first_error = err_;                             \
            }                                                   \
        } else {                                                \
            ESP_LOGD(TAG, "%s ok", (what));                     \
        }                                                       \
    } while (0)

esp_err_t bsp_init(const bsp_features_t *features)
{
    static const bsp_features_t defaults = BSP_FEATURES_DEFAULT();
    if (!features) {
        features = &defaults;
    }

    if (!features->display && (features->touch || features->encoder)) {
        ESP_LOGE(TAG, "touch and encoder need the display: enable features.display");
        return ESP_ERR_INVALID_ARG;
    }

    esp_err_t first_error = ESP_OK;

    /* --- Display first: everything graphical depends on it ---------------- */
    if (features->display) {
        BSP_TRY(bsp_display_init(), "display");
    }

    /* --- Input ------------------------------------------------------------ */
    if (features->touch) {
        BSP_TRY(bsp_touch_init(), "touch");
    }
    if (features->encoder) {
        BSP_TRY(bsp_encoder_init(), "encoder");
    }

    /* --- Outputs and storage ---------------------------------------------- */
    if (features->led_ring) {
        BSP_TRY(bsp_led_init(), "led ring");
    }
    if (features->audio_out) {
        const bsp_audio_out_config_t audio_out_cfg = BSP_AUDIO_OUT_CONFIG_DEFAULT();
        BSP_TRY(bsp_audio_out_init(&audio_out_cfg), "audio out");
    }
    if (features->audio_in) {
        /* The BSP macro says 16 kHz, but this instrument is 48 kHz end to end and the
         * microphone is read one engine block at a time in the audio task, so the PDM
         * clock is run at 48 kHz too. This number is the only place the microphone rate
         * is set. */
        bsp_audio_in_config_t audio_in_cfg = BSP_AUDIO_IN_CONFIG_DEFAULT();
        audio_in_cfg.sample_rate = 48000;
        /* Gain is left at the default 0 dB (unity, amplify_num 1). The microphone's
         * sensitivity setting is what moves it afterwards, through
         * bsp_audio_in_set_gain(): the channel is brought up here, once, while the
         * internal DMA pool still has room for its descriptors, and is never torn down
         * and rebuilt. */
        BSP_TRY(bsp_audio_in_init(&audio_in_cfg), "audio in");
    }
    if (features->sdcard) {
        BSP_TRY(bsp_sdcard_mount(), "sdcard");
    }

    /* --- Readouts ---------------------------------------------------------- */
    if (features->battery) {
        BSP_TRY(bsp_battery_init(), "battery");
    }

    if (first_error == ESP_OK) {
        ESP_LOGI(TAG, "board ready");
    } else {
        ESP_LOGW(TAG, "board ready with warnings (first: %s)",
                 esp_err_to_name(first_error));
    }
    return first_error;
}

uint32_t bsp_get_tick_rate_hz(void)
{
    return (uint32_t)configTICK_RATE_HZ;
}

void bsp_print_info(void)
{
    const esp_app_desc_t *app = esp_app_get_description();

    esp_chip_info_t chip;
    esp_chip_info(&chip);

    ESP_LOGI(TAG, "======================================================");
    ESP_LOGI(TAG, " Guition JC3636K718C  |  1.85\" round knob screen");
    ESP_LOGI(TAG, "======================================================");
    ESP_LOGI(TAG, " Firmware   : %s  (%s)", app->version, app->project_name);
    ESP_LOGI(TAG, " IDF        : %s", app->idf_ver);
    ESP_LOGI(TAG, " Chip       : ESP32-S3 rev %d, %d core(s) @ %d MHz",
             chip.revision, chip.cores, CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ);
    /* IDF 6.x takes the size as an out-parameter rather than returning it. */
    uint32_t flash_size = 0;
    esp_flash_get_size(NULL, &flash_size);
    ESP_LOGI(TAG, " Flash      : %u MB", (unsigned)(flash_size / (1024 * 1024)));
    ESP_LOGI(TAG, " Internal   : %u KB free of %u KB",
             (unsigned)(heap_caps_get_free_size(MALLOC_CAP_INTERNAL) / 1024),
             (unsigned)(heap_caps_get_total_size(MALLOC_CAP_INTERNAL) / 1024));
    ESP_LOGI(TAG, " PSRAM      : %u KB free of %u KB",
             (unsigned)(heap_caps_get_free_size(MALLOC_CAP_SPIRAM) / 1024),
             (unsigned)(heap_caps_get_total_size(MALLOC_CAP_SPIRAM) / 1024));
    ESP_LOGI(TAG, " Free heap  : %u KB",
             (unsigned)(esp_get_free_heap_size() / 1024));
    ESP_LOGI(TAG, "------------------------------------------------------");
    ESP_LOGI(TAG, " Display    : %s", bsp_display_get_lvgl_display() ? "up" : "off");
    ESP_LOGI(TAG, " Touch      : %s", bsp_touch_is_ready() ? "up" : "off");
    ESP_LOGI(TAG, " Encoder    : %s", bsp_encoder_is_ready() ? "up" : "off");
    ESP_LOGI(TAG, " LED ring   : %s",
             bsp_led_is_available() ? "up" : "not configured");
    ESP_LOGI(TAG, " Audio out  : %s", bsp_audio_out_is_ready() ? "up" : "off");
    ESP_LOGI(TAG, " Audio in   : %s", bsp_audio_in_is_ready() ? "up" : "off");
    ESP_LOGI(TAG, " microSD    : %s",
             bsp_sdcard_is_mounted() ? "mounted at " BSP_SD_MOUNT_POINT : "not mounted");
    ESP_LOGI(TAG, "======================================================");
}
