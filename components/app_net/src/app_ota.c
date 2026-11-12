/*
 * OTA firmware update over HTTP(S).
 *
 * The image is streamed into the inactive OTA slot; the running application
 * keeps working until the transfer has been verified. Nothing is committed
 * until esp_https_ota_finish() validates the image, so a failed or interrupted
 * download leaves the device on its current firmware.
 *
 * Rollback: after the new image boots, the bootloader is waiting for
 * app_ota_mark_valid(). If the application never calls it — because the new
 * firmware crashes or hangs before reaching that point — the next reboot falls
 * back to the previous slot. Call it from app_main once the UI is up.
 */

#include "app_net.h"

#include <inttypes.h>
#include <string.h>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "esp_app_desc.h"
#include "esp_crt_bundle.h"
#include "esp_http_client.h"
#include "esp_https_ota.h"
#include "esp_log.h"
#include "esp_ota_ops.h"
#include "esp_system.h"

static const char *TAG = "app_ota";

/* Report at most this often, so a slow link does not flood the log or the UI. */
#define OTA_PROGRESS_MIN_INTERVAL_MS 200

esp_err_t app_ota_run(const app_ota_config_t *cfg)
{
    if (!cfg || !cfg->url || cfg->url[0] == '\0') {
        return ESP_ERR_INVALID_ARG;
    }

    const esp_app_desc_t *running = esp_app_get_description();
    ESP_LOGI(TAG, "current firmware: %s (%s)", running->version, running->project_name);
    ESP_LOGI(TAG, "updating from: %s", cfg->url);

    esp_http_client_config_t http_cfg = {
        .url               = cfg->url,
        .timeout_ms        = 20000,
        .keep_alive_enable = true,
        /* Verifies HTTPS servers against the bundled Mozilla root store.
         * Harmless for plain-HTTP URLs, which are also accepted — useful on a
         * trusted LAN or when serving the binary from a local script. */
        .crt_bundle_attach = esp_crt_bundle_attach,
    };

    esp_https_ota_config_t ota_cfg = {
        .http_config = &http_cfg,
    };

    esp_https_ota_handle_t handle = NULL;
    esp_err_t err = esp_https_ota_begin(&ota_cfg, &handle);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "begin failed: %s", esp_err_to_name(err));
        return err;
    }

    const int image_size = esp_https_ota_get_image_size(handle);
    ESP_LOGI(TAG, "image size: %d bytes", image_size);

    int64_t last_report_ms = 0;
    int last_percent = -1;

    while (true) {
        err = esp_https_ota_perform(handle);
        if (err != ESP_ERR_HTTPS_OTA_IN_PROGRESS) {
            break;
        }

        if (cfg->progress_cb) {
            const int64_t now_ms = esp_log_timestamp();
            int percent = -1;

            if (image_size > 0) {
                const int read = esp_https_ota_get_image_len_read(handle);
                percent = (int)((int64_t)read * 100 / image_size);
            }

            if (percent != last_percent &&
                (now_ms - last_report_ms) >= OTA_PROGRESS_MIN_INTERVAL_MS) {
                last_percent   = percent;
                last_report_ms = now_ms;
                cfg->progress_cb(percent, cfg->user);
            }
        }
    }

    if (!esp_https_ota_is_complete_data_received(handle)) {
        ESP_LOGE(TAG, "transfer ended early");
        esp_https_ota_abort(handle);
        return ESP_ERR_INVALID_SIZE;
    }

    if (err != ESP_OK) {
        ESP_LOGE(TAG, "download failed: %s", esp_err_to_name(err));
        esp_https_ota_abort(handle);
        return err;
    }

    /* Validates the image signature/checksum and switches the boot partition. */
    err = esp_https_ota_finish(handle);
    if (err == ESP_ERR_OTA_VALIDATE_FAILED) {
        ESP_LOGE(TAG, "image validation failed — is this a real firmware binary?");
        return err;
    }
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "finish failed: %s", esp_err_to_name(err));
        return err;
    }

    if (cfg->progress_cb) {
        cfg->progress_cb(100, cfg->user);
    }
    ESP_LOGI(TAG, "update staged successfully");

    if (cfg->reboot_on_success) {
        ESP_LOGI(TAG, "rebooting in 1 s");
        vTaskDelay(pdMS_TO_TICKS(1000));
        esp_restart();
    }
    return ESP_OK;
}

esp_err_t app_ota_mark_valid(void)
{
    const esp_partition_t *running = esp_ota_get_running_partition();
    esp_ota_img_states_t state;

    esp_err_t err = esp_ota_get_state_partition(running, &state);
    if (err != ESP_OK) {
        /* No OTA state — this image was flashed over serial, nothing to
         * confirm. Not an error. */
        ESP_LOGD(TAG, "no OTA state for the running partition: %s", esp_err_to_name(err));
        return ESP_OK;
    }

    if (state != ESP_OTA_IMG_PENDING_VERIFY) {
        ESP_LOGD(TAG, "nothing to confirm (state %d)", (int)state);
        return ESP_OK;
    }

    err = esp_ota_mark_app_valid_cancel_rollback();
    if (err == ESP_OK) {
        ESP_LOGI(TAG, "firmware confirmed — rollback cancelled");
    } else {
        ESP_LOGE(TAG, "could not confirm firmware: %s", esp_err_to_name(err));
    }
    return err;
}
