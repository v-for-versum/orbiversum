//! Lock-free parameters shared between the UI and audio threads.
use core::sync::atomic::{AtomicU32, Ordering};

/// `f32` values stored as bits in atomics: no locks on the audio path.
#[derive(Debug)]
pub struct Params {
    pub hz: AtomicU32,
    pub amp: AtomicU32,
}

impl Params {
    pub fn new(hz: f32, amp: f32) -> Params {
        Params { hz: AtomicU32::new(hz.to_bits()), amp: AtomicU32::new(amp.to_bits()) }
    }
    pub fn hz(&self) -> f32 {
        f32::from_bits(self.hz.load(Ordering::Relaxed))
    }
    pub fn set_hz(&self, v: f32) {
        self.hz.store(v.to_bits(), Ordering::Relaxed);
    }
    pub fn amp(&self) -> f32 {
        f32::from_bits(self.amp.load(Ordering::Relaxed))
    }
    pub fn set_amp(&self, v: f32) {
        self.amp.store(v.to_bits(), Ordering::Relaxed);
    }
}

impl Default for Params {
    fn default() -> Params {
        Params::new(110.0, 0.5)
    }
}
