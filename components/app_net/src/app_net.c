/*
 * Wi-Fi provisioning and connection management.
 *
 * Flow:
 *   first boot  -> SoftAP "<prefix>_<mac>" comes up; a phone opens
 *                  http://192.168.4.1, picks the network and enters the
 *                  password; provisioning tears the SoftAP down and the
 *                  station connects.
 *   later boots -> wifi_prov_mgr finds credentials in NVS and connects
 *                  straight away.
 *
 * Everything is driven from the default event loop, so no dedicated task is
 * needed here; app_net_wait_connected() simply blocks on an event group.
 */

#include "app_net.h"

#include <stdio.h>
#include <string.h>

#include "freertos/FreeRTOS.h"
#include "freertos/event_groups.h"
#include "freertos/task.h"

#include "esp_event.h"
#include "esp_log.h"
#include "esp_mac.h"
#include "esp_netif.h"
#include "esp_wifi.h"
#include "nvs_flash.h"

/* Maps onto whichever provisioning API this IDF generation ships — see
 * app_prov.h for why this is not a plain include. */
#include "app_prov.h"

static const char *TAG = "app_net";

/* Event-group bits. */
#define NET_BIT_CONNECTED   BIT0
#define NET_BIT_FAILED      BIT1

/* A disconnect right after association is usually a wrong password rather
 * than a bad access point, so count attempts and give up eventually instead
 * of retrying forever and draining the battery. */
#define NET_DISCONNECT_GRACE_MS 2000

static EventGroupHandle_t s_net_events;
static app_net_state_t    s_state = APP_NET_STATE_IDLE;
static app_net_state_cb_t s_state_cb;
static void              *s_state_cb_user;
static int                s_retry_count;
static bool               s_initialised;
static bool               s_using_provisioning;
static char               s_prov_ssid[33];
static char               s_hostname[33];

/* -------------------------------------------------------------------------- */
/* Small helpers                                                              */
/* -------------------------------------------------------------------------- */

static void set_state(app_net_state_t state)
{
    if (state == s_state) {
        return;
    }
    s_state = state;
    ESP_LOGI(TAG, "state -> %s", app_net_state_name(state));

    if (s_state_cb) {
        s_state_cb(state, s_state_cb_user);
    }
}

const char *app_net_state_name(app_net_state_t state)
{
    switch (state) {
    case APP_NET_STATE_IDLE:         return "idle";
    case APP_NET_STATE_PROVISIONING: return "provisioning";
    case APP_NET_STATE_CONNECTING:   return "connecting";
    case APP_NET_STATE_CONNECTED:    return "connected";
    case APP_NET_STATE_DISCONNECTED: return "disconnected";
    case APP_NET_STATE_FAILED:       return "failed";
    default:                         return "unknown";
    }
}

esp_err_t app_net_register_state_cb(app_net_state_cb_t cb, void *user)
{
    s_state_cb      = cb;
    s_state_cb_user = user;
    return ESP_OK;
}

app_net_state_t app_net_get_state(void)
{
    return s_state;
}

bool app_net_is_connected(void)
{
    return s_state == APP_NET_STATE_CONNECTED;
}

esp_err_t app_net_get_provisioning_ssid(char *out, size_t out_len)
{
    if (!out || out_len == 0) {
        return ESP_ERR_INVALID_ARG;
    }
    if (!s_using_provisioning || s_prov_ssid[0] == '\0') {
        return ESP_ERR_INVALID_STATE;
    }
    strlcpy(out, s_prov_ssid, out_len);
    return ESP_OK;
}

/* -------------------------------------------------------------------------- */
/* Event handlers                                                             */
/* -------------------------------------------------------------------------- */

static void wifi_event_handler(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg;
    (void)base;

    switch (id) {
    case WIFI_EVENT_STA_START:
        ESP_LOGD(TAG, "station started");
        break;

    case WIFI_EVENT_STA_CONNECTED:
        ESP_LOGI(TAG, "associated, waiting for DHCP");
        set_state(APP_NET_STATE_CONNECTING);
        break;

    case WIFI_EVENT_STA_DISCONNECTED: {
        const wifi_event_sta_disconnected_t *disc = (const wifi_event_sta_disconnected_t *)data;
        ESP_LOGW(TAG, "disconnected, reason %d", disc ? disc->reason : -1);

        if (s_using_provisioning) {
            /* The provisioning manager owns reconnection while it is active. */
            break;
        }

        s_retry_count++;
        if (s_retry_count > CONFIG_APP_NET_RETRY_MAX) {
            ESP_LOGE(TAG, "giving up after %d attempts", s_retry_count - 1);
            set_state(APP_NET_STATE_FAILED);
            if (s_net_events) {
                xEventGroupSetBits(s_net_events, NET_BIT_FAILED);
            }
            break;
        }

        set_state(APP_NET_STATE_DISCONNECTED);
        ESP_LOGI(TAG, "reconnect attempt %d/%d", s_retry_count, CONFIG_APP_NET_RETRY_MAX);
        esp_wifi_connect();
        break;
    }

    default:
        break;
    }
}

static void ip_event_handler(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg;
    (void)base;

    if (id == IP_EVENT_STA_GOT_IP) {
        const ip_event_got_ip_t *event = (const ip_event_got_ip_t *)data;
        ESP_LOGI(TAG, "got IP " IPSTR, IP2STR(&event->ip_info.ip));
        s_retry_count = 0;
        set_state(APP_NET_STATE_CONNECTED);
        if (s_net_events) {
            xEventGroupClearBits(s_net_events, NET_BIT_FAILED);
            xEventGroupSetBits(s_net_events, NET_BIT_CONNECTED);
        }
    }
}

static void handle_prov_event(int32_t id, void *data)
{
    switch (id) {
    case APP_PROV_EVT_START:
        ESP_LOGI(TAG, "provisioning started — join \"%s\" and open http://192.168.4.1",
                 s_prov_ssid);
        set_state(APP_NET_STATE_PROVISIONING);
        break;

    case APP_PROV_EVT_CRED_RECV: {
        const wifi_sta_config_t *sta = (const wifi_sta_config_t *)data;
        ESP_LOGI(TAG, "received credentials for \"%s\"", sta ? (const char *)sta->ssid : "?");
        break;
    }

    case APP_PROV_EVT_CRED_FAIL: {
        const APP_PROV_FAIL_REASON_T *reason = (const APP_PROV_FAIL_REASON_T *)data;
        ESP_LOGE(TAG, "provisioning failed: %s",
                 (reason && *reason == APP_PROV_FAIL_AUTH_ERROR) ? "wrong password"
                                                                 : "access point not found");
        /* Restart so the user can try again without power-cycling. */
        APP_PROV_MGR_RESET_ON_FAILURE();
        set_state(APP_NET_STATE_PROVISIONING);
        break;
    }

    case APP_PROV_EVT_CRED_SUCCESS:
        ESP_LOGI(TAG, "provisioning succeeded");
        break;

    case APP_PROV_EVT_END:
        /* The SoftAP is gone by now; the station takes over. */
        APP_PROV_MGR_DEINIT();
        break;

    default:
        break;
    }
}

/*
 * Only the callback signature differs between the two provisioning APIs: the
 * IDF 5.x manager delivers esp_event-style arguments, the 6.x one delivers a
 * plain event enum. Both funnel into handle_prov_event() above.
 */
#if APP_PROV_API_V2
static void prov_event_handler(void *user, network_prov_cb_event_t event, void *data)
{
    (void)user;
    handle_prov_event((int32_t)event, data);
}
#else
static void prov_event_handler(void *user, esp_event_base_t base, int32_t event, void *data)
{
    (void)user;
    (void)base;
    handle_prov_event(event, data);
}
#endif

/* -------------------------------------------------------------------------- */
/* Setup helpers                                                              */
/* -------------------------------------------------------------------------- */

static void build_provisioning_ssid(void)
{
    uint8_t mac[6] = {0};
    esp_read_mac(mac, ESP_MAC_WIFI_STA);
    snprintf(s_prov_ssid, sizeof(s_prov_ssid), "%s_%02X%02X%02X",
             CONFIG_APP_NET_PROV_PREFIX, mac[3], mac[4], mac[5]);
}

static esp_err_t init_nvs(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_LOGW(TAG, "NVS needs reformatting");
        ESP_ERROR_CHECK(nvs_flash_erase());
        err = nvs_flash_init();
    }
    return err;
}

/*
 * Only compiled when the menuconfig option is on. The SSID and password
 * symbols this reads are themselves gated behind the same option, so without
 * the guard the file does not compile at all — Kconfig simply does not emit
 * CONFIG_APP_NET_STATIC_SSID when the feature is off.
 */
#if CONFIG_APP_NET_USE_STATIC_CREDENTIALS
static esp_err_t connect_with_static_credentials(void)
{
    wifi_config_t wifi_config = {0};

    strlcpy((char *)wifi_config.sta.ssid, CONFIG_APP_NET_STATIC_SSID,
            sizeof(wifi_config.sta.ssid));
    strlcpy((char *)wifi_config.sta.password, CONFIG_APP_NET_STATIC_PASSWORD,
            sizeof(wifi_config.sta.password));

    if (wifi_config.sta.password[0] == '\0') {
        wifi_config.sta.threshold.authmode = WIFI_AUTH_OPEN;
    } else {
        /* WPA2 as a floor: anything weaker is refused by the AP anyway, and
         * this keeps the association from silently downgrading. */
        wifi_config.sta.threshold.authmode = WIFI_AUTH_WPA2_PSK;
    }

    ESP_LOGI(TAG, "connecting to \"%s\" (static credentials)", wifi_config.sta.ssid);

    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wifi_config));
    ESP_ERROR_CHECK(esp_wifi_start());

    set_state(APP_NET_STATE_CONNECTING);
    return esp_wifi_connect();
}
#endif /* CONFIG_APP_NET_USE_STATIC_CREDENTIALS */

/* -------------------------------------------------------------------------- */
/* Public API                                                                 */
/* -------------------------------------------------------------------------- */

esp_err_t app_net_init(void)
{
    if (s_initialised) {
        return ESP_OK;
    }
    s_initialised = true;

    ESP_ERROR_CHECK(init_nvs());

    s_net_events = xEventGroupCreate();
    if (!s_net_events) {
        return ESP_ERR_NO_MEM;
    }

    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());

    esp_netif_t *sta_netif = esp_netif_create_default_wifi_sta();
    if (!sta_netif) {
        return ESP_FAIL;
    }

    snprintf(s_hostname, sizeof(s_hostname), "%s", CONFIG_APP_NET_HOSTNAME);
    esp_netif_set_hostname(sta_netif, s_hostname);

    wifi_init_config_t wifi_init_cfg = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&wifi_init_cfg));

    ESP_ERROR_CHECK(esp_event_handler_instance_register(WIFI_EVENT, ESP_EVENT_ANY_ID,
                                                        wifi_event_handler, NULL, NULL));
    ESP_ERROR_CHECK(esp_event_handler_instance_register(IP_EVENT, IP_EVENT_STA_GOT_IP,
                                                        ip_event_handler, NULL, NULL));

    /* Keep the radio config in RAM only — credentials belong in NVS through
     * the provisioning manager, not in the driver's own flash store. */
    ESP_ERROR_CHECK(esp_wifi_set_storage(WIFI_STORAGE_RAM));

#if CONFIG_APP_NET_USE_STATIC_CREDENTIALS
    s_using_provisioning = false;
    ESP_LOGW(TAG, "static credentials are enabled — do not ship this build");
    return connect_with_static_credentials();
#else
    APP_PROV_MGR_CONFIG_T prov_cfg = {
        .scheme               = APP_PROV_SCHEME_SOFTAP,
        .scheme_event_handler = APP_PROV_EVENT_HANDLER_NONE,
        .app_event_handler    = {
            .event_cb  = prov_event_handler,
            .user_data = NULL,
        },
    };

    ESP_ERROR_CHECK(APP_PROV_MGR_INIT(prov_cfg));

    bool provisioned = false;
    ESP_ERROR_CHECK(APP_PROV_MGR_IS_PROVISIONED(&provisioned));

    if (provisioned) {
        ESP_LOGI(TAG, "stored credentials found");
        APP_PROV_MGR_DEINIT();

        ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
        ESP_ERROR_CHECK(esp_wifi_start());

        set_state(APP_NET_STATE_CONNECTING);
        return esp_wifi_connect();
    }

    build_provisioning_ssid();
    s_using_provisioning = true;

    esp_netif_create_default_wifi_ap();

    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_APSTA));
    ESP_ERROR_CHECK(esp_wifi_start());

    ESP_LOGI(TAG, "starting provisioning as \"%s\"", s_prov_ssid);
    ESP_ERROR_CHECK(APP_PROV_MGR_START(APP_PROV_SECURITY_1,
                                       CONFIG_APP_NET_PROV_POP,
                                       s_prov_ssid, NULL));
    return ESP_OK;
#endif
}

esp_err_t app_net_wait_connected(uint32_t timeout_ms)
{
    if (!s_net_events) {
        return ESP_ERR_INVALID_STATE;
    }
    if (s_state == APP_NET_STATE_CONNECTED) {
        return ESP_OK;
    }

    TickType_t ticks = (timeout_ms == 0) ? portMAX_DELAY : pdMS_TO_TICKS(timeout_ms);
    EventBits_t bits = xEventGroupWaitBits(s_net_events,
                                           NET_BIT_CONNECTED | NET_BIT_FAILED,
                                           pdFALSE, pdFALSE, ticks);

    if (bits & NET_BIT_CONNECTED) {
        return ESP_OK;
    }
    if (bits & NET_BIT_FAILED) {
        return ESP_FAIL;
    }
    return ESP_ERR_TIMEOUT;
}

esp_err_t app_net_reset_provisioning(void)
{
#if CONFIG_APP_NET_USE_STATIC_CREDENTIALS
    return ESP_ERR_NOT_SUPPORTED;
#else
    if (!s_initialised) {
        return ESP_ERR_INVALID_STATE;
    }
    ESP_LOGW(TAG, "erasing stored credentials");
    return APP_PROV_MGR_RESET();
#endif
}
