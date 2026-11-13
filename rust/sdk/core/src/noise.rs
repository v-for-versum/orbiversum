//! Audio-rate noise: Noise2 (LFNoise2) and PinkNoise.
//!
//! Per-sample generators fed by an [`Rng`]; no allocation, cheap (a few multiply-adds and one
//! random draw per sample).

use crate::rng::Rng;

/// Quadratic B-spline through random values in [-1, 1] drawn `hz` times per second (LFNoise2).
///
/// With the last three values `p0, p1, p2` and `t` the phase in [0, 1), `u = 1 - t`, the output is
/// `0.5 * (u^2*p0 + (1 + 2t - 2t^2)*p1 + t^2*p2)`; the weights are non-negative and sum to 1, so
/// the output stays within the range of the drawn values. At most one new value per call.
pub struct Noise2 {
    rng: Rng,
    inv_sr: f32,
    phase: f32,
    p0: f32,
    p1: f32,
    p2: f32,
}

impl Noise2 {
    /// Creates the generator; `rng` supplies the values (three drawn now), `sr` is the sample
    /// rate in Hz.
    pub fn new(mut rng: Rng, sr: f32) -> Noise2 {
        let p0 = rng.range(-1.0, 1.0);
        let p1 = rng.range(-1.0, 1.0);
        let p2 = rng.range(-1.0, 1.0);
        Noise2 { rng, inv_sr: 1.0 / sr, phase: 0.0, p0, p1, p2 }
    }

    /// Advances by `hz / sr` and returns one sample.
    #[inline(always)]
    pub fn tick(&mut self, hz: f32) -> f32 {
        self.phase += hz * self.inv_sr;
        if self.phase >= 1.0 {
            self.phase -= crate::fast::floor(self.phase);
            self.p0 = self.p1;
            self.p1 = self.p2;
            self.p2 = self.rng.range(-1.0, 1.0);
        }
        let t = self.phase;
        let u = 1.0 - t;
        0.5 * (u * u * self.p0 + (1.0 + 2.0 * t - 2.0 * t * t) * self.p1 + t * t * self.p2)
    }
}

/// Paul Kellet "economy" pink noise filter on white noise.
///
/// White noise uniform in [-1, 1) goes through three one-poles (coefficients 0.99765, 0.963,
/// 0.57) whose outputs are summed with a little of the white input and scaled by 0.2.
pub struct PinkNoise {
    rng: Rng,
    b0: f32,
    b1: f32,
    b2: f32,
}

impl PinkNoise {
    /// Creates the generator; `rng` supplies the white noise. Independent of the sample rate.
    pub fn new(rng: Rng) -> PinkNoise {
        PinkNoise { rng, b0: 0.0, b1: 0.0, b2: 0.0 }
    }

    /// Returns the next pink-noise sample.
    #[inline(always)]
    pub fn tick(&mut self) -> f32 {
        let w = self.rng.range(-1.0, 1.0);
        self.b0 = 0.99765 * self.b0 + w * 0.099_046_0;
        self.b1 = 0.963 * self.b1 + w * 0.296_516_4;
        self.b2 = 0.57 * self.b2 + w * 1.052_691_3;
        (self.b0 + self.b1 + self.b2 + w * 0.1848) * 0.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn noise2_bounded_continuous() {
        let mut n = Noise2::new(Rng::new(7), SR);
        let mut prev = n.tick(1000.0);
        let mut sum = 0.0f64;
        let mut maxd = 0.0f32;
        for _ in 0..200_000 {
            let y = n.tick(1000.0);
            assert!(y.abs() <= 1.0);
            maxd = maxd.max((y - prev).abs());
            sum += y as f64;
            prev = y;
        }
        assert!(maxd < 0.05, "{maxd}");
        assert!((sum / 200_000.0).abs() < 0.05);
        let mut a = Noise2::new(Rng::new(7), SR);
        let mut b = Noise2::new(Rng::new(8), SR);
        assert!((0..1000).any(|_| a.tick(1000.0) != b.tick(1000.0)));
    }

    #[test]
    fn pink_noise_finite_and_spectrally_tilted() {
        let mut p = PinkNoise::new(Rng::new(7));
        let x: alloc::vec::Vec<f32> = (0..100_000).map(|_| p.tick()).collect();
        let mx = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(x.iter().all(|v| v.is_finite()) && mx < 2.0, "max {mx}");
        // Low-band power (64-sample moving average) over high-band power (first difference).
        // White noise gives ~1/128; pink must tilt well above that.
        fn tilt(x: &[f32]) -> f64 {
            let (mut lo, mut hi) = (0.0f64, 0.0f64);
            for i in 64..x.len() {
                let ma: f32 = x[i - 63..=i].iter().sum::<f32>() / 64.0;
                let d = x[i] - x[i - 1];
                lo += (ma * ma) as f64;
                hi += (d * d) as f64;
            }
            lo / hi
        }
        let mut r = Rng::new(7);
        let white: alloc::vec::Vec<f32> = (0..100_000).map(|_| r.range(-1.0, 1.0)).collect();
        let (tp, tw) = (tilt(&x), tilt(&white));
        assert!(tp >= 3.0 * tw, "pink {tp} white {tw}");
    }
}
