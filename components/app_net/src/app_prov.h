/*
 * Compatibility shim for Wi-Fi provisioning.
 *
 * ESP-IDF 5.x ships the provisioning manager in the IDF tree as
 * `wifi_provisioning`. ESP-IDF 6.0 moved it out to the component registry as
 * `espressif/network_provisioning`, renamed almost every symbol, and — the part
 * that cannot be papered over with a #define — changed the application event
 * callback signature:
 *
 *   IDF 5.x:  void cb(void *user, esp_event_base_t base, int32_t id, void *data)
 *   IDF 6.x:  void cb(void *user, network_prov_cb_event_t event, void *data)
 *
 * This header maps the 5.x names onto whichever API is present, so that the
 * only #if that has to survive in app_net.c is the one for the callback
 * itself. Everything else reads as ordinary provisioning code.
 *
 * The Thread half of the 6.x API is ignored: this board is Wi-Fi only.
 */
#pragma once

#include "esp_idf_version.h"

#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(6, 0, 0)

#include "network_provisioning/manager.h"
#include "network_provisioning/scheme_softap.h"

/* Set when the registry-backed API is in use. app_net.c keys the callback
 * signature off this. */
#define APP_PROV_API_V2 1

#define APP_PROV_MGR_CONFIG_T       network_prov_mgr_config_t
#define APP_PROV_EVENT_HANDLER_T    network_prov_event_handler_t
#define APP_PROV_EVENT_HANDLER_NONE NETWORK_PROV_EVENT_HANDLER_NONE
#define APP_PROV_SCHEME_SOFTAP      network_prov_scheme_softap
#define APP_PROV_SECURITY_1         NETWORK_PROV_SECURITY_1
#define APP_PROV_FAIL_AUTH_ERROR    NETWORK_PROV_WIFI_STA_AUTH_ERROR
#define APP_PROV_FAIL_REASON_T      network_prov_wifi_sta_fail_reason_t

#define APP_PROV_EVT_START          NETWORK_PROV_START
#define APP_PROV_EVT_CRED_RECV      NETWORK_PROV_WIFI_CRED_RECV
#define APP_PROV_EVT_CRED_FAIL      NETWORK_PROV_WIFI_CRED_FAIL
#define APP_PROV_EVT_CRED_SUCCESS   NETWORK_PROV_WIFI_CRED_SUCCESS
#define APP_PROV_EVT_END            NETWORK_PROV_END

#define APP_PROV_MGR_INIT(cfg)                network_prov_mgr_init(cfg)
#define APP_PROV_MGR_IS_PROVISIONED(p)        network_prov_mgr_is_wifi_provisioned(p)
#define APP_PROV_MGR_START(sec, params, n, k) network_prov_mgr_start_provisioning(sec, params, n, k)
#define APP_PROV_MGR_DEINIT()                 network_prov_mgr_deinit()
#define APP_PROV_MGR_RESET()                  network_prov_mgr_reset_wifi_provisioning()
#define APP_PROV_MGR_RESET_ON_FAILURE()       network_prov_mgr_reset_wifi_sm_state_on_failure()

#else /* ESP-IDF < 6.0 */

#include "wifi_provisioning/manager.h"
#include "wifi_provisioning/scheme_softap.h"

#define APP_PROV_API_V2 0

#define APP_PROV_MGR_CONFIG_T       wifi_prov_mgr_config_t
#define APP_PROV_EVENT_HANDLER_T    wifi_prov_event_handler_t
#define APP_PROV_EVENT_HANDLER_NONE WIFI_PROV_EVENT_HANDLER_NONE
#define APP_PROV_SCHEME_SOFTAP      wifi_prov_scheme_softap
#define APP_PROV_SECURITY_1         WIFI_PROV_SECURITY_1
#define APP_PROV_FAIL_AUTH_ERROR    WIFI_PROV_STA_AUTH_ERROR
#define APP_PROV_FAIL_REASON_T      wifi_prov_sta_fail_reason_t

#define APP_PROV_EVT_START          WIFI_PROV_START
#define APP_PROV_EVT_CRED_RECV      WIFI_PROV_CRED_RECV
#define APP_PROV_EVT_CRED_FAIL      WIFI_PROV_CRED_FAIL
#define APP_PROV_EVT_CRED_SUCCESS   WIFI_PROV_CRED_SUCCESS
#define APP_PROV_EVT_END            WIFI_PROV_END

#define APP_PROV_MGR_INIT(cfg)                wifi_prov_mgr_init(cfg)
#define APP_PROV_MGR_IS_PROVISIONED(p)        wifi_prov_mgr_is_provisioned(p)
#define APP_PROV_MGR_START(sec, params, n, k) wifi_prov_mgr_start_provisioning(sec, params, n, k)
#define APP_PROV_MGR_DEINIT()                 wifi_prov_mgr_deinit()
#define APP_PROV_MGR_RESET()                  wifi_prov_mgr_reset_provisioning()
#define APP_PROV_MGR_RESET_ON_FAILURE()       wifi_prov_mgr_reset_sm_state_on_failure()

#endif
