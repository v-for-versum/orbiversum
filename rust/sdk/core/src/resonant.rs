//! SC-style RLPF and Ringz resonant filters.
//!
//! Audio rate, per-sample `tick` is a handful of multiply-adds on two stored outputs
//! (`y1`, `y2`); no allocation. `set` is control rate (it needs several reciprocals and
//! sine/cosine evaluations) and keeps the state. Cost class: low (`tick` is the per-sample
//! cost; `set` costs more but runs at control rate only). Both clamp the frequency to
//! [10, 0.45 * sr] Hz and map NaN parameters to a safe default.

use crate::fast::{exp, recip, sin_cos_0_pi};

const TWO_PI: f32 = 2.0 * core::f32::consts::PI;

/// SC RLPF: two-pole resonant lowpass.
///
/// Coefficients, with `pf = 2*pi*hz/sr` and `d = tan(pf*rq/2)`: `c = (1-d)/(1+d)`,
/// `b1 = (1+c)*cos(pf)`, `b2 = -c`, `a0 = (1+c-b1)/4`. Per sample:
/// `y0 = a0*x + b1*y1 + b2*y2; out = y0 + 2*y1 + y2`.
pub struct RLpf {
    sr: f32,
    a0: f32,
    b1: f32,
    b2: f32,
    y1: f32,
    y2: f32,
}

impl RLpf {
    /// Creates the filter for sample rate `sr` (Hz), initially at 1000 Hz, `rq` = 1.
    pub fn new(sr: f32) -> RLpf {
        let mut f = RLpf { sr, a0: 0.0, b1: 0.0, b2: 0.0, y1: 0.0, y2: 0.0 };
        f.set(1000.0, 1.0);
        f
    }

    /// Sets cutoff `hz` (Hz, clamped to [10, 0.45 * sr], NaN -> 10) and `rq`, the reciprocal of Q
    /// (1 = Q of 1; smaller = more resonance; floored at 0.001, NaN -> 1). Control rate;
    /// keeps state.
    pub fn set(&mut self, hz: f32, rq: f32) {
        let hz = if hz.is_nan() { 10.0 } else { hz.clamp(10.0, self.sr * 0.45) };
        let qres = if rq.is_nan() { 1.0 } else { rq.max(0.001) };
        let pf = hz * TWO_PI * recip(self.sr);
        let t = (pf * qres * 0.5).min(core::f32::consts::FRAC_PI_2 * 0.99);
        let (st, ct) = sin_cos_0_pi(t);
        let d = st * recip(ct);
        let c = (1.0 - d) * recip(1.0 + d);
        let (_, cos_pf) = sin_cos_0_pi(pf);
        self.b1 = (1.0 + c) * cos_pf;
        self.b2 = -c;
        self.a0 = (1.0 + c - self.b1) * 0.25;
    }

    /// Filters one sample.
    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y0 = self.a0 * x + self.b1 * self.y1 + self.b2 * self.y2;
        let out = y0 + 2.0 * self.y1 + self.y2;
        self.y2 = self.y1;
        self.y1 = y0;
        out
    }
}

/// SC Ringz: resonator with a decay time (ring to -60 dB).
///
/// A two-pole resonator with pole radius `r = exp(-ln(1000) / (decay * sr))`, so an impulse
/// rings down by 60 dB (a factor of 1000 in amplitude) after `decay` seconds. With
/// `c = cos(2*pi*hz/sr)`: `b1 = 2*r*c`, `b2 = -r^2`; per sample
/// `y0 = x + b1*y1 + b2*y2; out = 0.5*(y0 - y2)`. The gain is not normalised: it grows with
/// `decay`.
pub struct Ringz {
    sr: f32,
    b1: f32,
    b2: f32,
    y1: f32,
    y2: f32,
}

impl Ringz {
    /// Creates the resonator for sample rate `sr` (Hz), initially 440 Hz, 1 s decay.
    pub fn new(sr: f32) -> Ringz {
        let mut f = Ringz { sr, b1: 0.0, b2: 0.0, y1: 0.0, y2: 0.0 };
        f.set(440.0, 1.0);
        f
    }

    /// Sets resonant frequency `hz` (Hz, clamped to [10, 0.45 * sr], NaN -> 10) and `decay_s`
    /// (seconds to -60 dB; floored at 0.01, NaN -> 0.01). Control rate; keeps state.
    pub fn set(&mut self, hz: f32, decay_s: f32) {
        let hz = if hz.is_nan() { 10.0 } else { hz.clamp(10.0, self.sr * 0.45) };
        let decay = if decay_s.is_nan() { 0.01 } else { decay_s.max(0.01) };
        let r = exp(-6.9078 * recip(decay * self.sr));
        let (_, c) = sin_cos_0_pi(hz * TWO_PI * recip(self.sr));
        self.b1 = 2.0 * r * c;
        self.b2 = -r * r;
    }

    /// Filters one sample.
    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y0 = x + self.b1 * self.y1 + self.b2 * self.y2;
        let out = 0.5 * (y0 - self.y2);
        self.y2 = self.y1;
        self.y1 = y0;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libm::sinf;

    const SR: f32 = 48000.0;

    fn gain(f: &mut RLpf, hz: f32) -> f32 {
        let mut peak = 0.0f32;
        for i in 0..48000 {
            let y = f.tick(sinf(TWO_PI * hz * i as f32 / SR));
            if i >= 24000 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn rlpf_passes_low_blocks_high() {
        let mut f = RLpf::new(SR);
        f.set(1000.0, 4.0);
        let lo = gain(&mut f, 100.0);
        assert!((lo - 1.0).abs() < 0.15, "100 Hz gain {lo}");
        let mut f = RLpf::new(SR);
        f.set(1000.0, 4.0);
        let hi = gain(&mut f, 10000.0);
        assert!(hi < 0.05, "10 kHz gain {hi}");

        let mut f = RLpf::new(SR);
        f.set(1000.0, 0.2);
        let pk = gain(&mut f, 1000.0);
        assert!(pk > 2.0, "resonance peak {pk}");

        for (hz, rq) in [(0.0, 0.0), (1e9, 100.0), (f32::NAN, f32::NAN)] {
            let mut f = RLpf::new(SR);
            f.set(hz, rq);
            for i in 0..10000 {
                let y = f.tick(if i % 100 == 0 { 1.0 } else { 0.0 });
                assert!(y.is_finite(), "hz={hz} rq={rq} i={i}");
            }
        }
    }

    #[test]
    fn ringz_rings_for_decay_time() {
        let mut f = Ringz::new(SR);
        f.set(440.0, 1.0);
        let mut out = alloc::vec::Vec::new();
        for i in 0..48000 + 480 {
            out.push(f.tick(if i == 0 { 1.0 } else { 0.0 }));
        }
        let crossings = out[..24000].windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((crossings as i32 / 2 - 220).abs() <= 2, "crossings {crossings}");
        let early = out[..480].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let late = out[48000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let ratio = late / early;
        assert!(ratio > 0.001 / 3.0 && ratio < 0.001 * 3.0, "ratio {ratio}");

        let mut f = Ringz::new(SR);
        f.set(440.0, 0.0);
        for i in 0..5000 {
            assert!(f.tick(if i == 0 { 1.0 } else { 0.0 }).is_finite());
        }
        let mut f = Ringz::new(SR);
        f.set(440.0, 1.0);
        for i in 0..20000 {
            if i % 32 == 0 {
                f.set(200.0 + (i % 3000) as f32, 1.0);
            }
            assert!(f.tick(if i == 0 { 1.0 } else { 0.0 }).is_finite());
        }
    }
}
