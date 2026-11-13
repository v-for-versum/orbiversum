//! Common interface of drone voices.
//!
//! A voice owns its DSP state and renders stereo audio, one `[left, right]` frame per
//! element, into a caller buffer. The firmware calls `set_sample_rate` at (re)configuration time,
//! `set_params` when the control values change and `process` once per audio block.
//!
//! Cost class: the trait itself adds one dynamic call per block; the cost is the implementor's.
//! Allocation: `set_params` and `process` must not allocate; only `set_sample_rate` may (it is
//! reconfiguration, not real time).
//!
//! ```
//! use orbiversum_core::{osc::SinOsc, pan::pan2, voice::Voice};
//!
//! struct Sine {
//!     osc: SinOsc,
//!     hz: f32,
//!     amp: f32,
//! }
//!
//! impl Voice for Sine {
//!     fn set_sample_rate(&mut self, sr: f32) {
//!         self.osc = SinOsc::new(sr, 0.0);
//!     }
//!     fn set_params(&mut self, hz: f32, amp: f32) {
//!         self.hz = hz;
//!         self.amp = amp;
//!     }
//!     fn process(&mut self, out: &mut [[f32; 2]]) {
//!         for frame in out.iter_mut() {
//!             *frame = pan2(self.osc.tick(self.hz) * self.amp, 0.0);
//!         }
//!     }
//! }
//!
//! let mut v = Sine { osc: SinOsc::new(48_000.0, 0.0), hz: 440.0, amp: 0.5 };
//! v.set_sample_rate(48_000.0);
//! v.set_params(440.0, 0.5);
//! let mut block = [[0.0f32; 2]; 64];
//! v.process(&mut block);
//! assert!(block.iter().all(|f| f[0].abs() <= 0.5 && (f[0] - f[1]).abs() < 1e-6));
//! ```

/// A drone voice: stereo output, two control parameters.
///
/// Implementors must keep `set_params` and `process` free of allocation so they can run
/// on the audio thread; only `set_sample_rate` may allocate.
pub trait Voice {
    /// Changes the sample rate. May allocate (reconfiguration, not real time).
    fn set_sample_rate(&mut self, sr: f32);
    /// Real-time safe: no allocation.
    fn set_params(&mut self, hz: f32, amp: f32);
    /// Overwrites `out` with stereo frames. No allocation.
    fn process(&mut self, out: &mut [[f32; 2]]);
}
