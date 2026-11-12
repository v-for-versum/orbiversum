/*
 * Network services: Wi-Fi provisioning, connection management and OTA.
 *
 * The knob has no keyboard, so credentials are never typed on the device.
 * Instead the first boot brings up a SoftAP named after the device; you open
 * http://192.168.4.1 on a phone, pick your network and enter the password.
 * The credentials land in NVS and are reused on every later boot.
 *
 * If CONFIG_APP_NET_USE_STATIC_CREDENTIALS is enabled, compressed-in
 * credentials from menuconfig are used instead and provisioning is skipped —
 * handy during development.
 */
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------- */
/* Connection state                                                           */
/* -------------------------------------------------------------------------- */
typedef enum {
    APP_NET_STATE_IDLE = 0,     /*!< Not started yet */
    APP_NET_STATE_PROVISIONING, /*!< SoftAP is up, waiting for a phone */
    APP_NET_STATE_CONNECTING,   /*!< Credentials known, associating */
    APP_NET_STATE_CONNECTED,    /*!< Got an IP address */
    APP_NET_STATE_DISCONNECTED, /*!< Lost the link, retrying */
    APP_NET_STATE_FAILED,       /*!< Gave up (bad credentials, AP unreachable) */
} app_net_state_t;

/* Called from the system event task. Keep it short and do not block: hand off
 * to a queue or a notification if the UI needs to react. */
typedef void (*app_net_state_cb_t)(app_net_state_t state, void *user);

/*
 * Initialise NVS, esp_netif and the Wi-Fi driver, then start provisioning or
 * connect with stored/static credentials depending on what is available.
 * Safe to call once from app_main. Returns as soon as the attempt is under
 * way — use app_net_wait_connected() to block until the link is up.
 */
esp_err_t app_net_init(void);

/* Current state. Updated from the Wi-Fi/IP event handlers. */
app_net_state_t app_net_get_state(void);

/* Human-readable state name, for logs and the on-screen status. */
const char *app_net_state_name(app_net_state_t state);

/* True once an IP address has been obtained. */
bool app_net_is_connected(void);

/* Block until connected or the timeout elapses (0 = wait forever). */
esp_err_t app_net_wait_connected(uint32_t timeout_ms);

/* Subscribe to state changes. Pass NULL to unsubscribe. */
esp_err_t app_net_register_state_cb(app_net_state_cb_t cb, void *user);

/* Erase stored credentials and reboot into provisioning. */
esp_err_t app_net_reset_provisioning(void);

/* Copy the SoftAP SSID used during provisioning into `out`. */
esp_err_t app_net_get_provisioning_ssid(char *out, size_t out_len);

/* -------------------------------------------------------------------------- */
/* OTA                                                                        */
/* -------------------------------------------------------------------------- */

/* Report download progress; percent is 0..100, or -1 when unknown. */
typedef void (*app_ota_progress_cb_t)(int percent, void *user);

typedef struct {
    const char *url;                  /*!< http:// or https:// firmware URL */
    bool reboot_on_success;           /*!< Reboot automatically when done */
    app_ota_progress_cb_t progress_cb;/*!< Optional, may be NULL */
    void *user;                       /*!< Passed to progress_cb */
} app_ota_config_t;

/*
 * Download and flash firmware from cfg->url into the inactive OTA slot.
 *
 * The URL may be plain HTTP on the LAN or HTTPS. For HTTPS the server's
 * certificate must be reachable — either embed a CA with
 * esp_crt_bundle_attach, or accept the risk explicitly on a trusted LAN.
 *
 * This function does not return on success when reboot_on_success is set.
 */
esp_err_t app_ota_run(const app_ota_config_t *cfg);

/*
 * Confirm that the running image is healthy.
 *
 * The bootloader rolls back to the previous slot if a freshly flashed image
 * never calls this. applications should call it once the UI is up and the
 * self-test has passed — typically a few seconds after app_main. Calling it
 * when there is nothing to roll back is harmless.
 */
esp_err_t app_ota_mark_valid(void);

#ifdef __cplusplus
}
#endif
