/*
 * CST816S touch bring-up.
 *
 * The controller owns the board's I2C bus,
 * so the bus itself is owned by the BSP (bsp_i2c.c) and this file only ever
 * adds a device to it.
 *
 * Two I2C generations exist in ESP-IDF. The newer i2c_master driver is used
 * here: it is the supported path from IDF 5.2 onwards, and the LCD panel-IO
 * layer accepts its bus handle directly.
 */

#include "bsp/bsp_touch.h"

#include "esp_check.h"
#include "esp_idf_version.h"
#include "esp_log.h"

#include "driver/i2c_master.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_touch_cst816s.h"
#include "esp_lvgl_port.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "bsp/bsp_display.h"
#include "bsp/bsp_pins.h"
#include "bsp_i2c.h"

static const char *TAG = "bsp.touch";

/* The controller needs a moment after power-up before it answers. */
#define BSP_TOUCH_PROBE_TRIES    3
#define BSP_TOUCH_PROBE_DELAY_MS 20

static esp_lcd_panel_io_handle_t s_touch_io;
static esp_lcd_touch_handle_t    s_touch;
static lv_indev_t               *s_indev;

/*
 * A TAP IS SHORTER THAN A FRAME, so sampling cannot live in the drawing loop.
 *
 * The port registers this panel in event mode: the controller's interrupt wakes the LVGL
 * task, and that task reads the panel when it next gets to it. The task is inside a frame
 * for thirty to ninety milliseconds at a time — a ring with five fills and six blocks, or a
 * page turn with two snapshots — and the reads in between are coalesced into one. A finger
 * that lands and lifts inside that window leaves no trace at all, and the two taps of a
 * double tap become one: measured on this board, taps are 24 to 50 ms long while the loop
 * that was meant to see them is busy for 40 ms and more. That is why the gesture answered on
 * the sparse pages and not on the mixer, where the frames are the heaviest.
 *
 * So the panel is read by a task of its own, every few milliseconds, whatever the display is
 * doing, and each change of the finger's state is queued. The input device then runs on its
 * own timer and takes the changes one at a time, so nothing can be coalesced away: two taps
 * are two presses and two releases even if both happened while a page was being drawn. A
 * finger that stays down keeps updating the live position, which is what a swipe needs.
 */
#define BSP_TOUCH_SAMPLE_MS     4
#define BSP_TOUCH_QUEUE_LEN     32
#define BSP_TOUCH_READ_TIMER_MS 10

typedef struct {
    bool     pressed;
    uint16_t x;
    uint16_t y;
} bsp_touch_ev_t;

static bsp_touch_ev_t    s_ev[BSP_TOUCH_QUEUE_LEN];
static volatile uint32_t s_ev_head;              /* where the sampler writes */
static volatile uint32_t s_ev_tail;              /* where the input device reads */
static volatile bool     s_live_pressed;

/* One per finger that goes down. The UI needs it to tell the click that ends a swipe from
 * the tap of the next finger: a count, not a clock, cannot be overtaken by a slow frame. */
static volatile uint32_t s_contact;
static volatile uint16_t s_live_x;
static volatile uint16_t s_live_y;

static void touch_ev_push(bool pressed, uint16_t x, uint16_t y)
{
    const uint32_t head = s_ev_head;

    if ((head - s_ev_tail) >= BSP_TOUCH_QUEUE_LEN) {
        /* Full: drop the oldest. Reaching this means the UI stopped reading for a quarter of
         * a second, and the newest state is the only one still worth having. */
        s_ev_tail = head - (BSP_TOUCH_QUEUE_LEN - 1);
    }

    s_ev[head % BSP_TOUCH_QUEUE_LEN] = (bsp_touch_ev_t){pressed, x, y};
    s_ev_head = head + 1;
}

static void touch_sampler_task(void *arg)
{
    (void)arg;

    bool last = false;

    for (;;) {
        esp_lcd_touch_point_data_t point[1];
        uint8_t count = 1;

        const bool down = (esp_lcd_touch_read_data(s_touch) == ESP_OK) &&
                          (esp_lcd_touch_get_data(s_touch, point, &count, 1) == ESP_OK) &&
                          (count > 0);

        if (down) {
            s_live_x = point[0].x;
            s_live_y = point[0].y;
            s_live_pressed = true;

            if (!last) {
                s_contact++;
                touch_ev_push(true, point[0].x, point[0].y);
                last = true;
            }
        } else {
            s_live_pressed = false;

            if (last) {
                /* The release is queued where the finger was, not where nothing is: a
                 * release with no point of its own reads as a jump to the corner. */
                touch_ev_push(false, s_live_x, s_live_y);
                last = false;
            }
        }

        vTaskDelay(pdMS_TO_TICKS(BSP_TOUCH_SAMPLE_MS));
    }
}

static void touch_read_cb(lv_indev_t *indev, lv_indev_data_t *data)
{
    (void)indev;

    bsp_touch_ev_t ev;

    if (s_ev_tail != s_ev_head) {
        ev = s_ev[s_ev_tail % BSP_TOUCH_QUEUE_LEN];
        s_ev_tail++;
    } else {
        ev.pressed = s_live_pressed;
        ev.x = s_live_x;
        ev.y = s_live_y;
    }

    if (ev.pressed) {
        data->point.x = ev.x;
        data->point.y = ev.y;
        data->state = LV_INDEV_STATE_PRESSED;
    } else {
        data->state = LV_INDEV_STATE_RELEASED;
    }
}

esp_err_t bsp_touch_init(void)
{
    if (s_touch) {
        ESP_LOGW(TAG, "touch already initialised");
        return ESP_OK;
    }

    lv_display_t *disp = bsp_display_get_lvgl_display();
    if (!disp) {
        ESP_LOGE(TAG, "call bsp_display_init() before bsp_touch_init()");
        return ESP_ERR_INVALID_STATE;
    }

    /* --- Shared I2C bus --------------------------------------------------- */
    ESP_RETURN_ON_ERROR(bsp_i2c_init(), TAG, "bsp_i2c_init");
    i2c_master_bus_handle_t bus = bsp_i2c_get_bus();
    if (!bus) {
        return ESP_ERR_INVALID_STATE;
    }

    /*
     * Probe before handing the bus to the driver. Without this, a missing or
     * unpowered controller produces a wall of panel-IO errors instead of one
     * clear message.
     */
    bool found = false;
    for (int attempt = 0; attempt < BSP_TOUCH_PROBE_TRIES && !found; attempt++) {
        if (i2c_master_probe(bus, BSP_TOUCH_I2C_ADDR, 100) == ESP_OK) {
            found = true;
            break;
        }
        vTaskDelay(pdMS_TO_TICKS(BSP_TOUCH_PROBE_DELAY_MS));
    }
    if (!found) {
        ESP_LOGE(TAG, "no CST816S at address 0x%02X — check the flex connector",
                 BSP_TOUCH_I2C_ADDR);
        return ESP_ERR_NOT_FOUND;
    }

    /* --- Panel IO over I2C ------------------------------------------------ */
    esp_lcd_panel_io_i2c_config_t io_cfg = ESP_LCD_TOUCH_IO_I2C_CST816S_CONFIG();
    io_cfg.scl_speed_hz = (uint32_t)CONFIG_BSP_TOUCH_I2C_FREQ_KHZ * 1000u;

    /*
     * The bus handle type changed in IDF 6.0: esp_lcd_new_panel_io_i2c() now
     * takes an i2c_master_bus_handle_t directly, where 5.x wanted an opaque
     * esp_lcd_i2c_bus_handle_t that had to be cast — and that type no longer
     * exists, so the 5.x line would not even compile.
     */
#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(6, 0, 0)
    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_io_i2c(bus, &io_cfg, &s_touch_io),
                        TAG, "esp_lcd_new_panel_io_i2c");
#else
    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_io_i2c(
                            (esp_lcd_i2c_bus_handle_t)bus, &io_cfg, &s_touch_io),
                        TAG, "esp_lcd_new_panel_io_i2c");
#endif

    /* --- Touch controller ------------------------------------------------- */
    const esp_lcd_touch_config_t touch_cfg = {
        .x_max        = BSP_LCD_H_RES,
        .y_max        = BSP_LCD_V_RES,
        .rst_gpio_num = BSP_TOUCH_RST,
        .int_gpio_num = BSP_TOUCH_INT,
        .levels = {
            /* Both lines are active low on the CST816S: reset is pulsed low,
             * and the interrupt fires on a falling edge. */
            .reset     = 0,
            .interrupt = 0,
        },
        .flags = {
            .swap_xy  = CONFIG_BSP_TOUCH_SWAP_XY,
            .mirror_x = CONFIG_BSP_TOUCH_MIRROR_X,
            .mirror_y = CONFIG_BSP_TOUCH_MIRROR_Y,
        },
    };

    ESP_RETURN_ON_ERROR(esp_lcd_touch_new_i2c_cst816s(s_touch_io, &touch_cfg, &s_touch),
                        TAG, "esp_lcd_touch_new_i2c_cst816s");

    /* --- Bind to LVGL ----------------------------------------------------- */
    const lvgl_port_touch_cfg_t port_cfg = {
        .disp   = disp,
        .handle = s_touch,
    };
    s_indev = lvgl_port_add_touch(&port_cfg);
    if (!s_indev) {
        ESP_LOGE(TAG, "lvgl_port_add_touch failed");
        esp_lcd_touch_del(s_touch);
        s_touch = NULL;
        return ESP_FAIL;
    }

    /*
     * The sampler takes over the reading, so the input device stops waiting on the
     * controller's interrupt — which only ever wakes a task that is busy — and runs on its
     * own timer instead, emptying the queue a change at a time. See the note above the queue:
     * this is the whole of why a double tap could be lost on a page whose frames are heavy.
     */
    if (bsp_display_lock(0)) {
        lv_indev_set_mode(s_indev, LV_INDEV_MODE_TIMER);
        lv_indev_set_read_cb(s_indev, touch_read_cb);

        lv_timer_t *read_timer = lv_indev_get_read_timer(s_indev);
        if (read_timer != NULL) {
            lv_timer_set_period(read_timer, BSP_TOUCH_READ_TIMER_MS);
        }

        bsp_display_unlock();
    }

    if (xTaskCreatePinnedToCore(touch_sampler_task, "bsp_touch", 3072, NULL, 6, NULL, 0) != pdPASS) {
        ESP_LOGE(TAG, "no task for the touch sampler");
        return ESP_ERR_NO_MEM;
    }

    ESP_LOGI(TAG, "CST816S ready at %d kHz (INT %d, RST %d), sampled every %d ms",
             CONFIG_BSP_TOUCH_I2C_FREQ_KHZ, BSP_TOUCH_INT, BSP_TOUCH_RST,
             BSP_TOUCH_SAMPLE_MS);
    return ESP_OK;
}

uint32_t bsp_touch_contact(void)
{
    return s_contact;
}

bool bsp_touch_get_live(uint16_t *x, uint16_t *y)
{
    if (x) { *x = s_live_x; }
    if (y) { *y = s_live_y; }
    return s_live_pressed;
}

bool bsp_touch_is_ready(void)
{
    return s_touch != NULL;
}

esp_lcd_touch_handle_t bsp_touch_get_handle(void)
{
    return s_touch;
}

lv_indev_t *bsp_touch_get_indev(void)
{
    return s_indev;
}

esp_err_t bsp_touch_set_swap_xy(bool swap)
{
    if (!s_touch) {
        return ESP_ERR_INVALID_STATE;
    }
    return esp_lcd_touch_set_swap_xy(s_touch, swap);
}

esp_err_t bsp_touch_set_mirror(bool mirror_x, bool mirror_y)
{
    if (!s_touch) {
        return ESP_ERR_INVALID_STATE;
    }

    esp_err_t err = esp_lcd_touch_set_mirror_x(s_touch, mirror_x);
    if (err != ESP_OK) {
        return err;
    }
    return esp_lcd_touch_set_mirror_y(s_touch, mirror_y);
}

esp_err_t bsp_touch_deinit(void)
{
    if (!s_touch) {
        return ESP_OK;
    }

    if (s_indev) {
        lvgl_port_remove_touch(s_indev);
        s_indev = NULL;
    }

    esp_lcd_touch_del(s_touch);
    s_touch = NULL;

    if (s_touch_io) {
        esp_lcd_panel_io_del(s_touch_io);
        s_touch_io = NULL;
    }

    /* The bus itself belongs to the BSP and stays up after the touch driver. */
    return ESP_OK;
}
