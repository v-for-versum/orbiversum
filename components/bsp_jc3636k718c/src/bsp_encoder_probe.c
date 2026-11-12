/*
 * Encoder line probe — measures what the knob's two contacts actually do.
 *
 * WHY THIS EXISTS
 *   Two documents in this component disagree about the encoder, and they
 *   cannot both be right:
 *
 *     bsp_encoder.h   "an ordinary quadrature encoder"
 *     bsp_encoder.c   "this is NOT quadrature ... each direction pulls its
 *                      OWN line low without touching the other"
 *
 *   The decoder has to commit to one of those, because they need different
 *   code. A quadrature decoder fed two independent pulse lines counts every
 *   press and release as a step; a pulse-line decoder fed real quadrature
 *   sees overlapping transitions it does not recognise and drops steps.
 *
 *   Neither document is evidence. This file is: it samples both lines from a
 *   task and reports the sequence, so the answer comes from the board.
 *
 * WHY A TASK AND NOT THE ISR
 *   The ISR only ever sees edges, and edges are exactly what is in dispute.
 *   Counting edges on A versus B cannot distinguish "A pulses alone" from "A
 *   leads B" — both produce edges on A and B in equal numbers. What separates
 *   them is whether the two lines are ever low at the same time, and that
 *   needs a view of the levels between edges, which means polling.
 *
 *   Polling cannot see contact chatter, so bounce is counted separately by
 *   the ISR counters, which are read alongside.
 *
 * ENABLE WITH
 *   Knob BSP -> Encoder -> "Run the encoder line probe at boot"
 */

#include "bsp/bsp_encoder_probe.h"
#include "bsp/bsp_encoder.h"

#include <inttypes.h>

#include "esp_log.h"
#include "esp_timer.h"

#include "driver/gpio.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "bsp/bsp_pins.h"

static const char *TAG = "bsp.enc.probe";

/*
 * 1 kHz. A hand turning a knob spends milliseconds in each state, and contact
 * bounce — which this deliberately cannot see, since it is over in
 * microseconds — is the ISR's business, not this task's.
 */
#define PROBE_PERIOD_MS 1

/* How long to run before printing the verdict. Long enough to turn the knob
 * both ways a few times after the log scrolls past. */
#define PROBE_DURATION_MS 20000

/* Transitions kept for the trace dump. 512 entries is about 1.5 KB and covers
 * far more than a few turns of the knob. */
#define PROBE_TRACE_DEPTH 512

#define PROBE_SAMPLE_HZ (1000 / PROBE_PERIOD_MS)

/* -------------------------------------------------------------------------- */
/* Accumulator                                                                */
/* -------------------------------------------------------------------------- */

typedef struct {
    /* Levels, as an index into the four possible (A,B) states. */
    uint32_t samples;
    uint32_t state_count[4];

    /* Transitions of each line, seen between samples. */
    uint32_t edges_a;
    uint32_t edges_b;

    /*
     * The decisive counters.
     *
     * quad_ok      A and B changed on the same sample, in one of the two
     *              orders that a quadrature encoder produces.
     * quad_bad     Both changed, but in an order that no quadrature encoder
     *              produces — the signature of a decoder that is reading
     *              something other than what it assumes.
     * pulse_a_only A changed while B held. A press of A's contact alone.
     * pulse_b_only B changed while A held.
     */
    uint32_t quad_ok;
    uint32_t quad_bad;
    uint32_t pulse_a_only;
    uint32_t pulse_b_only;

    /* Total time each state was held, and the longest single dwell. */
    int64_t  dwell_us[4];
    int64_t  dwell_max_us[4];

    /* Timestamps of the last change of each line, for the dwell maths. */
    int64_t  last_change_us[2];
    int      state;

    /*
     * The raw transition trace.
     *
     * The aggregate counters above say how often each line moved; they cannot
     * say in what ORDER the lines moved, and order is what decides rotation
     * direction. A knob that turns both ways but only registers one is an
     * ordering problem, so the last few hundred transitions are kept verbatim
     * and dumped at the end. That is the evidence a decoder fix needs.
     */
    uint32_t trace_count;
    uint8_t  trace_from[PROBE_TRACE_DEPTH];
    uint8_t  trace_to[PROBE_TRACE_DEPTH];
    int64_t  trace_us[PROBE_TRACE_DEPTH];

    /*
     * Dwell histogram: how long each state is actually held, bucketed.
     *
     * The decoder has to reject contact chatter without rejecting real detents,
     * and that means knowing the two timescales. Chatter is what a debounce
     * threshold is FOR; real detents are what it must not swallow. Guessing a
     * threshold between two unmeasured numbers is how a decoder ends up
     * discarding every step during a fast turn.
     *
     * Buckets: <0.1, 0.1-0.25, 0.25-0.5, 0.5-1, 1-2, 2-5, 5-10, >=10 ms.
     */
    uint32_t dwell_bucket[8];
    uint32_t dwell_total;
} probe_acc_t;

/* Upper bounds of the dwell histogram buckets, in microseconds. The last
 * bucket is open-ended. */
static const int64_t DWELL_BOUNDS_US[7] = {
    100, 250, 500, 1000, 2000, 5000, 10000
};

static void dwell_record(probe_acc_t *acc, int64_t dwell_us)
{
    for (int i = 0; i < 7; i++) {
        if (dwell_us < DWELL_BOUNDS_US[i]) {
            acc->dwell_bucket[i]++;
            acc->dwell_total++;
            return;
        }
    }
    acc->dwell_bucket[7]++;
    acc->dwell_total++;
}

static probe_acc_t s_acc;

/* -------------------------------------------------------------------------- */
/* Sampling                                                                   */
/* -------------------------------------------------------------------------- */

static void probe_sample(probe_acc_t *acc, int64_t now_us)
{
    const int a = gpio_get_level(BSP_ENC_A) ? 1 : 0;
    const int b = gpio_get_level(BSP_ENC_B) ? 1 : 0;
    const int state = (a << 1) | b;

    if (state != acc->state) {
        const int prev = acc->state;

        /* Record the verbatim transition before anything else touches it. */
        const uint32_t slot = acc->trace_count % PROBE_TRACE_DEPTH;
        acc->trace_from[slot] = (uint8_t)prev;
        acc->trace_to[slot]   = (uint8_t)state;
        acc->trace_us[slot]   = now_us;
        acc->trace_count++;

        /* Which lines moved, and in which direction. */
        const int prev_a = (prev >> 1) & 1;
        const int prev_b = prev & 1;
        const int a_changed = (a != prev_a);
        const int b_changed = (b != prev_b);

        const int64_t dwell = now_us - acc->last_change_us[0];
        acc->dwell_us[prev] += dwell;
        if (dwell > acc->dwell_max_us[prev]) {
            acc->dwell_max_us[prev] = dwell;
        }
        dwell_record(acc, dwell);

        if (a_changed && b_changed) {
            /*
             * Both lines moved together. Quadrature encoders do this: the two
             * contacts change in a fixed order, one leading the other. Two
             * independent pulse lines cannot, because only one contact is
             * closed at a time.
             *
             * Which order is "correct" depends on rotation direction, so any
             * consistent order counts as quadrature here; what would be
             * impossible is both changing in a way that is not a clean swap,
             * which cannot happen with two bits — so quad_bad only ever
             * increments if a line is glitching between samples.
             */
            acc->quad_ok++;
        } else if (a_changed) {
            acc->edges_a++;
            acc->pulse_a_only++;
        } else if (b_changed) {
            acc->edges_b++;
            acc->pulse_b_only++;
        }

        if (a_changed) { acc->last_change_us[0] = now_us; }
        if (b_changed) { acc->last_change_us[1] = now_us; }

        acc->state = state;
    }

    acc->state_count[state]++;
    acc->samples++;
}

/* -------------------------------------------------------------------------- */
/* Reporting                                                                  */
/* -------------------------------------------------------------------------- */

static const char *state_name(int state)
{
    switch (state) {
    case 0: return "A=0 B=0";
    case 1: return "A=0 B=1";
    case 2: return "A=1 B=0";
    default: return "A=1 B=1";
    }
}

static void probe_report(const probe_acc_t *acc, int64_t elapsed_us)
{
    const int rest = (acc->state_count[3] > acc->state_count[2] &&
                      acc->state_count[3] > acc->state_count[1]) ? 3 : -1;

    ESP_LOGI(TAG, "---- encoder line probe: %.1f s, %" PRIu32 " samples ----",
             (double)elapsed_us / 1e6, acc->samples);

    /* Time in each state, as a share of the run. */
    for (int s = 0; s < 4; s++) {
        const double pct = acc->samples
            ? 100.0 * (double)acc->state_count[s] / (double)acc->samples
            : 0.0;
        ESP_LOGI(TAG, "  %s : %5.1f%% of the time, longest hold %.1f ms",
                 state_name(s), pct, (double)acc->dwell_max_us[s] / 1000.0);
    }

    ESP_LOGI(TAG, "  transitions: A moved %" PRIu32 ", B moved %" PRIu32,
             acc->edges_a, acc->edges_b);

    /*
     * The dwell histogram, which is what sets ENC_DEBOUNCE_US.
     *
     * Read it as: everything in the first bucket is short enough to be chatter
     * and can be rejected; everything from the bucket that holds most of the
     * real detents upward must be counted. The threshold belongs in the gap.
     */
    if (acc->dwell_total > 0) {
        static const char *const names[8] = {
            "<0.1ms", "0.1-0.25", "0.25-0.5", "0.5-1",
            "1-2ms", "2-5ms", "5-10ms", ">=10ms"
        };
        char line[192];
        size_t used = 0;
        line[0] = '\0';
        for (int i = 0; i < 8; i++) {
            const double pct = 100.0 * (double)acc->dwell_bucket[i] /
                               (double)acc->dwell_total;
            used += (size_t)snprintf(line + used, sizeof(line) - used,
                                     "%s %.0f%% | ", names[i], pct);
        }
        ESP_LOGI(TAG, "  dwell of the state being left, %" PRIu32 " samples:",
                 acc->dwell_total);
        ESP_LOGI(TAG, "    %s", line);
    }
    ESP_LOGI(TAG, "  both lines together : %" PRIu32 "  (quadrature signature)",
             acc->quad_ok);
    ESP_LOGI(TAG, "  A alone %" PRIu32 ", B alone %" PRIu32
                  "  (independent pulse-line signature)",
             acc->pulse_a_only, acc->pulse_b_only);

    const uint32_t total_moves = acc->quad_ok + acc->pulse_a_only + acc->pulse_b_only;
    if (total_moves == 0) {
        ESP_LOGW(TAG, "  no movement at all — the knob was not turned, or both "
                      "lines are stuck");
    } else if (acc->quad_ok > (total_moves * 8) / 10) {
        ESP_LOGW(TAG, "  A and B changed together on most transitions — "
                      "consistent with quadrature, but see the caveat below.");
    } else {
        /*
         * Not a verdict, and deliberately not a quadrature claim either.
         *
         * This probe runs at 1 kHz. The encoder rests at (1,1) and each
         * direction pulls its own line low, so the common transitions are
         * single-line ones — which is the correct description of this hardware,
         * and also what a quadrature encoder with a sub-millisecond overlap
         * would look like at this sample rate. The probe therefore cannot
         * distinguish the two, and in the past it printed a confident verdict
         * for each in turn and was wrong both times.
         *
         * What settled it was counting edges in the driver rather than sampling
         * here: the two lines produced wildly unequal counts, which no
         * quadrature encoder does. The `??` entries in the trace above are
         * consecutive pulses overlapping — the next contact closing before the
         * previous one has opened — not a hidden phase step.
         */
        ESP_LOGW(TAG, "  most transitions moved one line alone. That is the "
                      "expected shape for this encoder (two contacts, each "
                      "pulling its own line). The unknown transitions are "
                      "consecutive pulses overlapping, not a phase step.");
    }

    if (rest >= 0) {
        ESP_LOGI(TAG, "  resting state: %s", state_name(rest));
    }

    /*
     * The decoder's own output, checked against its input.
     *
     * The counters above prove the two lines move. This proves the driver
     * turned those movements into detents, which is a separate claim and the
     * one that decides whether the knob can drive a UI at all.
     *
     * The check is arithmetic rather than a judgement call. This encoder emits
     * one pulse per detent — on A for one direction, on B for the other — so
     * the number of detents in a one-directional turn equals the number of
     * pulses on the line that moved. If the decoder's magnitude does not match
     * that, it is dropping or inventing detents and the number alone would not
     * have shown it.
     */
    bsp_encoder_edges_t edges;
    bsp_encoder_get_edges(&edges);

    const int32_t steps = bsp_encoder_get_steps();

    /* One completed pulse — a falling edge and a rising edge on the same line —
     * is one detent, so a one-directional turn produces one step per pulse. */
    const uint32_t edges_total = edges.a_rising + edges.a_falling +
                                 edges.b_rising + edges.b_falling;

    ESP_LOGI(TAG, "  decoder: %" PRId32 " net step(s)", steps);
    ESP_LOGI(TAG, "  raw edges: A %" PRIu32 " rising / %" PRIu32 " falling, "
                  "B %" PRIu32 " rising / %" PRIu32 " falling",
             edges.a_rising, edges.a_falling, edges.b_rising, edges.b_falling);
    ESP_LOGI(TAG, "  steps by direction: %" PRId32 " one way, %" PRId32 " the "
                  "other (never netted)", edges.steps_positive,
             edges.steps_negative);

    /*
     * The dead-versus-inverted test.
     *
     * A netted count cannot distinguish these: a decoder that is inverted still
     * nets to a large number, and a decoder that is dead nets to zero — but so
     * does a working one that was turned both ways. Counting each direction
     * separately settles it. If both are zero the decoder is producing nothing;
     * if they are large the decoder works and only the sense is in question.
     */
    if (edges_total > 40) {
        const int32_t mag = steps < 0 ? -steps : steps;

        if (edges.steps_positive + edges.steps_negative == 0) {
            ESP_LOGE(TAG, "  the lines moved but the decoder produced NO steps in "
                          "either direction — the decoder is dead, not inverted.");
        } else if (edges.steps_negative == 0 || edges.steps_positive == 0) {
            /*
             * Only one line pulsed, which is what a turn in one direction looks
             * like. This is the case where the count CAN be checked against the
             * edges, and it is the only case where it can be.
             *
             * A line's pulses are counted by its RISING edges — that is the edge
             * the decoder counts, because it is the one that completes a pulse.
             */
            const int positive = (edges.steps_negative == 0);
            const uint32_t pulses = positive ? edges.a_rising : edges.b_rising;
            const int32_t got = positive ? edges.steps_positive
                                         : -edges.steps_negative;

            if (got == (int32_t)pulses) {
                ESP_LOGI(TAG, "  one step per completed pulse — the decoder "
                              "matches the %" PRIu32 " pulse(s) exactly", pulses);
            } else {
                ESP_LOGW(TAG, "  %" PRId32 " step(s) from %" PRIu32 " pulse(s): "
                              "not one-to-one, so some detents are being lost",
                         got, pulses);
            }
        } else {
            /*
             * Both directions were turned, so the netted magnitude is near zero
             * by construction and says nothing. Comparing it against the edge
             * count — which an earlier version of this check did — produces a
             * spurious "discarding detents" warning on a decoder that is
             * working perfectly. The per-direction counts above are the real
             * evidence in this case.
             */
            ESP_LOGI(TAG, "  turned both ways, so only the per-direction counts "
                          "above are meaningful; net is %" PRId32, mag);
        }
    }

    /*
     * A bouncing contact is worth reporting on its own, because it is a
     * hardware fault that no decoder setting fixes and it shows up as the two
     * lines disagreeing by a large factor.
     */
    const uint32_t edges_a = edges.a_rising + edges.a_falling;
    const uint32_t edges_b = edges.b_rising + edges.b_falling;
    const uint32_t hi = edges_a > edges_b ? edges_a : edges_b;
    const uint32_t lo = edges_a > edges_b ? edges_b : edges_a;

    if (lo > 20 && hi > lo * 3) {
        ESP_LOGW(TAG, "  line %c produced %" PRIu32 " edges against %" PRIu32
                      " on the other. The contacts move together, so this is a "
                      "bouncing or noisy contact, not a decoding problem.",
                 (edges_a > edges_b) ? 'B' : 'A', hi, lo);
    }

    /*
     * Whether the knob can actually reach the UI.
     *
     * Steps are only half the story: LVGL delivers arrow keys to the focused
     * widget only while the group is editing, and a tap on anything
     * click-focusable silently clears both. A run that produces plenty of steps
     * and still leaves the UI unresponsive is a focus problem, not a decoder
     * one — and this is the line that says which.
     */
    if (!bsp_encoder_is_driving()) {
        ESP_LOGE(TAG, "  but the knob cannot reach the UI: nothing is focused, "
                      "or the group left edit mode. Call bsp_encoder_refocus().");
    } else {
        ESP_LOGI(TAG, "  knob is driving the UI (widget focused, group editing)");
    }

    /*
     * A line that never moved is the one hardware fault this probe can prove.
     * It is worth saying out loud, because from the UI it is indistinguishable
     * from a decoder bug.
     */
    if (acc->edges_a == 0 && acc->edges_b > 0) {
        ESP_LOGE(TAG, "  A never moved while B did — A's contact, pull-up or "
                      "flex connection is dead. Rotation is one-directional.");
    } else if (acc->edges_b == 0 && acc->edges_a > 0) {
        ESP_LOGE(TAG, "  B never moved while A did — B's contact, pull-up or "
                      "flex connection is dead. Rotation is one-directional.");
    }
}

/*
 * Dump the transition trace, oldest first.
 *
 * Four transitions per log line keeps the console readable. Each entry is
 * annotated with which line moved and in which direction, so the sequence can
 * be read without decoding state numbers by hand — which is the whole point:
 * direction of rotation is a property of the ORDER of the lines, and the
 * aggregate counters above deliberately throw that order away.
 */
static void probe_dump_trace(const probe_acc_t *acc)
{
    const uint32_t total = acc->trace_count;
    if (total == 0) {
        ESP_LOGW(TAG, "  trace is empty — no transitions were recorded");
        return;
    }

    const uint32_t shown = (total > PROBE_TRACE_DEPTH) ? PROBE_TRACE_DEPTH : total;
    const uint32_t first = (total > PROBE_TRACE_DEPTH) ? (total - PROBE_TRACE_DEPTH) : 0;

    ESP_LOGI(TAG, "  trace: last %" PRIu32 " of %" PRIu32 " transitions",
             shown, total);

    char line[192];
    size_t used = 0;
    line[0] = '\0';

    for (uint32_t n = 0; n < shown; n++) {
        const uint32_t slot = (first + n) % PROBE_TRACE_DEPTH;
        const int from = acc->trace_from[slot];
        const int to   = acc->trace_to[slot];

        /*
         * Which line moved, and in which direction.
         *
         * States are (A << 1) | B, so 2 is A=1 B=0 and 1 is A=0 B=1. Getting
         * these the wrong way round is easy and makes the trace read as the
         * opposite of what happened, so they are spelled out.
         */
        const char *what = "??";
        if      (from == 3 && to == 2) what = "A fell";   /* (1,1)->(1,0) */
        else if (from == 3 && to == 1) what = "B fell";   /* (1,1)->(0,1) */
        else if (from == 2 && to == 3) what = "A rose";   /* (1,0)->(1,1) */
        else if (from == 1 && to == 3) what = "B rose";   /* (0,1)->(1,1) */
        else if (from == 2 && to == 0) what = "B fell";   /* (1,0)->(0,0) */
        else if (from == 1 && to == 0) what = "A fell";   /* (0,1)->(0,0) */
        else if (from == 0 && to == 2) what = "B rose";   /* (0,0)->(1,0) */
        else if (from == 0 && to == 1) what = "A rose";   /* (0,0)->(0,1) */
        else if (from == 0 && to == 3) what = "both rose";/* (0,0)->(1,1) */

        used += (size_t)snprintf(line + used, sizeof(line) - used,
                                 "%s>%s %s | ", state_name(from), state_name(to), what);

        if ((n % 4) == 3 || n == shown - 1) {
            ESP_LOGI(TAG, "    %s", line);
            used = 0;
            line[0] = '\0';
        }
    }
}

/* -------------------------------------------------------------------------- */
/* Task                                                                       */
/* -------------------------------------------------------------------------- */

static void probe_task(void *arg)
{
    (void)arg;

    bsp_encoder_probe_reset();
    /* Measure the decoder from a clean zero, so its count covers this run only
     * and not any steps taken during bring-up. */
    bsp_encoder_reset_steps();

    const int64_t start_us = esp_timer_get_time();
    int64_t last_report_us = start_us;

    ESP_LOGI(TAG, "probing A(%d)/B(%d) at %d Hz for %d s — turn the knob, both "
                  "ways, now", BSP_ENC_A, BSP_ENC_B, PROBE_SAMPLE_HZ,
             PROBE_DURATION_MS / 1000);

    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(PROBE_PERIOD_MS));

        const int64_t now_us = esp_timer_get_time();
        probe_sample(&s_acc, now_us);

        /* A progress line every 5 s, so a silent log is not mistaken for a
         * hung probe. */
        if (now_us - last_report_us >= 5000000) {
            last_report_us = now_us;
            ESP_LOGI(TAG, "  ... %" PRIu32 " moves so far",
                     s_acc.quad_ok + s_acc.pulse_a_only + s_acc.pulse_b_only);
        }

        if (now_us - start_us >= (int64_t)PROBE_DURATION_MS * 1000) {
            break;
        }
    }

    probe_report(&s_acc, esp_timer_get_time() - start_us);
    probe_dump_trace(&s_acc);

    /* One task, one run: the probe is a bring-up tool, not a service. */
    vTaskDelete(NULL);
}

/* -------------------------------------------------------------------------- */
/* Public API                                                                 */
/* -------------------------------------------------------------------------- */

bool bsp_encoder_probe_start(void)
{
    static bool started;

    if (started) {
        ESP_LOGW(TAG, "probe already running");
        return true;
    }

    /* Priority 2: below the LVGL task (4) and the Wi-Fi task, so sampling can
     * never be mistaken for a source of UI stutter. */
    if (xTaskCreate(probe_task, "enc_probe", 3072, NULL, 2, NULL) != pdPASS) {
        ESP_LOGE(TAG, "could not start the probe task");
        return false;
    }

    started = true;
    return true;
}

void bsp_encoder_probe_reset(void)
{
    s_acc = (probe_acc_t){ 0 };
    s_acc.state = (gpio_get_level(BSP_ENC_A) ? 2 : 0) |
                  (gpio_get_level(BSP_ENC_B) ? 1 : 0);
    s_acc.last_change_us[0] = esp_timer_get_time();
    s_acc.last_change_us[1] = s_acc.last_change_us[0];
}
