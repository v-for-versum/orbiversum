/*
 * The board's shared I2C bus — see bsp_i2c.h.
 */

#include "bsp_i2c.h"

#include "esp_check.h"
#include "esp_log.h"

#include "bsp/bsp_pins.h"

static const char *TAG = "bsp.i2c";

static i2c_master_bus_handle_t s_bus;

esp_err_t bsp_i2c_init(void)
{
    if (s_bus) {
        return ESP_OK;
    }

    const i2c_master_bus_config_t bus_cfg = {
        .i2c_port          = BSP_I2C_PORT,
        .sda_io_num        = BSP_I2C_SDA,
        .scl_io_num        = BSP_I2C_SCL,
        .clk_source        = I2C_CLK_SRC_DEFAULT,
        .glitch_ignore_cnt = 7,
        /* The board carries external pull-ups (R56/R57). Enabling the
         * internal ones as well costs nothing at these speeds and keeps the
         * bus alive if a revision omits them. */
        .flags = { .enable_internal_pullup = true },
    };

    ESP_RETURN_ON_ERROR(i2c_new_master_bus(&bus_cfg, &s_bus), TAG, "i2c_new_master_bus");

    ESP_LOGI(TAG, "bus up on SDA %d / SCL %d", BSP_I2C_SDA, BSP_I2C_SCL);
    return ESP_OK;
}

i2c_master_bus_handle_t bsp_i2c_get_bus(void)
{
    return s_bus;
}

esp_err_t bsp_i2c_deinit(void)
{
    if (!s_bus) {
        return ESP_OK;
    }
    esp_err_t err = i2c_del_master_bus(s_bus);
    s_bus = NULL;
    return err;
}
