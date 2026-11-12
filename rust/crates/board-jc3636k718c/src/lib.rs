//! Board support for the JC3636K718C (ESP32-S3) on top of the C BSP.
pub mod convert;

#[cfg(target_os = "espidf")]
mod ffi;
#[cfg(target_os = "espidf")]
pub mod audio;
