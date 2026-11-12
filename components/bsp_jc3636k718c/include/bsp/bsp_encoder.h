/*
 * Rotary encoder, bound to LVGL as an encoder input device.
 *
 * Call bsp_encoder_init() after bsp_display_init().
 *
 * WHY THIS DOES NOT USE espressif/knob
 *   Because this encoder is NOT quadrature, and espressif/knob only decodes
 *   quadrature. That is a measurement, not an assumption — see below.
 *
 *   A quadrature encoder has two contacts 90° out of phase, so one line always
 *   leads the other and the two lines change together in a fixed order. This
 *   knob does not work that way. Its two lines are independent: each direction
 *   of rotation briefly pulls its OWN line low and leaves the other alone.
 *
 *   MEASURED, with the line probe in bsp_encoder_probe.c (20 s of turning,
 *   both directions, ~20 000 samples):
 *
 *       A moved 262 times, B moved 286 times
 *       A alone 262, B alone 286
 *       both lines together: 18  (3%, and that is sampling coincidence at
 *                                 1 kHz — not phase overlap)
 *       A=0 B=0: never observed
 *       resting state: A=1 B=1
 *
 *   So there is no phase relationship to decode. espressif/knob looks for
 *   transitions between A and B and finds none, which is why the knob appeared
 *   dead when it was tried. bsp_encoder.c decodes the levels directly instead:
 *   the falling edge of A is one step one way, the falling edge of B is one
 *   step the other.
 *
 *   Enable Knob BSP -> Encoder -> "Run the encoder line probe at boot" to
 *   re-measure this on your own board; the numbers above are one board's, and
 *   a different revision could differ.
 *
 *   A second, unrelated fault also made the knob look dead and is worth knowing
 *   about: LVGL only sends arrow keys to the focused widget while the input
 *   group is in edit mode, and edit mode is normally toggled by the encoder's
 *   push button — which this board does not have. The BSP forces edit mode
 *   permanently (see below).
 *
 * THE CONSTRAINT THAT SHAPES EVERY UI ON THIS BOARD
 *   There is no push button, so LVGL never receives an ENTER. Two consequences
 *   follow, and both bite silently:
 *
 *   1. The BSP forces the input group into edit mode permanently. LVGL only
 *      sends arrow keys to the focused widget while editing; otherwise a
 *      rotation means "move focus". Edit mode is normally toggled by the
 *      encoder's button, which does not exist here.
 *
 *   2. Even so, an encoder in edit mode can only change the value of the
 *      focused widget — it cannot activate anything. So put exactly ONE
 *      focusable widget on screen at a time, and drive everything else from
 *      touch.
 *
 *   For touch-only controls — page arrows, tabs, anything tappable that should
 *   not be rotatable — create an lv_obj rather than an lv_button. The group
 *   builder in the application UI code only collects interactive classes, so a plain
 *   clickable lv_obj stays out of the knob's path while remaining tappable.
 */
#pragma once

#include <stdbool.h>
#include "esp_err.h"
#include "iot_button.h"
#include "lvgl.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Create the knob driver, the optional button, the LVGL encoder indev and a
 * default input group.
 */
esp_err_t bsp_encoder_init(void);

esp_err_t bsp_encoder_deinit(void);

bool bsp_encoder_is_ready(void);

/* LVGL encoder input device. */
lv_indev_t *bsp_encoder_get_indev(void);

/*
 * The input group the encoder navigates. Add every focusable widget to it:
 *
 *     lv_group_add_obj(bsp_encoder_get_group(), my_button);
 *
 * Must be called under the LVGL lock.
 */
lv_group_t *bsp_encoder_get_group(void);

/*
 * Replace the default group, for applications that switch groups per screen.
 * Takes ownership of the group you pass; the default one is deleted.
 */
esp_err_t bsp_encoder_set_group(lv_group_t *group);

/*
 * Put the focus back on the widget the knob drives, and restore edit mode.
 *
 * Call this after anything that could have moved the focus or cleared edit
 * mode: loading a screen, creating or deleting widgets, or handling a touch
 * event that changed what is on screen.
 *
 * It is deliberately safe to call at any time — on an empty group, with nothing
 * focused, or when the focused widget has just been deleted — so the simplest
 * correct pattern is to call it unconditionally at the end of any such handler
 * rather than to work out whether it was needed.
 *
 * Must be called under the LVGL lock.
 */
void bsp_encoder_refocus(void);

/*
 * True when the knob can actually change the focused widget's value: something
 * is focused AND the group is editing.
 *
 * Both halves are needed. With edit mode off a rotation only moves focus, which
 * on a one-widget screen does nothing at all — so "focused" alone would report
 * a healthy knob on a dead UI.
 */
bool bsp_encoder_is_driving(void);

/*
 * Enforce the focus policy, and report whether anything needed repairing.
 *
 * The policy is: with exactly one focusable widget on the screen the knob
 * always drives it and a tap cannot take it away; with several, a tap chooses
 * which one the knob drives. Only the first half needs enforcing, so that is
 * all this does — it pins the focus when there is one candidate, and otherwise
 * leaves stock LVGL behaviour alone.
 *
 * Called automatically on every input read, so application code rarely needs
 * it. Call it directly after a screen change to fail fast rather than on the
 * next rotation, and check the return value: true means the knob had been
 * disconnected from the UI.
 */
bool bsp_encoder_enforce_focus(void);

/*
 * How many widgets the knob can reach on the current screen.
 *
 * The focus policy keys on this: one candidate means the target is not in
 * question, so the focus is pinned; two or more means the user chooses, so
 * stock LVGL behaviour is left alone.
 *
 * The focus ring keys on it too, and for the same reason — with a single
 * candidate there is nothing to disambiguate, so a ring would only be visual
 * noise. Exposing the count rather than letting the UI re-derive it keeps the
 * two from disagreeing about what counts as a candidate.
 */
uint32_t bsp_encoder_focusable_count(void);

/*
 * Push button handle, or NULL when CONFIG_BSP_ENCODER_USE_BUTTON is off or the
 * button failed to initialise.
 *
 * esp_lvgl_port already uses it for ENTER. Extra gestures (long press, double
 * click) can be added on top — the button component supports several
 * callbacks per event, unlike the knob.
 */
button_handle_t bsp_encoder_get_button(void);

/*
 * Net detents decoded since boot: positive one way, negative the other.
 *
 * This is the decoder's own output, after debounce — the same number the LVGL
 * input device reports. It exists so the line probe can show that the edges it
 * measured were turned into steps, which is a different claim from "the lines
 * move" and the one that decides whether the knob is usable.
 *
 * Resettable with bsp_encoder_reset_steps().
 */
int32_t bsp_encoder_get_steps(void);

/*
 * Raw edge counts per line, counted by the ISR before the decoder interprets
 * them.
 *
 * These are the input to the decoder, so comparing them against the step count
 * is what proves the decoder is right rather than merely plausible: this
 * encoder produces one pulse on one of the two lines per detent, so
 *
 *     |steps| == a_rising + b_rising   (or the falling pair)
 *
 * must hold for a turn in one direction. A mismatch means the decoder is
 * dropping or inventing detents.
 */
typedef struct {
    uint32_t a_rising;
    uint32_t a_falling;
    uint32_t b_rising;
    uint32_t b_falling;
    int32_t  steps_positive;   /*!< Sum of steps emitted with a + sign */
    int32_t  steps_negative;   /*!< Sum of steps emitted with a - sign */
} bsp_encoder_edges_t;

void bsp_encoder_get_edges(bsp_encoder_edges_t *out);

/* Zero the decoded step count and the edge counters. Does not touch the GPIO
 * configuration. */
void bsp_encoder_reset_steps(void);

#ifdef __cplusplus
}
#endif
