#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "esp_err.h"
#include "bsp/bsp.h"

void app_main(void)
{
    bsp_features_t features = BSP_FEATURES_ALL();
    features.audio_in = false; /* PDM microphone is off by default in this base */
    ESP_ERROR_CHECK(bsp_init(&features));
    bsp_print_info();

    for (;;) {
        vTaskDelay(portMAX_DELAY);
    }
}
