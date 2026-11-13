/*
 * Audio: PCM5100A I2S output and PDM microphone input.
 *
 * The two directions live on separate I2S controllers, and which one gets
 * which is not arbitrary: the ESP32-S3's PDM-to-PCM converter only exists on
 * I2S0. Putting PDM RX on I2S1 fails at init with
 *
 *   i2s_pdm_rx_set_slot(486): PDM2PCM converter is not supported on selected port
 *
 * so the microphone takes I2S_NUM_0 and the PCM5100A, which is plain standard
 * mode and works on either port, takes I2S_NUM_1.
 */

#include "bsp/bsp_audio.h"

#include <inttypes.h>
#include <string.h>

#include "esp_log.h"

#include "driver/gpio.h"
#include "driver/i2s_pdm.h"
#include "driver/i2s_std.h"

#include "bsp/bsp_pins.h"

static const char *TAG = "bsp.audio";

/* -------------------------------------------------------------------------- */
/* Output                                                                     */
/* -------------------------------------------------------------------------- */

static i2s_chan_handle_t s_tx;
static bool              s_tx_muted = true;

static i2s_data_bit_width_t to_bit_width(uint8_t bits)
{
    switch (bits) {
    case 16: return I2S_DATA_BIT_WIDTH_16BIT;
    case 24: return I2S_DATA_BIT_WIDTH_24BIT;
    case 32: return I2S_DATA_BIT_WIDTH_32BIT;
    default: return I2S_DATA_BIT_WIDTH_16BIT;
    }
}

static esp_err_t mute_line_init(void)
{
    gpio_config_t cfg = {
        .pin_bit_mask = 1ULL << BSP_I2S_MUTE,
        .mode         = GPIO_MODE_OUTPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
    };
    esp_err_t err = gpio_config(&cfg);
    if (err != ESP_OK) {
        return err;
    }
    /* XSMT is active low: hold the DAC muted while the channel comes up so the
     * power-on transient never reaches the speaker. */
    gpio_set_level(BSP_I2S_MUTE, 0);
    s_tx_muted = true;
    return ESP_OK;
}

esp_err_t bsp_audio_out_init(const bsp_audio_out_config_t *cfg)
{
    if (s_tx) {
        ESP_LOGW(TAG, "output already initialised");
        return ESP_OK;
    }

    bsp_audio_out_config_t defaults = BSP_AUDIO_OUT_CONFIG_DEFAULT();
    if (!cfg) {
        cfg = &defaults;
    }

    ESP_ERROR_CHECK(mute_line_init());

    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_1, I2S_ROLE_MASTER);
    /*
     * The Rust audio task (pinned to core 1) feeds this channel; a deeper queue absorbs scheduling
     * jitter and flash stalls without audible gaps.
     *
     * Twelve descriptors and not six, and the reason is measured rather than
     * assumed. Writing a setting to NVS erases a flash sector, and the cache is
     * off for both cores while that happens: the audio task's own cost counter
     * recorded a single block of 45.8 ms during a settings commit, with no other
     * logging in that second at all. At 240 frames per descriptor and 48 kHz a
     * descriptor is 5 ms, so six of them are 30 ms of slack — less than the
     * stall, and the short crackle heard once or twice while the settings drawer
     * is in use is that stall reaching the DAC. Twelve is 60 ms, which covers
     * the worst stall measured.
     *
     * The price is latency: 60 ms of queue in front of the DAC instead of 30.
     * That is the right trade here. The queue is hardware and keeps draining
     * through a stall that has switched the cache off, and this instrument is a
     * drone machine driven by one knob rather than a keyboard, so a fixed delay
     * in the output is a delay nobody is playing against.
     */
    chan_cfg.dma_desc_num  = 12;
    chan_cfg.dma_frame_num = 240;

    esp_err_t err = i2s_new_channel(&chan_cfg, &s_tx, NULL);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "i2s_new_channel failed: %s", esp_err_to_name(err));
        return err;
    }

    i2s_std_config_t std_cfg = {
        .clk_cfg = I2S_STD_CLK_DEFAULT_CONFIG(cfg->sample_rate),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(
            to_bit_width(cfg->bits),
            cfg->stereo ? I2S_SLOT_MODE_STEREO : I2S_SLOT_MODE_MONO),
        .gpio_cfg = {
            /* The PCM5100A derives its own clock from BCK/LRCK; MCLK is not
             * routed on this board. */
            .mclk = I2S_GPIO_UNUSED,
            .bclk = BSP_I2S_BCLK,
            .ws   = BSP_I2S_LRCK,
            .dout = BSP_I2S_DOUT,
            .din  = I2S_GPIO_UNUSED,
            .invert_flags = {
                .mclk_inv = false,
                .bclk_inv = false,
                .ws_inv   = false,
            },
        },
    };

    err = i2s_channel_init_std_mode(s_tx, &std_cfg);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_init_std_mode failed: %s", esp_err_to_name(err));
        i2s_del_channel(s_tx);
        s_tx = NULL;
        return err;
    }

    err = i2s_channel_enable(s_tx);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_enable failed: %s", esp_err_to_name(err));
        i2s_del_channel(s_tx);
        s_tx = NULL;
        return err;
    }

    ESP_LOGI(TAG, "output ready: %" PRIu32 " Hz, %u-bit, %s",
             cfg->sample_rate, cfg->bits, cfg->stereo ? "stereo" : "mono");

    /* Unmute only now that a valid clock is present. */
    return bsp_audio_out_mute(false);
}

bool bsp_audio_out_is_ready(void)
{
    return s_tx != NULL;
}

esp_err_t bsp_audio_out_write(const void *data, size_t bytes,
                              size_t *written, uint32_t timeout_ms)
{
    if (!s_tx) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!data || bytes == 0) {
        return ESP_ERR_INVALID_ARG;
    }
    return i2s_channel_write(s_tx, data, bytes, written, timeout_ms);
}

esp_err_t bsp_audio_out_set_sample_rate(uint32_t sample_rate)
{
    if (!s_tx) {
        return ESP_ERR_INVALID_STATE;
    }

    i2s_std_clk_config_t clk_cfg = I2S_STD_CLK_DEFAULT_CONFIG(sample_rate);
    esp_err_t err = i2s_channel_reconfig_std_clock(s_tx, &clk_cfg);
    if (err == ESP_OK) {
        ESP_LOGI(TAG, "sample rate -> %" PRIu32 " Hz", sample_rate);
    } else {
        ESP_LOGE(TAG, "could not change sample rate: %s", esp_err_to_name(err));
    }
    return err;
}

esp_err_t bsp_audio_out_mute(bool mute)
{
    gpio_set_level(BSP_I2S_MUTE, mute ? 0 : 1);
    s_tx_muted = mute;
    return ESP_OK;
}

bool bsp_audio_out_is_muted(void)
{
    return s_tx_muted;
}

esp_err_t bsp_audio_out_deinit(void)
{
    if (!s_tx) {
        return ESP_OK;
    }

    bsp_audio_out_mute(true);
    i2s_channel_disable(s_tx);
    i2s_del_channel(s_tx);
    s_tx = NULL;
    return ESP_OK;
}

/* -------------------------------------------------------------------------- */
/* Input                                                                      */
/* -------------------------------------------------------------------------- */

static i2s_chan_handle_t s_rx;

esp_err_t bsp_audio_in_init(const bsp_audio_in_config_t *cfg)
{
    if (s_rx) {
        ESP_LOGW(TAG, "input already initialised");
        return ESP_OK;
    }

    bsp_audio_in_config_t defaults = BSP_AUDIO_IN_CONFIG_DEFAULT();
    if (!cfg) {
        cfg = &defaults;
    }

    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_0, I2S_ROLE_MASTER);
    chan_cfg.dma_desc_num  = 8;
    chan_cfg.dma_frame_num = 240;

    esp_err_t err = i2s_new_channel(&chan_cfg, NULL, &s_rx);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "i2s_new_channel (rx) failed: %s", esp_err_to_name(err));
        return err;
    }

#if CONFIG_BSP_MIC_INTERFACE_PDM
    /*
     * PDM RX supports 16-bit samples only — "Data bit width only support 16
     * bits" in i2s_pdm.h. Anything else is refused rather than quietly
     * producing a stream the driver cannot configure.
     */
    if (cfg->bits != 16) {
        ESP_LOGW(TAG, "PDM requires 16-bit samples; using 16 instead of %u", cfg->bits);
    }

    const i2s_pdm_rx_config_t pdm_cfg = {
        .clk_cfg = I2S_PDM_RX_CLK_DEFAULT_CONFIG(cfg->sample_rate),
        /* MONO already selects the left slot, which is exactly right for a
         * single microphone: taking both slots would put silence in every
         * second frame. */
        .slot_cfg = I2S_PDM_RX_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_16BIT,
                                                   I2S_SLOT_MODE_MONO),
        .gpio_cfg = {
            .clk = BSP_MIC_SCK,
            .din = BSP_MIC_SD,
            .invert_flags = {
                .clk_inv = false,
            },
        },
    };

    err = i2s_channel_init_pdm_rx_mode(s_rx, &pdm_cfg);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_init_pdm_rx_mode failed: %s", esp_err_to_name(err));
        i2s_del_channel(s_rx);
        s_rx = NULL;
        return err;
    }
#else
    i2s_std_config_t std_cfg = {
        .clk_cfg  = I2S_STD_CLK_DEFAULT_CONFIG(cfg->sample_rate),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(
            to_bit_width(cfg->bits), I2S_SLOT_MODE_MONO),
        .gpio_cfg = {
            .mclk = I2S_GPIO_UNUSED,
            .bclk = BSP_MIC_SCK,
            .ws   = (gpio_num_t)CONFIG_BSP_MIC_WS_GPIO,
            .dout = I2S_GPIO_UNUSED,
            .din  = BSP_MIC_SD,
            .invert_flags = {
                .mclk_inv = false,
                .bclk_inv = false,
                .ws_inv   = false,
            },
        },
    };
    err = i2s_channel_init_std_mode(s_rx, &std_cfg);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_init_std_mode (rx) failed: %s", esp_err_to_name(err));
        i2s_del_channel(s_rx);
        s_rx = NULL;
        return err;
    }
#endif

    err = i2s_channel_enable(s_rx);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_enable (rx) failed: %s", esp_err_to_name(err));
        i2s_del_channel(s_rx);
        s_rx = NULL;
        return err;
    }

    if (cfg->gain_db > 0) {
        bsp_audio_in_set_gain(cfg->gain_db);
    }

    ESP_LOGI(TAG, "input ready: %" PRIu32 " Hz, %u-bit, %s",
             cfg->sample_rate, cfg->bits,
#if CONFIG_BSP_MIC_INTERFACE_PDM
             "PDM"
#else
             "I2S"
#endif
    );
    return ESP_OK;
}

bool bsp_audio_in_is_ready(void)
{
    return s_rx != NULL;
}

esp_err_t bsp_audio_in_read(void *data, size_t bytes,
                            size_t *read, uint32_t timeout_ms)
{
    if (!s_rx) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!data || bytes == 0) {
        return ESP_ERR_INVALID_ARG;
    }
    return i2s_channel_read(s_rx, data, bytes, read, timeout_ms);
}

esp_err_t bsp_audio_in_set_gain(uint8_t gain_db)
{
    if (!s_rx) {
        return ESP_ERR_INVALID_STATE;
    }

#if CONFIG_BSP_MIC_INTERFACE_PDM
    /*
     * There is no "set gain" call on the PDM RX path in ESP-IDF. The only
     * control is `amplify_num` in the slot configuration: a digital
     * multiplier of 1..15 applied after the PDM-to-PCM conversion, worth
     * roughly 3 dB per step.
     *
     * It multiplies the digital result, so it lifts the noise floor along
     * with the signal. Drive the microphone as hard as you can before
     * reaching for this.
     *
     * The field only exists on chips with the PDM RX high-pass/amplify stage,
     * hence the guard — referencing it unconditionally would fail to compile
     * on the others.
     */
#if defined(SOC_I2S_SUPPORTS_PDM_RX_HP_FILTER) && SOC_I2S_SUPPORTS_PDM_RX_HP_FILTER
    uint32_t amplify = 1u + (uint32_t)(gain_db / 3u);
    if (amplify > 15u) {
        amplify = 15u;
    }

    i2s_pdm_rx_slot_config_t slot_cfg =
        I2S_PDM_RX_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_16BIT, I2S_SLOT_MODE_MONO);
    slot_cfg.amplify_num = amplify;

    /* A slot configuration can only be changed while the channel is stopped. */
    i2s_channel_disable(s_rx);
    esp_err_t err = i2s_channel_reconfig_pdm_rx_slot(s_rx, &slot_cfg);
    i2s_channel_enable(s_rx);

    if (err != ESP_OK) {
        ESP_LOGE(TAG, "could not set input gain: %s", esp_err_to_name(err));
        return err;
    }

    /* Debug, not info, and deliberately so: the only caller is the microphone's
     * sensitivity applier in the audio task, next to the block render — and a log line is
     * a blocking UART write, which is the one thing that path must not do. The caller
     * already avoids the call unless the multiplier itself moves, so at INFO this would be
     * fourteen lines across the knob's whole travel, each with the block deadline missed
     * behind it. The value is still worth having when someone is looking for it. */
    ESP_LOGD(TAG, "input gain %u dB -> amplify_num %" PRIu32, gain_db, amplify);
    return ESP_OK;
#else
    ESP_LOGW(TAG,
             "this chip has no PDM RX amplify stage, so %u dB of gain cannot be "
             "applied in hardware",
             gain_db);
    return ESP_ERR_NOT_SUPPORTED;
#endif /* SOC_I2S_SUPPORTS_PDM_RX_HP_FILTER */

#else
    ESP_LOGW(TAG, "hardware gain is only available on the PDM input path");
    return ESP_ERR_NOT_SUPPORTED;
#endif /* CONFIG_BSP_MIC_INTERFACE_PDM */
}

esp_err_t bsp_audio_in_deinit(void)
{
    if (!s_rx) {
        return ESP_OK;
    }

    i2s_channel_disable(s_rx);
    i2s_del_channel(s_rx);
    s_rx = NULL;
    return ESP_OK;
}
