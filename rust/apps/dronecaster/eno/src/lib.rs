//! Eno: port of dronecaster's Eno.scd (milestone 1: pad, sub sines, tanh, Moog).
//!
//! Control-rate parts (plane changes, LFOs, cutoffs, pan) update every `CTRL`
//! samples at fixed absolute positions, so the output does not depend on the
//! caller's block size.

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
