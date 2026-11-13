//! Eno: port of dronecaster's `Eno.scd` for a fixed-size, allocation-free real-time voice.
//!
//! [`Eno`] implements `orbiversum_core::voice::Voice`: `set_params(hz, amp)` sets the base pitch
//! and the level, `process` renders stereo frames. Everything is allocated in the constructor.
//!
//! # Signal flow
//!
//! One `process` call is cut into segments that end at control-step boundaries. Per segment:
//!
//! 1. `P_CONTROL`: once per `CTRL` = 32 samples (at fixed absolute sample
//!    positions, so the output does not depend on the caller's block size) the control step
//!    runs: chord "plane" changes (`Dust` trigger, envelope, delayed pick of one of four
//!    planes), saw pitches, pad voice modulation, the sub-sine amplitude LFOs, the Moog cutoff,
//!    the Klank resonator tuning, and the note layer (note rate, Airports-style note sequence,
//!    piano and PolyPerc pitches, layer crossfade).
//! 2. `P_PAD`: two sub sines (`hz / 4`, `hz / 2`) plus four shared saws, tuned to the current
//!    plane. Eight pad voices run as two groups of four; voice `i` listens to saw `i % 4`, then
//!    has its own modulated lowpass, a short modulated delay and a pan position. The voices
//!    are summed to stereo with the plane envelope. A group of four is processed together so the
//!    serial FPU chains overlap on the in-order core.
//! 3. `P_MOOG`: a stereo Moog ladder with a slowly wandering cutoff (no outer saturator, the
//!    ladder saturates its input itself).
//! 4. `P_KLANK`: four `Ringz` resonators per channel, tuned to the current plane and excited by
//!    pink noise (can be switched off in `tuning.rs`).
//! 5. `P_PIANO`: on every note a noise burst goes through two slightly detuned combs (the string),
//!    a resonant lowpass and a highpass. The string loop is skipped `PIANO_LIVE_S` = 8 s after
//!    the last note (the 6 s -60 dB decay is then below -80 dB), which saves CPU between slow
//!    notes.
//! 6. `P_PERC`: PolyPerc, a pulse through a Moog ladder with a percussive envelope; skipped
//!    while the envelope is zero. Piano and PolyPerc are mixed with a slow crossfade and added
//!    to both channels.
//! 7. `P_FINAL`: lowpass at `8 * hz`, highpass at `HPF_HZ`, and the intro fade-in.
//! 8. `P_REVERB`: a stereo `ReverbLite` (Freeverb: per channel 4 combs and 2 allpasses, i.e.
//!    "4+2", room 2.0 clamped to 1.0, damping 0.5, dry/wet `REVERB_MIX`), then the output gain
//!    `amp * OUT_TRIM`.
//!
//! The `P_*` constants index the profiler sections of `orbiversum_core::prof`; [`ENO_NAMES`]
//! gives their names.
//!
//! `PAD_CTRL_STRIDE` = 4: a pad voice modulates at 0.03-0.1 Hz (noise 5-10 Hz), so only one in
//! four voices gets its control update per step, round-robin, and every voice is refreshed every
//! fourth step instead of every one.
//!
//! # CPU budget
//!
//! On the board (ESP32-S3 core 1, block of 128 frames at 48 kHz = 2666 us; fw `aace293-dirty`:
//! reverb 4+2, Klank enabled, `EnvPerc` attack on curve -4, `PAD_CTRL_STRIDE` 4), steady state
//! with an almost idle UI: core 1 mean ~91.0-91.2 %, max 91-93 % (2430-2470 us), worst block
//! per the profiler 2380-2405 us. Profile per section (us per block): control ~97, pad 790,
//! moog 462, klank 262, piano 160, perc 151-281, reverb 225, final 81. Switching Starlids to
//! Eno while the menu animates exceeded the block budget once (max 100.5-101 %). See the
//! constants in the private `tuning` module for the switches that trade sound for CPU.
//!
//! # Determinism
//!
//! The output is a pure function of `(seed, sample rate, hz, amp)` and the sequence of samples
//! rendered; the block size does not matter. Random generators are drawn in a fixed order
//! (pad first, then the note layer), so adding to a later layer does not change an earlier one.

#![warn(missing_docs)]
#![no_std]
extern crate alloc;

mod control;
mod engine;
mod music;
mod pad;
mod profile;
mod tuning;
mod voice;

pub use engine::Eno;
pub use profile::{ENO_NAMES, P_CONTROL, P_FINAL, P_KLANK, P_MOOG, P_PAD, P_PERC, P_PIANO, P_REVERB};
