#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "esp_err.h"
#include "bsp/bsp.h"

void app_main(void)
{
    const bsp_features_t features = BSP_FEATURES_ALL();
    ESP_ERROR_CHECK(bsp_init(&features));
    bsp_print_info();

    for (;;) {
        vTaskDelay(portMAX_DELAY);
    }
}
