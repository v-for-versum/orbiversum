//! `orbiversum-os`: applet and board abstractions plus the audio/UI dispatcher.
//!
//! An applet is split in two halves. The audio half ([`Applet`]) is owned by
//! the [`Dispatcher`] and runs on the realtime audio thread through
//! [`AudioPath`]; the UI half ([`AppletUi`]) stays on the UI thread. The two
//! halves talk only through lock-free atomics ([`Params`]). A [`Board`]
//! supplies input, a [`Display`] and an [`AudioOut`].
//!
//! The crate is `no_std` (with `alloc`) outside of tests.
#![cfg_attr(not(test), no_std)]
#![warn(missing_docs)]
extern crate alloc;

pub mod applet;
pub mod board;
pub mod dispatcher;
pub mod param_ui;
pub mod params;
pub mod voice_applet;
pub mod zones;

pub use applet::{Applet, AppletInfo, AppletUi, Dirty, InputEvent, MAX_DIRTY};
pub use board::{AudioError, AudioOut, Board, Canvas, Display, Surface, RenderFn};
pub use dispatcher::{AudioPath, Dispatcher, DEFAULT_SAMPLE_RATE};
pub use param_ui::{ParamUi, AMP_STEP, MAX_HZ, MIN_HZ};
pub use params::Params;
pub use voice_applet::VoiceApplet;
pub use zones::{Edge, Glyph, TapZone, GLYPH};
