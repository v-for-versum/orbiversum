//! RBJ cookbook biquads (transposed direct form II).
//!
//! Audio rate; state is two delay values (`z1`, `z2`) plus five normalised coefficients.
//! No allocation. `tick` is five multiplies and four adds per sample; recomputing
//! coefficients (`set_lowpass`/`set_highpass`) costs one [`crate::fast::sin_cos_0_pi`] and
//! one [`crate::fast::recip`], so do it at control rate.
//!
//! ```
//! use orbiversum_core::biquad::Biquad;
//!
//! let mut lp = Biquad::lowpass(48_000.0, 1000.0, 0.707);
//! let mut y = 0.0;
//! for _ in 0..4800 {
//!     y = lp.tick(1.0); // DC passes a lowpass with unity gain
//! }
//! assert!((y - 1.0).abs() < 1e-3);
//! ```

use crate::fast::sin_cos_0_pi;

/// Second-order IIR filter, lowpass or highpass, with fixed Q.
///
/// With `w0 = 2*pi*hz/sr`, `alpha = sin(w0) / (2*Q)` and `c = cos(w0)`, the lowpass has
/// `b0 = b2 = (1-c)/2`, `b1 = 1-c`, the highpass `b0 = b2 = (1+c)/2`, `b1 = -(1+c)`, and
/// both `a0 = 1+alpha`, `a1 = -2c`, `a2 = 1-alpha`. Coefficients are divided by `a0`
/// once. `tick` runs `y = b0*x + z1; z1 = b1*x - a1*y + z2; z2 = b2*x - a2*y`.
pub struct Biquad {
    sr: f32,
    inv_sr: f32,
    inv_2q: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn blank(sr: f32, q: f32) -> Biquad {
        Biquad { sr, inv_sr: 1.0 / sr, inv_2q: 0.5 / q, b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }

    #[inline(always)]
    fn trig(&self, hz: f32) -> (f32, f32) {
        let hz = hz.clamp(1.0, self.sr * 0.49);
        let w0 = 2.0 * core::f32::consts::PI * hz * self.inv_sr;
        let (sn, cs) = sin_cos_0_pi(w0);
        (cs, sn * self.inv_2q)
    }

    #[inline(always)]
    fn set_coeffs(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        let inv = crate::fast::recip(a0);
        self.b0 = b0 * inv;
        self.b1 = b1 * inv;
        self.b2 = b2 * inv;
        self.a1 = a1 * inv;
        self.a2 = a2 * inv;
    }

    /// Lowpass at `hz` (Hz) with quality factor `q` for sample rate `sr` (Hz). `q` is fixed
    /// for the life of the filter and must be > 0 (not checked). `hz` is clamped to
    /// [1, 0.49 * sr].
    pub fn lowpass(sr: f32, hz: f32, q: f32) -> Biquad {
        let mut f = Biquad::blank(sr, q);
        f.set_lowpass(hz);
        f
    }

    /// Highpass at `hz` (Hz) with quality factor `q`; same parameters and limits as
    /// [`Biquad::lowpass`].
    pub fn highpass(sr: f32, hz: f32, q: f32) -> Biquad {
        let mut f = Biquad::blank(sr, q);
        f.set_highpass(hz);
        f
    }

    /// Recompute lowpass coefficients for `hz` (Hz, clamped to [1, 0.49 * sr]); keeps state and
    /// Q, no allocation. On a highpass-built filter this turns it into a lowpass.
    #[inline(always)]
    pub fn set_lowpass(&mut self, hz: f32) {
        let (c, alpha) = self.trig(hz);
        let b = (1.0 - c) * 0.5;
        self.set_coeffs(b, 1.0 - c, b, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    /// Recompute highpass coefficients for `hz` (Hz, clamped to [1, 0.49 * sr]); keeps state
    /// and Q, no allocation.
    pub fn set_highpass(&mut self, hz: f32) {
        let (c, alpha) = self.trig(hz);
        let b = (1.0 + c) * 0.5;
        self.set_coeffs(b, -(1.0 + c), b, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    /// Filters one sample.
    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libm::{sinf, sqrtf};

    const SR: f32 = 48000.0;

    fn rms_after_settle(f: &mut Biquad, hz: f32) -> f32 {
        let mut acc = 0.0f32;
        let n = 48000;
        for i in 0..n {
            let x = sinf(2.0 * core::f32::consts::PI * hz * i as f32 / SR);
            let y = f.tick(x);
            if i >= n / 2 {
                acc += y * y;
            }
        }
        sqrtf(acc / (n / 2) as f32)
    }

    #[test]
    fn lowpass_attenuates_above_cutoff() {
        let mut f = Biquad::lowpass(SR, 500.0, 0.707);
        let rms = rms_after_settle(&mut f, 5000.0);
        let in_rms = core::f32::consts::FRAC_1_SQRT_2;
        let db = 20.0 * libm::log10f(rms / in_rms);
        assert!(db < -20.0, "attenuation {db} dB");
    }

    #[test]
    fn highpass_blocks_dc() {
        let mut f = Biquad::highpass(SR, 100.0, 0.707);
        let mut y = 1.0;
        for _ in 0..48000 {
            y = f.tick(1.0);
        }
        assert!(libm::fabsf(y) < 1e-4, "dc leak {y}");
    }

    #[test]
    fn set_lowpass_changes_cutoff() {
        let mut f = Biquad::lowpass(SR, 200.0, 0.707);
        f.set_lowpass(8000.0);
        let rms = rms_after_settle(&mut f, 1000.0);
        assert!(rms > 0.6, "rms {rms}");
    }
}
