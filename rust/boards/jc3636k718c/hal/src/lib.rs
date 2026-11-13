//! Board support for the JC3636K718C (ESP32-S3) on top of the C BSP.
//!
//! * `convert`, `display`, `input`, `menu`: host-testable logic (sample
//!   conversion, framebuffer and dirty-region flush, input filtering, applet menu).
//! * `audio`, `jc`, `ffi` (ESP-IDF target only): the I2S audio task, `JcBoard`
//!   (the `orbiversum_os::Board` implementation) and the raw C BSP bindings.
extern crate alloc;

pub mod convert;
pub mod display;
pub mod input;
pub mod menu;

#[cfg(target_os = "espidf")]
mod ffi;
#[cfg(target_os = "espidf")]
pub mod audio;
#[cfg(target_os = "espidf")]
mod jc;
#[cfg(target_os = "espidf")]
pub use jc::{BoardError, BspPanel, JcBoard, JcDisplay};
