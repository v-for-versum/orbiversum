//! Plain sine drone.

use crate::voice::Voice;
use blocks::osc::SinOsc;

pub struct Sine {
    sr: f32,
    hz: f32,
    amp: f32,
    osc: SinOsc,
}

impl Sine {
    pub fn new(sr: f32) -> Sine {
        Sine { sr, hz: 440.0, amp: 0.0, osc: SinOsc::new(sr, 0.0) }
    }
}

impl Voice for Sine {
    fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        self.osc = SinOsc::new(sr, 0.0);
    }

    fn set_params(&mut self, hz: f32, amp: f32) {
        self.hz = if hz.is_finite() { hz.clamp(0.0, self.sr * 0.45) } else { 0.0 };
        self.amp = if amp.is_finite() { amp.clamp(0.0, 1.0) } else { 0.0 };
    }

    fn process(&mut self, out: &mut [[f32; 2]]) {
        for f in out.iter_mut() {
            let v = self.osc.tick(self.hz) * self.amp;
            *f = [v, v];
        }
    }
}
