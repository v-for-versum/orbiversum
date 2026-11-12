# ---------------------------------------------------------------------------
# The provisioning component is not just renamed between IDF generations — it
# changes *source*. On IDF 5.x it ships inside the IDF tree as
# `wifi_provisioning`; from 6.0 it lives in the component registry as
# espressif/network_provisioning and is reached under the name
# `network_provisioning`.
#
# Requiring the wrong one fails component resolution before a single file is
# compiled:
#
#   Failed to resolve component 'wifi_provisioning' required by component
#   'app_net': unknown name.
#
# src/app_prov.h then hides the API difference behind one set of names.
# ---------------------------------------------------------------------------
if(IDF_VERSION_MAJOR GREATER_EQUAL 6)
    set(APP_PROV_COMPONENT network_provisioning)
else()
    set(APP_PROV_COMPONENT wifi_provisioning)
endif()

idf_component_register(
    SRCS
        "src/app_net.c"
        "src/app_ota.c"
    INCLUDE_DIRS
        "include"
    REQUIRES
        esp_wifi
        esp_event
        esp_netif
        nvs_flash
    PRIV_REQUIRES
        ${APP_PROV_COMPONENT}
        esp_http_client
        esp_https_ota
        app_update
        esp_timer
        # esp_crt_bundle.h, used by app_ota.c to verify HTTPS servers against
        # the bundled root store. Not pulled in implicitly.
        mbedtls
)
