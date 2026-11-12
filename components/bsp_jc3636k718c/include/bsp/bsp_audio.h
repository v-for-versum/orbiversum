/*
 * Audio: PCM5100A DAC output and an I2S MEMS microphone.
 *
 * Both directions are ES8311-free — there is no codec to configure over I2C.
 * The PCM5100A is a plain I2S DAC, so the stream can be written directly once
 * the channel is up.
 *
 * MICROPHONE ASSUMPTION
 *   The schematic routes only two signals to the microphone (SCK on GPIO5,
 *   SD on GPIO4). A conventional I2S MEMS mic would also need a word-select
 *   line, so the part is almost certainly a PDM microphone and the driver uses
 *   the ESP32-S3 PDM RX path. If your board revision turns out to carry an
 *   I2S mic, set CONFIG_BSP_MIC_INTERFACE_I2S and supply the extra pins in
 *   menuconfig.
 *
 * OUT-OF-SCOPE
 *   Sample-rate conversion, mixing and decoding (MP3/AAC/WAV) are application
 *   concerns. This layer moves PCM frames and nothing else.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------- */
/* Output — PCM5100A                                                          */
/* -------------------------------------------------------------------------- */
typedef struct {
    uint32_t sample_rate;  /*!< e.g. 44100, 48000 */
    uint8_t  bits;         /*!< 16, 24 or 32 */
    bool     stereo;       /*!< false = mono, duplicated across both slots */
} bsp_audio_out_config_t;

/*
 * 48 kHz, not 44.1.
 *
 * This board's firmware is a 48 kHz instrument end to end, and the rate is not
 * a preference: the synthesis engine is designed and tested at 48 kHz and the
 * filter patches it ships hardcode coefficient tables for that rate. Feeding it
 * 44.1 kHz would not just resample, it would detune the filters — and the
 * engine's own setSampleRate() changes only its DSP rate, never the I2S clock,
 * so a mismatch there is silent and permanent.
 */
#define BSP_AUDIO_OUT_CONFIG_DEFAULT() \
    { .sample_rate = 48000, .bits = 16, .stereo = true }

/*
 * Bring up I2S TX and unmute the DAC.
 *
 * The mute line is held muted until the channel is enabled, which keeps the
 * power-on thump out of the speaker.
 */
esp_err_t bsp_audio_out_init(const bsp_audio_out_config_t *cfg);

/* True once I2S TX is running. */
bool bsp_audio_out_is_ready(void);

/*
 * Write PCM frames. Blocks until everything is queued or the timeout expires;
 * pass portMAX_DELAY to always wait. `written` may be NULL.
 */
esp_err_t bsp_audio_out_write(const void *data, size_t bytes,
                              size_t *written, uint32_t timeout_ms);

/* Reconfigure the sample rate without a full teardown. */
esp_err_t bsp_audio_out_set_sample_rate(uint32_t sample_rate);

/* Mute (true) or unmute (false) via the PCM5100A XSMT line. */
esp_err_t bsp_audio_out_mute(bool mute);

/* Current mute state. */
bool bsp_audio_out_is_muted(void);

/* Silence any queued data and stop the channel. */
esp_err_t bsp_audio_out_deinit(void);

/* -------------------------------------------------------------------------- */
/* Input — MEMS microphone                                                    */
/* -------------------------------------------------------------------------- */
typedef struct {
    uint32_t sample_rate;  /*!< PDM mics are typically run at 16 kHz */
    uint8_t  bits;         /*!< 16 for PDM (the only value it supports) */
    uint8_t  gain_db;      /*!< 0..42; see bsp_audio_in_set_gain() */
} bsp_audio_in_config_t;

#define BSP_AUDIO_IN_CONFIG_DEFAULT() \
    { .sample_rate = 16000, .bits = 16, .gain_db = 0 }

esp_err_t bsp_audio_in_init(const bsp_audio_in_config_t *cfg);

bool bsp_audio_in_is_ready(void);

/* Read PCM frames. `read` may be NULL. */
esp_err_t bsp_audio_in_read(void *data, size_t bytes,
                            size_t *read, uint32_t timeout_ms);

/*
 * Raise the input level, 0..42 dB. Returns ESP_ERR_NOT_SUPPORTED when the
 * hardware cannot do it — which is a real possibility, not a formality.
 *
 * On the PDM path (the default) there is no analogue gain to set. ESP-IDF
 * exposes only `amplify_num`, a digital multiplier of the converted samples,
 * so this maps dB onto that in roughly 3 dB steps and caps out around 23 dB.
 * It multiplies signal and noise floor alike: if the microphone sounds thin,
 * fix the acoustics or the sample rate before reaching for this.
 *
 * Passing 0 is valid and restores unity gain.
 */
esp_err_t bsp_audio_in_set_gain(uint8_t gain_db);

esp_err_t bsp_audio_in_deinit(void);

#ifdef __cplusplus
}
#endif
