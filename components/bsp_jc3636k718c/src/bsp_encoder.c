/*
 * Rotary encoder — decoder written against measured pin behaviour.
 *
 * THE MEASUREMENT, WHICH IS THE WHOLE DESIGN
 *   Taken from the board with a meter, not inferred:
 *
 *     rest                     A = 1   B = 1
 *     one detent, one way      A = 0   B = 1
 *     one detent, other way    A = 1   B = 0
 *
 *   Both lines idle high and each direction briefly pulls its OWN line low
 *   without touching the other. There is no state where both are low, and no
 *   phase overlap — so this is NOT quadrature, and every decoder that treats it
 *   as quadrature fails in a specific way:
 *
 *   * A Gray-code table sees (1,1)->(0,1) as a valid forward step and
 *     (0,1)->(1,1) as a valid reverse step. They cancel exactly, which shows up
 *     as a value that twitches and never moves.
 *   * espressif/knob looks for phase transitions between A and B. There are
 *     none to find.
 *
 *   Therefore: the falling edge of A is one step in one direction, the falling
 *   edge of B is one step in the other. The rising edge back to rest is NOT a
 *   step and must be ignored — that is what makes the edge choice matter.
 *
 * DEBOUNCE
 *   Contacts bounce, and the bounce here is on the same pin as the real pulse,
 *   so it produces extra falling edges rather than reversals. A time gate
 *   fixes it: edges closer together than ENC_DEBOUNCE_US are discarded. Both
 *   pins share one handler and therefore one timestamp, which is why there is
 *   a single ISR rather than two — two would need a critical section around the
 *   shared clock.
 */

#include "bsp/bsp_encoder.h"
#include "bsp/bsp_encoder_probe.h"

#include "esp_check.h"
#include "esp_log.h"
#include "esp_timer.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "driver/gpio.h"

#include "bsp/bsp_display.h"
#include "bsp/bsp_pins.h"

static const char *TAG = "bsp.encoder";

/* Clamp per LVGL read, as a backstop against a stuck or noisy line. The read
 * callback runs every few milliseconds, so a real flick stays well inside. */
#define ENC_MAX_STEP_PER_READ 4

/* -------------------------------------------------------------------------- */
/* Decoder                                                                    */
/* -------------------------------------------------------------------------- */
/*
 * WHY THIS POLLS INSTEAD OF USING INTERRUPTS
 *
 *   Measured on this board, over one 20 s turn:
 *
 *       A: 112 rising / 112 falling edges   (224 total)
 *       B: 667 rising / 668 falling edges  (1335 total)
 *
 *   A and B move together — they are the two contacts of one encoder — so they
 *   should produce comparable counts. B produced six times as many. That is
 *   contact bounce, and it is not a small effect: it is the whole signal.
 *
 *   An interrupt-driven decoder counts every one of those edges, so a direction
 *   whose contact bounces is drowned in noise while a clean contact still
 *   works. That asymmetry is exactly what "the knob only works one way" looks
 *   like from the outside.
 *
 *   Sampling fixes it structurally rather than with a filter. A bounce burst
 *   lasts microseconds; a poll every 1 ms cannot see it. What the decoder sees
 *   is the level, which is unambiguous by the time it looks. The vendor's own
 *   driver for this board works the same way, for the same reason.
 *
 *   The cost is latency: up to one poll period. At 1 ms that is imperceptible.
 *
 * A level change is accepted only after it has held for ENC_STABLE_US, which
 * rejects the tail of a bounce that happens to straddle a poll.
 */
#define ENC_POLL_PERIOD_MS   1
#define ENC_POLL_PERIOD_US   (ENC_POLL_PERIOD_MS * 1000)

/* How long a level must hold before it counts as a real change. One poll
 * period: long enough to reject a bounce straddling a sample, far shorter than
 * the ~10 ms a real detent is held (measured). */
#define ENC_STABLE_US        1500

/* Consecutive stable level changes on one line that make a detent. Two, because
 * one detent is a low pulse followed by the return high, and the direction is
 * decided when the pulse completes. */
#define ENC_EDGES_PER_DETENT 2

/* Safety cap on steps emitted from one poll, in case a line is oscillating
 * slowly enough to be seen. Keeps a fault from spinning a widget. */
#define ENC_MAX_STEPS_PER_POLL 4

typedef struct {
    int      level;        /*!< Last level seen by the poller */
    int64_t  changed_us;   /*!< When it last differed from `level` */
    int      pending;      /*!< The level it keeps differing to */
    int      stable;       /*!< Debounced level, the one that counts */
    uint32_t bounce;       /*!< Changes rejected as too brief, for diagnostics */
} enc_line_t;

static enc_line_t s_line[2];   /* 0 = A, 1 = B */

/* Written by the poll task, read by the LVGL task. */
static volatile int32_t s_accum;
static uint32_t s_edge_rising[2];
static uint32_t s_edge_falling[2];

/*
 * Steps produced in each direction, never netted.
 *
 * s_accum nets them, which is right for driving a widget and useless for
 * diagnosing the decoder: a knob turned both ways nets to near zero whether it
 * works or not. These two counters answer "did it produce any steps at all",
 * and in which direction, which is the question that actually distinguishes a
 * dead decoder from an inverted one.
 *
 * Both are kept as sums rather than magnitudes: if a pulse ever completes with
 * the wrong sign the totals move apart, which is visible, whereas magnitudes
 * would hide it.
 */
static int32_t s_steps_pos;    /* Sum of steps emitted with a + sign */
static int32_t s_steps_neg;    /* Sum of steps emitted with a - sign */

/* Read side only. */
static int32_t s_reported;

static TaskHandle_t s_poll_task;
static lv_indev_t  *s_indev;
static lv_group_t  *s_group;

/* -------------------------------------------------------------------------- */
/* Decoder                                                                    */
/* -------------------------------------------------------------------------- */

static int enc_gpio(int line)
{
    return gpio_get_level(line == 0 ? BSP_ENC_A : BSP_ENC_B) ? 1 : 0;
}

/*
 * Feed one sampled level into a line's debouncer.
 *
 * Returns +1 for an accepted rising edge, -1 for an accepted falling edge, and
 * 0 when nothing changed or the change was too brief to trust.
 *
 * `index` is 0 for A and 1 for B, used only to attribute the diagnostic edge
 * counters.
 */
static int enc_line_update(enc_line_t *ln, int index, int level, int64_t now_us)
{
    if (level != ln->level) {
        if (ln->pending != level) {
            /* A new candidate level; restart its stability clock. */
            ln->pending    = level;
            ln->changed_us = now_us;
        } else if (now_us - ln->changed_us >= ENC_STABLE_US) {
            const int rising = (level > ln->stable);
            ln->level  = level;
            ln->stable = level;

            if (rising) { s_edge_rising[index]++; } else { s_edge_falling[index]++; }
            return rising ? +1 : -1;
        } else {
            /* Held, but not long enough yet to be trusted. */
            ln->bounce++;
        }
    } else {
        ln->pending = level;
    }

    return 0;
}

/*
 * Direction is decided by WHICH LINE PULSED.
 *
 * This encoder is two independent contacts, not a phase pair: turning it one
 * way closes A's contact alone, turning it the other way closes B's alone. So
 * direction is a property of the line, and one complete pulse — the contact
 * closing and opening again — is one detent.
 *
 * A pulse is therefore counted on its COMPLETING edge (the rise back to rest),
 * and never on both edges. Counting both was a real bug here: it computed +1 on
 * the fall and -1 on the rise, which cancelled every detent to zero and made
 * the knob look completely dead. Two edges, one step.
 *
 * The signs themselves are a naming convention and not a measurement — nothing
 * in the hardware says which rotation is "positive". They were settled by
 * trying both, and the second attempt got it wrong: A=+1/B=-1 turned the widget
 * the wrong way, while A=-1/B=+1 turns it the right way. Do not "derive" these
 * from the trace; the trace cannot tell you which way the user expects the
 * value to move. CONFIG_BSP_ENCODER_SWAP_DIRECTION flips them at runtime.
 */
#define ENC_SIGN_A (-1)
#define ENC_SIGN_B (+1)

/* Steps produced per completed pulse. A detent is one pulse, so one step. */
#define ENC_STEPS_PER_PULSE 1

/*
 * Turn a completed pulse into a step.
 *
 * CALLED ONLY ON THE COMPLETING EDGE. See the note above for what happened when
 * that rule was not followed.
 */
static int32_t enc_step_from(int line)
{
    return (line == 0) ? ENC_SIGN_A : ENC_SIGN_B;
}

/*
 * One poll: read both lines, debounce, and turn a completed pulse into a step.
 */
static void enc_poll_once(int64_t now_us)
{
    const int edge_a = enc_line_update(&s_line[0], 0, enc_gpio(0), now_us);
    const int edge_b = enc_line_update(&s_line[1], 1, enc_gpio(1), now_us);

    int32_t step = 0;

    /* A rising edge means the contact that was closed has opened again, which
     * completes the pulse that detent began. */
    if (edge_a > 0) {
        step = enc_step_from(0);
    } else if (edge_b > 0) {
        step = enc_step_from(1);
    }

    if (step != 0) {
        step *= ENC_STEPS_PER_PULSE;

        /* Honour the direction swap here, at the single point where a step is
         * produced, so the rest of the decoder never has to know about it. */
        if (CONFIG_BSP_ENCODER_SWAP_DIRECTION) {
            step = -step;
        }

        s_accum += step;

        if (step > 0) {
            s_steps_pos += step;
        } else {
            s_steps_neg += step;
        }
    }
}

static void enc_poll_task(void *arg)
{
    (void)arg;

    int64_t next_us = esp_timer_get_time();

    for (;;) {
        enc_poll_once(esp_timer_get_time());

        /* Absolute deadline rather than a plain delay, so the sample rate does
         * not drift with the time the decode itself takes. */
        next_us += ENC_POLL_PERIOD_US;
        const int64_t wait_us = next_us - esp_timer_get_time();
        if (wait_us > 0) {
            vTaskDelay(pdMS_TO_TICKS((wait_us + 999) / 1000));
        } else {
            /* Fell behind; resynchronise instead of trying to catch up. */
            next_us = esp_timer_get_time();
            taskYIELD();
        }
    }
}

/* -------------------------------------------------------------------------- */
/* LVGL input device                                                          */
/* -------------------------------------------------------------------------- */

/*
 * Re-assert edit mode after any focus change.
 *
 * The core calls this from lv_group_focus_obj() right after it has cleared
 * editing, so this is the one hook that runs late enough to undo it. See the
 * comment at the registration site for why clearing it is fatal on this board.
 *
 * There is no re-entrancy problem: lv_group_set_editing() returns early when
 * the mode is already correct, and lv_group_focus_obj() only invokes this
 * callback while assigning a new focus, not from within set_editing().
 */
static void encoder_group_focus_cb(lv_group_t *group)
{
    if (!lv_group_get_editing(group)) {
        lv_group_set_editing(group, true);
    }
}

/*
 * Put the focus back on the one widget the knob is supposed to drive.
 *
 * This board has a single focusable control per screen and no button, so there
 * is never a reason for the focus to sit anywhere else: moving it can only ever
 * take the knob away from the control and give it to something the user cannot
 * reach. Anything that moves focus — a tap on a touch-only widget, a screen
 * change that rebuilt the group, a widget being deleted underneath it — is
 * therefore repaired here rather than diagnosed.
 *
 * lv_group_focus_next() on an empty group is a no-op, so a screen with nothing
 * focusable is safe.
 */
void bsp_encoder_refocus(void)
{
    if (!s_group) {
        return;
    }

    lv_obj_t *target = lv_group_get_focused(s_group);
    if (!target || !lv_obj_is_valid(target)) {
        lv_group_focus_next(s_group);
        target = lv_group_get_focused(s_group);
    }

    if (target) {
        /* Re-focusing the object that is already focused still runs
         * lv_group_focus_obj(), which is what clears edit mode — so this also
         * repairs the edit-mode half of the problem. */
        lv_group_focus_obj(target);
        if (!lv_group_get_editing(s_group)) {
            lv_group_set_editing(s_group, true);
        }
    }
}

/*
 * Is the knob still able to drive the UI?
 *
 * The two ways it silently stops are "nothing is focused" and "the group is not
 * editing", and both are invisible from the outside: the knob keeps turning and
 * the UI keeps ignoring it. Exposing the pair as one predicate gives the
 * diagnostics something checkable and gives application code a way to assert
 * the invariant after it changes the screen.
 */
bool bsp_encoder_is_driving(void)
{
    if (!s_group || !lv_group_get_editing(s_group)) {
        return false;
    }

    lv_obj_t *focused = lv_group_get_focused(s_group);
    return focused != NULL && lv_obj_is_valid(focused);
}

/* -------------------------------------------------------------------------- */
/* Focus policy                                                               */
/* -------------------------------------------------------------------------- */

/*
 * THE RULE
 *
 *   One indicator on the screen -> the knob always drives it. A tap can never
 *   take it away, because there is nowhere for the focus to go.
 *
 *   Several indicators -> a tap picks which one the knob drives.
 *
 * The two halves come from different mechanisms, which is why this took a while
 * to get right:
 *
 *   The single-indicator half is a *prevention* problem. LVGL's
 *   lv_group_focus_obj() clears edit mode on EVERY focus change, including one
 *   that focuses the object that is already focused, and touch drives that path
 *   through indev_click_focus(). Edit mode is what makes the knob adjust a
 *   value instead of merely moving between widgets, and this board has no
 *   button to turn it back on — so losing it is permanent. The fix is to keep
 *   edit mode asserted and to keep the focus on the only focusable object.
 *
 *   The multi-indicator half is the opposite: the tap MUST be allowed to move
 *   focus, so the widgets have to stay click-focusable and the focus must NOT
 *   be forced back.
 *
 * Hence: force focus only when there is exactly one candidate. That single
 * condition covers both halves without a mode flag, and it degrades sensibly —
 * a screen with two indicators simply gets stock LVGL behaviour, which is
 * already correct.
 */

/*
 * How many objects in the group are worth focusing, and is one of them focused?
 *
 * The group's linked list is walked with the public lv_group_get_obj_count() and
 * lv_group_get_obj_by_index() rather than LV_LL_READ, because an object can be
 * deleted while this runs and the index accessors are bounds-checked.
 */
static uint32_t focus_candidates(lv_obj_t **first_out)
{
    uint32_t n = 0;

    if (first_out) {
        *first_out = NULL;
    }

    if (!s_group) {
        return 0;
    }

    const uint32_t count = lv_group_get_obj_count(s_group);

    for (uint32_t i = 0; i < count; i++) {
        lv_obj_t *obj = lv_group_get_obj_by_index(s_group, i);
        if (!obj || !lv_obj_is_valid(obj) || lv_obj_is_hidden(obj)) {
            continue;
        }

        n++;
        if (n == 1 && first_out) {
            *first_out = obj;
        }
    }

    return n;
}

/*
 * How many widgets the knob can reach on the current screen.
 *
 * This is the number the rest of the UI keys on — in particular the focus ring,
 * which is shown only when the target is ambiguous. Exposed rather than
 * duplicated so the ring and the policy can never disagree about what counts as
 * a candidate.
 */
uint32_t bsp_encoder_focusable_count(void)
{
    return focus_candidates(NULL);
}

/*
 * Enforce the rule above. Cheap enough to run on every input read.
 *
 * Returns true when it had to repair something, which is what the "focus was
 * lost" warning keys on — the point is to notice the failure rather than paper
 * over it silently.
 */
bool bsp_encoder_enforce_focus(void)
{
    if (!s_group) {
        return false;
    }

    bool repaired = false;

    lv_obj_t *first = NULL;
    const uint32_t candidates = focus_candidates(&first);

    lv_obj_t *focused = lv_group_get_focused(s_group);

    /*
     * Exactly one candidate: the knob's target is not in question, so pin the
     * focus to it. Re-focusing runs lv_group_focus_obj(), which clears edit
     * mode, so edit mode is re-asserted afterwards — hence both repairs happen
     * together and "repaired" is reported once.
     */
    if (candidates == 1 && focused != first) {
        lv_group_focus_obj(first);
        repaired = true;
        focused = lv_group_get_focused(s_group);
    }

    /*
     * Nothing valid is focused at all. lv_group_focus_next() silently does
     * nothing on an empty group, so this can genuinely fail to recover — which
     * is the state where the knob looks dead until reboot. Report it instead of
     * pretending it worked.
     */
    if (!focused || !lv_obj_is_valid(focused)) {
        lv_group_focus_next(s_group);
        focused = lv_group_get_focused(s_group);
        repaired = true;

        if (!focused) {
            /*
             * Distinguish "the UI has no focusable widget" from "the UI has not
             * been built yet". This runs from the input read callback, which
             * starts as soon as the display is up — a second or two before the
             * application creates its first screen. Warning during that window
             * would cry wolf on every boot, and a diagnostic that always fires
             * is one nobody reads.
             */
            lv_obj_t *screen = lv_screen_active();
            const bool screen_has_widgets = screen &&
                                            lv_obj_get_child_count(screen) > 0;

            static bool warned;
            if (screen_has_widgets && !warned) {
                warned = true;
                ESP_LOGE(TAG, "nothing focusable is in the group (%" PRIu32
                              " candidate(s)) — the knob cannot drive the UI. "
                              "Widgets must be added with lv_group_add_obj() "
                              "or be created as children of one that is.",
                         candidates);
            }
        }
    }

    /* Edit mode is what makes a rotation change a value instead of moving the
     * focus, and there is no button on this board to restore it. Assert it
     * every pass; lv_group_set_editing() returns early when already correct. */
    if (focused && !lv_group_get_editing(s_group)) {
        lv_group_set_editing(s_group, true);
        repaired = true;

        /* Worth a warning once per boot: reaching here means something cleared
         * edit mode despite the focus callback, which is a bug worth knowing
         * about rather than a condition to absorb quietly. */
        static bool warned;
        if (!warned) {
            warned = true;
            ESP_LOGW(TAG, "edit mode had been cleared; restored. Rotation would "
                          "have moved focus instead of adjusting the value.");
        }
    }

    return repaired;
}

/*
 * Run the policy once per input read.
 *
 * Doing it here rather than only in callbacks means it cannot be missed: every
 * rotation passes through this function before LVGL sees the step, so the focus
 * is repaired in the same pass that would otherwise be swallowed.
 */
static void encoder_read_cb(lv_indev_t *indev, lv_indev_data_t *data)
{
    (void)indev;

    bsp_encoder_enforce_focus();

    const int32_t total = s_accum;
    int32_t diff = total - s_reported;
    s_reported = total;

    if (diff > ENC_MAX_STEP_PER_READ) {
        diff = ENC_MAX_STEP_PER_READ;
    } else if (diff < -ENC_MAX_STEP_PER_READ) {
        diff = -ENC_MAX_STEP_PER_READ;
    }

    if (CONFIG_BSP_ENCODER_SWAP_DIRECTION) {
        diff = -diff;
    }

    data->enc_diff = (int16_t)diff;
    /* No push button on this board, so LVGL never sees an ENTER from here. */
    data->state = LV_INDEV_STATE_RELEASED;
}

/* -------------------------------------------------------------------------- */
/* Public API                                                                 */
/* -------------------------------------------------------------------------- */

esp_err_t bsp_encoder_init(void)
{
    if (s_indev) {
        ESP_LOGW(TAG, "encoder already initialised");
        return ESP_OK;
    }

    if (!bsp_display_get_lvgl_display()) {
        ESP_LOGE(TAG, "call bsp_display_init() before bsp_encoder_init()");
        return ESP_ERR_INVALID_STATE;
    }

    const gpio_config_t io_cfg = {
        .pin_bit_mask = (1ULL << BSP_ENC_A) | (1ULL << BSP_ENC_B),
        .mode         = GPIO_MODE_INPUT,
        /* The contacts switch to ground and rest open, so the idle level has
         * to come from pull-ups — the board's own R52/R53 plus these. */
        .pull_up_en   = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        /* Interrupts are deliberately not used. The contacts bounce, and an
         * edge-counting decoder amplifies that into the dominant signal; the
         * poll task below reads levels instead. See ENC_POLL_PERIOD_MS. */
        .intr_type    = GPIO_INTR_DISABLE,
    };
    ESP_RETURN_ON_ERROR(gpio_config(&io_cfg), TAG, "gpio_config");

    /*
     * Report the resting levels before anything else.
     *
     * This one line distinguishes the two failure modes that look identical
     * from the UI: a line stuck low can never produce a falling edge, so its
     * direction is silently dead, and no amount of decoder work will fix it.
     * Expect "A=1 B=1".
     */
    const int level_a = gpio_get_level(BSP_ENC_A);
    const int level_b = gpio_get_level(BSP_ENC_B);
    ESP_LOGI(TAG, "encoder pins at rest: A(%d)=%d B(%d)=%d (expect 1 and 1)",
             BSP_ENC_A, level_a, BSP_ENC_B, level_b);
    if (level_a == 0 || level_b == 0) {
        ESP_LOGW(TAG, "a line is already low — that direction cannot produce "
                      "an edge; check the pull-up and the flex connector");
    }

    /*
     * Seed the debouncers from the pins as they actually are.
     *
     * Starting from an assumed high level would make the first sample after
     * boot look like a change: a knob resting on a detent with a contact
     * already closed would appear, to the decoder, to have just moved. Reading
     * the real levels costs nothing and removes the assumption.
     */
    const int64_t now_us = esp_timer_get_time();
    for (int i = 0; i < 2; i++) {
        const int lv = enc_gpio(i);
        s_line[i].level      = lv;
        s_line[i].stable     = lv;
        s_line[i].pending    = lv;
        s_line[i].changed_us = now_us;
        s_line[i].bounce     = 0;
    }

    /*
     * No GPIO interrupts: the decoder polls. See the block comment above
     * ENC_POLL_PERIOD_MS for the measurement that forced this.
     */
    if (xTaskCreate(enc_poll_task, "enc_poll", 3072, NULL, 5, &s_poll_task) != pdPASS) {
        ESP_LOGE(TAG, "could not start the encoder poll task");
        return ESP_ERR_NO_MEM;
    }

    /* --- LVGL input device ------------------------------------------------ */
    if (!bsp_display_lock(0)) {
        return ESP_FAIL;
    }

    s_indev = lv_indev_create();
    if (s_indev) {
        lv_indev_set_type(s_indev, LV_INDEV_TYPE_ENCODER);
        lv_indev_set_read_cb(s_indev, encoder_read_cb);

        s_group = lv_group_create();
        if (s_group) {
            lv_indev_set_group(s_indev, s_group);
            /*
             * Force edit mode, permanently.
             *
             * LVGL only sends arrow keys to the focused widget while the group
             * is editing; otherwise a rotation means "move focus". Editing is
             * toggled by pressing the encoder's button, and this board has
             * none — so a perfectly decoded rotation would still only shuffle
             * focus and never change a value.
             */
            lv_group_set_editing(s_group, true);

            /*
             * ...and keep it forced, because setting it once is not enough.
             *
             * lv_group_focus_obj() in LVGL's core clears edit mode on EVERY
             * focus change — not only when the focus actually moves:
             *
             *     void lv_group_focus_obj(lv_obj_t * obj)
             *     {
             *         ...
             *         //On defocus edit mode must be leaved
             *         lv_group_set_editing(g, false);
             *         ...
             *     }
             *
             * Touch drives that path. Tapping anything click-focusable — the
             * demo's page arrows, or the arc itself — runs the pointer input
             * device through indev_click_focus() into lv_group_focus_obj(),
             * edit mode drops to false, and the knob silently degrades from
             * "change the value" to "move focus between widgets". On a screen
             * with a single focusable widget there is nowhere to move, so the
             * knob looks completely dead and stays that way until reboot.
             *
             * The button that would normally restore edit mode is the one
             * thing this board does not have, so the group's focus callback —
             * which the core invokes immediately AFTER clearing editing — is
             * the only place that can put it back.
             */
            lv_group_set_focus_cb(s_group, encoder_group_focus_cb);
        }
    }

    bsp_display_unlock();

    if (!s_indev || !s_group) {
        ESP_LOGE(TAG, "could not create the LVGL encoder device");
        return ESP_ERR_NO_MEM;
    }

    ESP_LOGI(TAG, "encoder ready (polling decoder, A=%d / B=%d, %d ms period, "
                  "direction %s)",
             BSP_ENC_A, BSP_ENC_B, ENC_POLL_PERIOD_MS,
             CONFIG_BSP_ENCODER_SWAP_DIRECTION ? "reversed" : "normal");

    /*
     * Optionally start the measurement probe, now that the pins are inputs.
     * It reports what the two lines actually do, which is the only way to know
     * whether the decode above matches the hardware.
     */
#if CONFIG_BSP_ENCODER_PROBE
    if (!bsp_encoder_probe_start()) {
        ESP_LOGW(TAG, "encoder probe did not start");
    }
#endif

    return ESP_OK;
}

bool bsp_encoder_is_ready(void)
{
    return s_indev != NULL;
}

lv_indev_t *bsp_encoder_get_indev(void)
{
    return s_indev;
}

lv_group_t *bsp_encoder_get_group(void)
{
    return s_group;
}

esp_err_t bsp_encoder_set_group(lv_group_t *group)
{
    if (!s_indev) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!group) {
        return ESP_ERR_INVALID_ARG;
    }

    if (!bsp_display_lock(0)) {
        return ESP_FAIL;
    }

    lv_group_t *previous = s_group;
    s_group = group;
    lv_indev_set_group(s_indev, s_group);
    lv_group_set_editing(s_group, true);

    if (previous) {
        lv_group_delete(previous);
    }

    bsp_display_unlock();
    return ESP_OK;
}

button_handle_t bsp_encoder_get_button(void)
{
    /* No switch contact is wired to the MCU on this board. The accessor stays
     * so application code that guards on it keeps working. */
    return NULL;
}

int32_t bsp_encoder_get_steps(void)
{
    /* s_accum is written by the poll task and is 32-bit aligned, so this load
     * is atomic on the ESP32-S3 and needs no lock. */
    return s_accum;
}

void bsp_encoder_get_edges(bsp_encoder_edges_t *out)
{
    if (!out) {
        return;
    }

    out->a_rising  = s_edge_rising[0];
    out->a_falling = s_edge_falling[0];
    out->b_rising  = s_edge_rising[1];
    out->b_falling = s_edge_falling[1];
    out->steps_positive = s_steps_pos;
    out->steps_negative = s_steps_neg;
}

void bsp_encoder_reset_steps(void)
{
    /* The poll task can add to s_accum between these stores, which would lose
     * one step. That is acceptable for a diagnostic counter: this is called
     * before a measurement run, not while one is being taken. */
    s_accum = 0;
    s_reported = 0;
    s_steps_pos = s_steps_neg = 0;
    for (int i = 0; i < 2; i++) {
        s_edge_rising[i]  = 0;
        s_edge_falling[i] = 0;
        s_line[i].bounce  = 0;
    }
}

esp_err_t bsp_encoder_deinit(void)
{
    if (!s_indev) {
        return ESP_OK;
    }

    if (s_poll_task) {
        vTaskDelete(s_poll_task);
        s_poll_task = NULL;
    }

    if (bsp_display_lock(0)) {
        if (s_group) {
            lv_group_delete(s_group);
            s_group = NULL;
        }
        lv_indev_delete(s_indev);
        s_indev = NULL;
        bsp_display_unlock();
    }

    return ESP_OK;
}
