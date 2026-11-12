//! `os`: applet and board abstractions plus the audio/UI dispatcher.
#![cfg_attr(not(test), no_std)]
extern crate alloc;

pub mod applet;
pub mod board;
pub mod dispatcher;
pub mod params;

pub use applet::{Applet, AppletInfo, Dirty, InputEvent, MAX_DIRTY};
pub use board::{AudioError, AudioOut, Board, Canvas, Display, Surface, RenderFn};
pub use dispatcher::{AudioPath, Dispatcher, DEFAULT_SAMPLE_RATE};
pub use params::Params;
