/*
 * Encoder line probe — a bring-up tool, not part of the runtime path.
 *
 * Samples the knob's two lines and reports what they actually do, to settle
 * whether this encoder is quadrature or two independent pulse lines. See
 * bsp_encoder_probe.c for why that matters and how to enable it.
 *
 * Both calls are safe to ignore: the whole probe compiles to nothing unless
 * CONFIG_BSP_ENCODER_PROBE is set, so application code may call it
 * unconditionally.
 */
#pragma once

#include <stdbool.h>

#include "sdkconfig.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Start sampling. Returns false if the task could not be created. Call after
 * bsp_encoder_init(), which configures the pins as inputs.
 *
 * The probe runs for a fixed window and then deletes itself; there is nothing
 * to stop.
 */
bool bsp_encoder_probe_start(void);

/* Zero the counters and re-read the resting state. Useful when a probe run is
 * preceded by other bring-up noise. */
void bsp_encoder_probe_reset(void);

#ifdef __cplusplus
}
#endif
