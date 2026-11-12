# Board support package for the Guition JC3636K718C (1.85" round knob screen).
#
# One component rather than several so that the pin map, the Kconfig options
# and the bring-up order live in a single place. The application only ever
# includes <bsp/bsp.h>.

idf_component_register(
    SRCS
        "src/bsp.c"
        "src/bsp_i2c.c"
        "src/bsp_display.c"
        "src/bsp_touch.c"
        "src/bsp_encoder.c"
        "src/bsp_encoder_probe.c"
        "src/bsp_audio.c"
        "src/bsp_sdcard.c"
        "src/bsp_led.c"
        "src/bsp_battery.c"
        "src/st77916_init_cmds.c"
    INCLUDE_DIRS
        "include"
    PRIV_INCLUDE_DIRS
        "src"
    REQUIRES
        # Public: bsp.h exposes these types, so dependents need the headers.
        esp_lcd
        esp_lcd_touch
        esp_lvgl_port
        button
        driver
    PRIV_REQUIRES
        esp_lcd_st77916
        esp_lcd_touch_cst816s
        led_strip
        esp_timer
        # bsp_print_info() reports the firmware version, which needs
        # esp_app_desc.h. IDF does not infer that dependency, so it has to be
        # named here or bsp.c fails to compile.
        esp_app_format
        # esp_flash_get_size(), same story.
        spi_flash
        esp_driver_i2c
        esp_driver_i2s
        esp_driver_gpio
        # The battery sense is a plain ADC pin read through the oneshot driver.
        esp_adc
        esp_driver_ledc
        esp_driver_spi
        esp_driver_sdmmc
        fatfs
        sdmmc
)
