//! Moog ladder filter, Huovilainen model (approximation of SC MoogLadder).

use libm::{expf, tanhf};

pub struct MoogLadder {
    sr: f32,
    s: [f32; 4],
    last: f32,
}

impl MoogLadder {
    pub fn new(sr: f32) -> MoogLadder {
        MoogLadder { sr, s: [0.0; 4], last: 0.0 }
    }

    /// `res` 0..4; clamped to 3.8 so the ladder always rings out instead of
    /// self-oscillating (the Huovilainen feedback gain is ~4 at oscillation).
    pub fn tick(&mut self, x: f32, cutoff_hz: f32, res: f32) -> f32 {
        let fc = (cutoff_hz / self.sr).clamp(0.0005, 0.45);
        let fcr = 1.8730 * fc * fc * fc + 0.4955 * fc * fc - 0.6490 * fc + 0.9988;
        let acr = -3.9364 * fc * fc + 1.8409 * fc + 0.9968;
        let tune = 1.0 - expf(-2.0 * core::f32::consts::PI * (fc * 0.5) * fcr);
        let res4 = res.clamp(0.0, 3.8) * acr;
        // 2x oversampled, input repeated.
        for _ in 0..2 {
            let input = x - res4 * self.last;
            let mut prev = tanhf(input);
            for k in 0..4 {
                let t = tanhf(self.s[k]);
                self.s[k] += tune * (prev - t);
                prev = tanhf(self.s[k]);
            }
            self.last = 0.5 * (self.s[3] + self.last);
        }
        self.s[3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_stable_at_high_resonance() {
        let mut f = MoogLadder::new(48000.0);
        let mut y = f.tick(1.0, 1000.0, 3.99);
        assert!(y.is_finite());
        let mut peak_late = 0.0f32;
        for i in 1..96000 {
            y = f.tick(0.0, 1000.0, 3.99);
            assert!(y.is_finite(), "NaN/inf at {i}");
            if i > 90000 {
                peak_late = peak_late.max(y.abs());
            }
        }
        assert!(peak_late < 1e-3, "impulse response not decaying: {peak_late}");
    }
}
