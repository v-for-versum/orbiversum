//! Common interface of drone voices.

pub trait Voice {
    /// Changes the sample rate. May allocate (reconfiguration, not real time).
    fn set_sample_rate(&mut self, sr: f32);
    /// Real-time safe: no allocation.
    fn set_params(&mut self, hz: f32, amp: f32);
    /// Overwrites `out` with stereo frames. No allocation.
    fn process(&mut self, out: &mut [[f32; 2]]);
}
