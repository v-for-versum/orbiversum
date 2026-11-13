//! Moog ladder filter, Huovilainen model (approximation of SC MoogLadder).

use crate::fast::tanh_cheap as tanh;
use crate::fast::exp as expf;

pub struct MoogLadder {
    inv_sr: f32,
    inv_passes: f32,
    /// oversampling passes per tick (1 or 2)
    passes: usize,
    s: [f32; 4],
    last: f32,
    /// tanh of each stage state, kept in sync with `s`
    ts: [f32; 4],
    /// cutoff the cached coefficients were computed for (NaN = none yet)
    cached_cutoff: f32,
    tune: f32,
    acr: f32,
}

impl MoogLadder {
    pub fn new(sr: f32) -> MoogLadder {
        MoogLadder::with_oversample(sr, 2)
    }

    /// `passes` ladder iterations per tick (1 = no oversampling, cheaper).
    pub fn with_oversample(sr: f32, passes: usize) -> MoogLadder {
        let passes = passes.clamp(1, 2);
        MoogLadder { inv_sr: 1.0 / sr, inv_passes: 1.0 / passes as f32, passes, s: [0.0; 4], last: 0.0, ts: [0.0; 4], cached_cutoff: f32::NAN, tune: 0.0, acr: 0.0 }
    }

    /// `res` 0..4; clamped to 3.8 so the ladder always rings out instead of
    /// self-oscillating (the Huovilainen feedback gain is ~4 at oscillation).
    #[inline(always)]
    pub fn tick(&mut self, x: f32, cutoff_hz: f32, res: f32) -> f32 {
        if cutoff_hz != self.cached_cutoff {
            let fc = (cutoff_hz * self.inv_sr).clamp(0.0005, 0.45);
            let fcr = 1.8730 * fc * fc * fc + 0.4955 * fc * fc - 0.6490 * fc + 0.9988;
            self.acr = -3.9364 * fc * fc + 1.8409 * fc + 0.9968;
            self.tune = 1.0 - expf(-2.0 * core::f32::consts::PI * (fc * self.inv_passes) * fcr);
            self.cached_cutoff = cutoff_hz;
        }
        let (tune, acr) = (self.tune, self.acr);
        let res4 = res.clamp(0.0, 3.8) * acr;
        // 2x oversampled, input repeated. tanh(s[k]) is cached: the value
        // computed after a stage update is exactly the next step's `t`.
        // With res == 0 the input is the same on both passes: one tanh.
        let mut tin = if res4 == 0.0 { tanh(x) } else { 0.0 };
        for _ in 0..self.passes {
            if res4 != 0.0 {
                tin = tanh(x - res4 * self.last);
            }
            let mut prev = tin;
            for k in 0..4 {
                self.s[k] += tune * (prev - self.ts[k]);
                self.ts[k] = tanh(self.s[k]);
                prev = self.ts[k];
            }
            self.last = 0.5 * (self.s[3] + self.last);
        }
        self.s[3]
    }
}

/// Production configuration (Starlids): ONE ladder pass per sample, not the 2x
/// oversampling of the norns original, and a single input saturation tanh(x) (the old
/// pipeline applied tanh twice: tanh outside, tanh inside). Both are deliberate: the
/// real-time budget on the ESP32-S3 did not allow more; see
/// `dsp/tests/starlids_ref.rs` for the audible cost (within the golden tolerances).
///
/// Two MoogLadders with a shared cutoff, processed in lock step.
///
/// Bit-identical to two `MoogLadder`s; the point is instruction-level
/// parallelism: each ladder is a serial chain of dependent FPU ops (tanh ->
/// stage -> tanh ...), and the in-order S3 core stalls on every one, so
/// interleaving the two independent channels fills those stalls.
pub struct MoogStereo {
    inv_sr: f32,
    inv_passes: f32,
    passes: usize,
    s: [[f32; 4]; 2],
    last: [f32; 2],
    ts: [[f32; 4]; 2],
    cached_cutoff: f32,
    tune: f32,
    acr: f32,
}

impl MoogStereo {
    pub fn with_oversample(sr: f32, passes: usize) -> MoogStereo {
        let passes = passes.clamp(1, 2);
        MoogStereo {
            inv_sr: 1.0 / sr,
            inv_passes: 1.0 / passes as f32,
            passes,
            s: [[0.0; 4]; 2],
            last: [0.0; 2],
            ts: [[0.0; 4]; 2],
            cached_cutoff: f32::NAN,
            tune: 0.0,
            acr: 0.0,
        }
    }

    #[inline(always)]
    pub fn tick(&mut self, x: [f32; 2], cutoff_hz: f32, res: f32) -> [f32; 2] {
        if cutoff_hz != self.cached_cutoff {
            let fc = (cutoff_hz * self.inv_sr).clamp(0.0005, 0.45);
            let fcr = 1.8730 * fc * fc * fc + 0.4955 * fc * fc - 0.6490 * fc + 0.9988;
            self.acr = -3.9364 * fc * fc + 1.8409 * fc + 0.9968;
            self.tune = 1.0 - expf(-2.0 * core::f32::consts::PI * (fc * self.inv_passes) * fcr);
            self.cached_cutoff = cutoff_hz;
        }
        let (tune, acr) = (self.tune, self.acr);
        let res4 = res.clamp(0.0, 3.8) * acr;
        let mut tin = if res4 == 0.0 { [tanh(x[0]), tanh(x[1])] } else { [0.0; 2] };
        for _ in 0..self.passes {
            if res4 != 0.0 {
                tin = [tanh(x[0] - res4 * self.last[0]), tanh(x[1] - res4 * self.last[1])];
            }
            let mut prev = tin;
            for k in 0..4 {
                let s0 = self.s[0][k] + tune * (prev[0] - self.ts[0][k]);
                let s1 = self.s[1][k] + tune * (prev[1] - self.ts[1][k]);
                let t0 = tanh(s0);
                let t1 = tanh(s1);
                self.s[0][k] = s0;
                self.s[1][k] = s1;
                self.ts[0][k] = t0;
                self.ts[1][k] = t1;
                prev = [t0, t1];
            }
            self.last[0] = 0.5 * (self.s[0][3] + self.last[0]);
            self.last[1] = 0.5 * (self.s[1][3] + self.last[1]);
        }
        [self.s[0][3], self.s[1][3]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference: the previous implementation (libm tanh, 18 tanh/tick).
    struct Old {
        sr: f32,
        s: [f32; 4],
        last: f32,
    }
    impl Old {
        fn tick(&mut self, x: f32, cutoff_hz: f32, res: f32) -> f32 {
            let fc = (cutoff_hz / self.sr).clamp(0.0005, 0.45);
            let fcr = 1.8730 * fc * fc * fc + 0.4955 * fc * fc - 0.6490 * fc + 0.9988;
            let acr = -3.9364 * fc * fc + 1.8409 * fc + 0.9968;
            let tune = 1.0 - expf(-2.0 * core::f32::consts::PI * (fc * 0.5) * fcr);
            let res4 = res.clamp(0.0, 3.8) * acr;
            for _ in 0..2 {
                let input = x - res4 * self.last;
                let mut prev = libm::tanhf(input);
                for k in 0..4 {
                    let t = libm::tanhf(self.s[k]);
                    self.s[k] += tune * (prev - t);
                    prev = libm::tanhf(self.s[k]);
                }
                self.last = 0.5 * (self.s[3] + self.last);
            }
            self.s[3]
        }
    }

    fn compare(res: f32, tol: f32) {
        let mut new = MoogLadder::new(48000.0);
        let mut old = Old { sr: 48000.0, s: [0.0; 4], last: 0.0 };
        let mut worst = 0.0f32;
        for i in 0..48000 {
            let t = i as f32 / 48000.0;
            let x = libm::tanhf(1.5 * libm::sinf(2.0 * core::f32::consts::PI * 220.0 * t))
                * 0.8;
            let cut = 300.0 + 2000.0 * (i % 4800) as f32 / 4800.0;
            let (a, b) = (new.tick(x, cut, res), old.tick(x, cut, res));
            worst = worst.max((a - b).abs());
        }
        assert!(worst < tol, "res {res}: max diff {worst}");
    }

    #[test]
    fn matches_reference_res0() {
        compare(0.0, 3e-2);
    }

    #[test]
    fn matches_reference_res_positive() {
        compare(1.5, 6e-2);
    }

    #[test]
    fn stereo_is_bit_identical_to_two_ladders() {
        for passes in [1usize, 2] {
            for res in [0.0f32, 2.0] {
                let mut st = MoogStereo::with_oversample(48000.0, passes);
                let mut a = MoogLadder::with_oversample(48000.0, passes);
                let mut b = MoogLadder::with_oversample(48000.0, passes);
                for i in 0..20000 {
                    let (xl, xr) = (libm::sinf(i as f32 * 0.05) * 0.9, libm::sinf(i as f32 * 0.031) * 0.7);
                    let cut = 400.0 + (i / 32 % 50) as f32 * 40.0;
                    let y = st.tick([xl, xr], cut, res);
                    assert_eq!(y, [a.tick(xl, cut, res), b.tick(xr, cut, res)], "passes {passes} res {res} i {i}");
                }
            }
        }
    }

    /// Production config against the old pipeline (2 passes, tanh(tanh(x)) input, libm tanh).
    /// Measures the waveform difference caused by the two deliberate budget decisions.
    #[test]
    fn production_config_close_to_old_pipeline() {
        let sr = 48000.0;
        let mut new = MoogStereo::with_oversample(sr, 1);
        let mut old = Old { sr, s: [0.0; 4], last: 0.0 };
        let (mut worst, mut ref_peak) = (0.0f32, 0.0f32);
        for i in 0..96000 {
            let t = i as f32 / sr;
            // level like the real Starlids mix (RMS about -19 dB, input of the outer tanh)
            let l = 0.3 * libm::sinf(2.0 * core::f32::consts::PI * 110.0 * t)
                + 0.15 * libm::sinf(2.0 * core::f32::consts::PI * 331.0 * t);
            let cut = 880.0 + 3000.0 * (i % 9600) as f32 / 9600.0;
            let a = new.tick([l, l], cut, 0.0)[0];
            let b = old.tick(libm::tanhf(l), cut, 0.0);
            worst = worst.max((a - b).abs());
            ref_peak = ref_peak.max(b.abs());
        }
        // Measured: max difference 0.027 against a 0.416 peak (6.5%, about -24 dB); bound is 12%.
        assert!(worst < 0.12 * ref_peak, "worst {worst}, ref peak {ref_peak}");
    }

    #[test]
    fn single_pass_stable_at_high_resonance() {
        let mut f = MoogLadder::with_oversample(48000.0, 1);
        let mut y = f.tick(1.0, 1000.0, 3.99);
        for i in 1..96000 {
            y = f.tick(0.0, 1000.0, 3.99);
            assert!(y.is_finite(), "NaN/inf at {i}");
        }
        assert!(y.abs() < 1e-3, "not decaying: {y}");
    }

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
