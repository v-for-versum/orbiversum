//! Lock-free parameters shared between the UI and audio threads.
//!
//! [`Params`] is the only channel between an applet's UI half and its audio
//! half: the UI thread stores, the audio thread loads, and neither ever waits.
use core::sync::atomic::{AtomicU32, Ordering};

/// `f32` values stored as bits in atomics: no locks on the audio path.
///
/// All accesses use `Ordering::Relaxed`: each value is independent, so there is
/// no ordering between `hz` and `amp` (a reader may see a new `hz` with the
/// old `amp` for one block) and no other memory is published through them.
/// One `Params` is shared by every applet (see `VoiceApplet`).
#[derive(Debug)]
pub struct Params {
    /// Frequency in Hz, as `f32` bits. Use [`Params::hz`] / [`Params::set_hz`].
    pub hz: AtomicU32,
    /// Amplitude, as `f32` bits. Use [`Params::amp`] / [`Params::set_amp`].
    pub amp: AtomicU32,
}

impl Params {
    /// Creates parameters with the given frequency (Hz) and amplitude.
    pub fn new(hz: f32, amp: f32) -> Params {
        Params { hz: AtomicU32::new(hz.to_bits()), amp: AtomicU32::new(amp.to_bits()) }
    }
    /// Current frequency in Hz.
    pub fn hz(&self) -> f32 {
        f32::from_bits(self.hz.load(Ordering::Relaxed))
    }
    /// Sets the frequency in Hz.
    pub fn set_hz(&self, v: f32) {
        self.hz.store(v.to_bits(), Ordering::Relaxed);
    }
    /// Current amplitude.
    pub fn amp(&self) -> f32 {
        f32::from_bits(self.amp.load(Ordering::Relaxed))
    }
    /// Sets the amplitude.
    pub fn set_amp(&self, v: f32) {
        self.amp.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// 110 Hz at amplitude 0.5.
impl Default for Params {
    fn default() -> Params {
        Params::new(110.0, 0.5)
    }
}
