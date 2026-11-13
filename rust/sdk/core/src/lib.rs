//! Allocation-free-at-runtime DSP building blocks for the Orbiversum drone voices.
//!
//! The crate is `#![no_std]` (it uses `alloc` for delay buffers) and is shared by the
//! firmware (ESP32-S3) and the host tests. Every block is a small struct with a `tick`
//! (or `next`/`pick`) method that produces one sample or one control value.
//!
//! # Conventions
//!
//! * Sample rates `sr` are in Hz, times are in seconds, frequencies in Hz, gains linear.
//! * Allocation happens only in constructors of the blocks that own a buffer
//!   ([`delay::DelayC`], [`comb::DelayL`], [`comb::CombL`], [`reverb::ReverbLite`]);
//!   `tick` never allocates.
//! * Blocks that name an SC (SuperCollider) UGen say so; they follow that UGen's
//!   behaviour in the way described in their own docs, not bit for bit.
//! * `set_*`/`set` methods are control rate: they may call `sin`/`exp`/reciprocal helpers
//!   and are meant to run when a parameter changes, not necessarily every sample.
//! * [`fast`] replaces libm functions with f32-only approximations because the ESP32-S3
//!   has no hardware divide and no double-precision FPU.
//!
//! # Blocks
//!
//! | module | contents | rate |
//! |---|---|---|
//! | [`fast`] | f32 `recip`, `tanh`, `sin`, `cos`, `exp`, `exp2`, `floor` approximations | audio (stateless) |
//! | [`osc`] | `SawDpw`, `Pulse`, `SinOsc`, `LfTri` | audio |
//! | [`biquad`] | RBJ lowpass/highpass `Biquad` | audio |
//! | [`resonant`] | `RLpf`, `Ringz` | audio |
//! | [`moog`] | `MoogLadder`, `MoogStereo` | audio |
//! | [`comb`] | `DelayL` (linear delay), `CombL` | audio |
//! | [`delay`] | `DelayC` (cubic delay) | audio |
//! | [`reverb`] | `ReverbLite` (Freeverb) | audio |
//! | [`mod@env`] | `Decay2` (audio), `EnvPerc` (control) | mixed |
//! | [`lfo`] | `Noise0`, `Noise1`, `VarLagSine` | slow modulators, `hz / sr` per call |
//! | [`noise`] | `Noise2`, `PinkNoise` | audio |
//! | [`rng`] | `Rng` (xorshift64*) | any |
//! | [`pan`] | `pan2` | audio |
//! | [`seq`] | `Dust`, `Impulse`, `Dseq`, `Dxrand`, `TChoose`, `TDelay` | control |
//! | [`prof`] | per-section cycle timing | per block |
//! | [`voice`] | the `Voice` trait | per block |
#![warn(missing_docs)]
#![no_std]
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
extern crate alloc;

pub mod biquad;
pub mod comb;
pub mod delay;
pub mod fast;
pub mod env;
pub mod lfo;
pub mod moog;
pub mod noise;
pub mod osc;
pub mod pan;
pub mod prof;
pub mod resonant;
pub mod reverb;
pub mod rng;
pub mod seq;
pub mod voice;
