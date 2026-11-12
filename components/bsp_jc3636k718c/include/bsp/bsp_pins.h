/*
 * Pin map for the Guition JC3636K718C — 1.85" round knob screen.
 *
 * Sources: the vendor Arduino demo's pincfg.h
 * (2-Program Example/Demo_arduino/ST77916_LVGL_DEMO/pincfg.h) cross-checked
 * against the board schematic (6-Schematic diagram/JC3636K718.pdf) and against
 * two independent working firmwares for this exact board. Where the vendor
 * demo and the schematic disagree, the schematic wins and the disagreement is
 * called out below.
 *
 * Nothing outside this header should contain a raw GPIO number.
 *
 * LCD  : ST77916, 360x360 IPS, QSPI
 * Touch: CST816S capacitive, I2C
 * Ring : WS2812-compatible addressable LEDs
 * Knob : mechanical rotary encoder
 * Audio: PCM5100A I2S DAC (out) + PDM MEMS microphone (in)
 * SD   : SDMMC 4-bit
 */
#pragma once

#include "driver/gpio.h"

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------- */
/* Display — ST77916 over QSPI                                                */
/* -------------------------------------------------------------------------- */
#define BSP_LCD_H_RES               360
#define BSP_LCD_V_RES               360

#define BSP_LCD_CS                  GPIO_NUM_12
#define BSP_LCD_PCLK                GPIO_NUM_11
#define BSP_LCD_DATA0               GPIO_NUM_13
#define BSP_LCD_DATA1               GPIO_NUM_14
#define BSP_LCD_DATA2               GPIO_NUM_15
#define BSP_LCD_DATA3               GPIO_NUM_16
#define BSP_LCD_RST                 GPIO_NUM_17

/* Backlight: single white LED anode driven by LEDC PWM. */
#define BSP_LCD_BL                  GPIO_NUM_21
#define BSP_LCD_BL_ACTIVE_LEVEL     1

/* Panel tear-effect output. Unused by this BSP; listed so it is not
 * accidentally claimed by application code. */
#define BSP_LCD_TE                  GPIO_NUM_18

/* -------------------------------------------------------------------------- */
/* I2C bus — the touch controller                                             */
/* -------------------------------------------------------------------------- */
#define BSP_I2C_PORT                0
#define BSP_I2C_SDA                 GPIO_NUM_9
#define BSP_I2C_SCL                 GPIO_NUM_10

/* CST816S capacitive touch */
#define BSP_TOUCH_I2C_ADDR          0x15
#define BSP_TOUCH_INT               GPIO_NUM_7
#define BSP_TOUCH_RST               GPIO_NUM_8


/* -------------------------------------------------------------------------- */
/* Rotary encoder                                                             */
/* -------------------------------------------------------------------------- */
#define BSP_ENC_A                   GPIO_NUM_2
#define BSP_ENC_B                   GPIO_NUM_1

/*
 * NO PUSH BUTTON.
 *
 * The vendor demo's pincfg.h defines `BTN_PIN 0`, but that macro is dead code
 * — it is never referenced anywhere in the vendor sources — and GPIO0 is in
 * fact the data line for the LED ring (confirmed by the schematic and by two
 * independent firmwares for this board).
 *
 * Sweep the board and no GPIO is left over for a switch either. The knob is a
 * pure rotary encoder, so the UI must be usable with rotation alone. This is
 * why CONFIG_BSP_ENCODER_USE_BUTTON defaults to off.
 */
#define BSP_ENC_BTN                 GPIO_NUM_NC

/* -------------------------------------------------------------------------- */
/* Addressable LED ring                                                       */
/* -------------------------------------------------------------------------- */
#define BSP_LED_DATA                GPIO_NUM_0
#define BSP_LED_COUNT               13

/* -------------------------------------------------------------------------- */
/* Audio — PCM5100A DAC out, PDM MEMS microphone in                           */
/* -------------------------------------------------------------------------- */
#define BSP_I2S_MCLK                GPIO_NUM_NC   /* PCM5100A runs without MCLK */
#define BSP_I2S_BCLK                GPIO_NUM_3
#define BSP_I2S_LRCK                GPIO_NUM_45
#define BSP_I2S_DOUT                GPIO_NUM_42
/* XSMT shutdown line of the PCM5100A; low = muted. */
#define BSP_I2S_MUTE                GPIO_NUM_46

#define BSP_MIC_SCK                 GPIO_NUM_5
#define BSP_MIC_SD                  GPIO_NUM_4

/* -------------------------------------------------------------------------- */
/* microSD — SDMMC 4-bit                                                      */
/* -------------------------------------------------------------------------- */
#define BSP_SD_CLK                  GPIO_NUM_39
#define BSP_SD_CMD                  GPIO_NUM_38
#define BSP_SD_D0                   GPIO_NUM_40
#define BSP_SD_D1                   GPIO_NUM_41
#define BSP_SD_D2                   GPIO_NUM_48
#define BSP_SD_D3                   GPIO_NUM_47
#define BSP_SD_MOUNT_POINT          "/sdcard"

/* -------------------------------------------------------------------------- */
/* Misc                                                                       */
/* -------------------------------------------------------------------------- */
/* Battery sense input, if a cell is fitted. */
#define BSP_BATTERY_ADC             GPIO_NUM_6

/*
 * Cross-checked against Guition's own board definition (component guition-jc/
 * guition_boards, board jc3636k718): every pin in this file except the battery matches
 * it exactly — QSPI on 11..17, the backlight on 21, touch on 9/10, the SD card on
 * 38..48 — so the map is right.
 *
 * The battery is the one thing that definition does NOT describe. It declares a
 * display, a backlight, a touch panel and an SD card, and nothing else: no ADC
 * channel, no charger, no charge-status line. That is why nothing here can be checked
 * against a vendor number, why a "charging" indicator in an older Guition demo can
 * only have been inferred from the voltage rather than read from a pin, and why the
 * divider had to be settled from the charger's own 4.20 V instead — see bsp_battery.h.
 */


#ifdef __cplusplus
}
#endif
