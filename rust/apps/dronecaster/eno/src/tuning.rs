//! Tunable constants and CPU-budget switches.
//!
//! The CPU budget on the board (ESP32-S3 core 1, block of 128 frames at 48 kHz = 2666 us) was
//! measured on 2026-11-13 with firmware `aace293-dirty` (reverb 4+2, Klank enabled, `EnvPerc`
//! attack on curve -4, `PAD_CTRL_STRIDE` 4), steady state with an almost idle UI: core 1 mean
//! ~91.0-91.2 %, max 91-93 % (2430-2470 us), write errors 0, short writes 0; the worst block
//! per the profiler 2380-2405 us (headroom ~260 us up to 2666). Profile per section (us per
//! block): control ~97, pad 790, moog 462, klank 262, piano 160, perc 151-281 (PolyPerc is
//! skipped while its envelope is zero; budget for 281), reverb 225, final 81; the sum is ~2360
//! at perc 281. The target of <= ~86 % was not reached (about 5 percentage points higher;
//! Starlids sits at 87 %); the deviation was accepted by the owner and the sound confirmed by
//! ear. Switching Starlids -> Eno while the menu animation runs (30 flushes in 5 s): max
//! 100.5-101 % (2680-2693 us, above 2666), mean up to 91.4 %; write errors 0, but a block
//! exceeded the budget, so a click is possible (not checked by ear). Source: the layered
//! platform design spec, 2026-11-12.

/// Control period in samples (a unit of 32 frames, not seconds). Plane changes, LFOs, cutoffs and
/// pan update once per period at fixed absolute sample positions, which is what makes the output
/// independent of the caller's block size. A smaller value makes modulation smoother and costs
/// more CPU in the `control` section; a larger one is cheaper but steppier.
pub(crate) const CTRL: usize = 32;
/// Output trim, a linear gain applied together with `amp`: the layers peak at ~1.56 at amp 1,
/// which the i16 conversion would hard-clip.
pub(crate) const OUT_TRIM: f32 = 0.6;
/// CPU-budget switch (flipped by hand after a measurement): reverb on the mono sum with the same
/// wet on both channels. Note: the current `ReverbLite` has no mono tick, so this saves no CPU
/// until it gets one. When `true` the stereo image of the reverb tail collapses to mono.
pub(crate) const REVERB_SINGLE_CHANNEL: bool = false;
/// CPU-budget switch: `false` removes the Klank layer (four resonators per channel, `klank`
/// section, ~262 us per block on the board) and its control update.
pub(crate) const KLANK_ENABLED: bool = true;
/// Reverb dry/wet mix, 0..1 (0 = dry). Higher means a wetter, more washed-out sound; CPU is the
/// same at any value.
pub(crate) const REVERB_MIX: f32 = 0.45;
/// Klank resonator decay in seconds to -60 dB. Longer values ring longer after each chord change
/// and make the layer louder and more tonal; CPU is unaffected.
pub(crate) const KLANK_DECAY: f32 = 1.0;
/// Klank excitation: linear gain on the pink noise fed to the resonators. Together with
/// `KLANK_GAIN` it sets how loud the shimmer is; CPU is unaffected.
pub(crate) const KLANK_EXCITE: f32 = 0.004;
/// Linear gain of the Klank output when added to the pad, before the final filters. Higher is a
/// louder ringing layer; CPU is unaffected.
pub(crate) const KLANK_GAIN: f32 = 0.55;
/// Silence before the intro fade starts, in seconds from construction. It only shapes the start.
pub(crate) const INTRO_DELAY_S: f32 = 0.5;
/// Length of the linear fade-in after `INTRO_DELAY_S`, in seconds. Longer is a gentler start.
pub(crate) const INTRO_RAMP_S: f32 = 3.0;
/// Corner frequency in Hz of the final high-pass (Butterworth). Higher thins out the low end of
/// the pad; the sub sines sit below it at low `hz`.
pub(crate) const HPF_HZ: f32 = 120.0;
/// Number of pad voices (each a saw, a modulated lowpass, a short delay and a pan position).
/// Pad cost scales linearly with it (the `pad` section is the largest, ~790 us per block).
pub(crate) const VOICES: usize = 8;
/// Each pad voice gets its control update every this many control steps (staggered across voices).
/// Higher saves `control` time (about VOICES / this many voice updates per step) at the price of
/// steppier cutoff, delay and pan modulation; the voice LFOs run at 0.03-0.1 Hz so 4 is inaudible.
pub(crate) const PAD_CTRL_STRIDE: usize = 4;
/// Voices processed together in the hot loops. The FPU chains of one voice are serial on the
/// in-order core, so interleaving four independent voices hides the latency. Must be 4: voice `i`
/// uses the shared saw `i % 4`.
pub(crate) const GROUP: usize = 4;
/// Number of voice groups (`VOICES / GROUP`, two with the defaults).
pub(crate) const GROUPS: usize = VOICES / GROUP;
const _: () = assert!(VOICES.is_multiple_of(GROUP) && GROUP == 4, "voice i uses shared saw i % 4");
/// Biquad Q of 1/sqrt(2): the maximally flat (Butterworth) response, no resonance peak.
pub(crate) const Q_BUTTERWORTH: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// Size in seconds of each pad voice's delay line. The largest delay reached is
/// (0.02 + 0.01) / 15 s = 2 ms; the extra margin only costs a few bytes.
pub(crate) const MAX_DELAY_S: f32 = 0.004;
/// log2(5): the pad lowpass cutoff sweeps `hz * 2^(LOG2_5 * (lfo + 1) / 2)`, i.e. from `hz` up to
/// `5 * hz`. A larger value opens the filter further and makes the pad brighter.
pub(crate) const LOG2_5: f32 = 2.321_928;
/// log2(30): the Moog cutoff wanders over `2 * hz * 2^(LOG2_30 * (x + 1) / 2)`, i.e. from `2 * hz`
/// up to `60 * hz`. A larger value widens the sweep.
pub(crate) const LOG2_30: f32 = 4.906_890_6;
/// Rate in Hz of the stepped random pan position (`LFNoise0.kr(1/3)` in the original): a new
/// target about every 3 s, smoothed by a lag. Higher moves the voices around faster.
pub(crate) const PAN_RATE: f32 = 1.0 / 3.0;
/// Size in seconds of each piano comb delay line. The lowest string pitch is 40 Hz, i.e. 25 ms,
/// so this is the longest delay needed; it sets the buffer size, not the sound.
pub(crate) const PIANO_MAX_DELAY_S: f32 = 0.025;
/// The piano string loop is skipped this long (seconds) after the last note: 8 s of a 6 s (-60 dB)
/// decay is below -80 dB, so the skipped tail is inaudible. Counted in control steps (block-size
/// independent). Saves the `piano` section (~160 us per block) between slow notes; the peak
/// block time does not change.
pub(crate) const PIANO_LIVE_S: f32 = 8.0;
