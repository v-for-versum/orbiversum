/*
 * Internal: the board's single I2C bus.
 *
 * The CST816S touch controller sits on these two wires (SDA 9, SCL 10). The bus
 * is therefore owned by the BSP as a whole, not by the driver: whoever comes up
 * first creates it, and it survives the teardown of individual devices.
 *
 * Per-device clock rates are set when each device handle is added, so a second
 * device on the same wires could run at its own rate.
 *
 * Not a public header — applications never need to touch the bus directly.
 */
#pragma once

#include "esp_err.h"
#include "driver/i2c_master.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Create the bus if it does not exist yet. Idempotent. */
esp_err_t bsp_i2c_init(void);

/* The bus handle, or NULL when bsp_i2c_init() has not succeeded. */
i2c_master_bus_handle_t bsp_i2c_get_bus(void);

/* Release the bus. Only meaningful once every device has been removed. */
esp_err_t bsp_i2c_deinit(void);

#ifdef __cplusplus
}
#endif
