//! Envelopes: Decay2 (audio rate) and EnvPerc (control rate).

use crate::fast;

/// SC Decay2: `Decay(x, decay) - Decay(x, attack)`, each a one-pole `y = x + coef * y_prev`.
pub struct Decay2 {
    ca: f32,
    cd: f32,
    ya: f32,
    yd: f32,
}

impl Decay2 {
    pub fn new(sr: f32, attack_s: f32, decay_s: f32) -> Decay2 {
        let coef = |t: f32| if t > 0.0 { libm::expf(-1.0 / (sr * t)) } else { 0.0 };
        Decay2 { ca: coef(attack_s), cd: coef(decay_s), ya: 0.0, yd: 0.0 }
    }

    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.yd = x + self.cd * self.yd;
        self.ya = x + self.ca * self.ya;
        self.yd - self.ya
    }
}

/// SC Env.perc-style envelope: linear attack to `level`, curved release to 0.
pub struct EnvPerc {
    attack_s: f32,
    inv_attack: f32,
    inv_release: f32,
    level: f32,
    curve: f32,
    /// 1 / (1 - e^curve), or 0 when curve ~ 0 (linear release)
    inv_den: f32,
    t: f32,
    active: bool,
}

impl EnvPerc {
    pub fn new(attack_s: f32, release_s: f32, level: f32, curve: f32) -> EnvPerc {
        let den = 1.0 - fast::exp(curve);
        EnvPerc {
            attack_s,
            inv_attack: if attack_s > 0.0 { 1.0 / attack_s } else { 0.0 },
            inv_release: if release_s > 0.0 { 1.0 / release_s } else { 0.0 },
            level,
            curve,
            inv_den: if den.abs() > 1e-4 { 1.0 / den } else { 0.0 },
            t: 0.0,
            active: false,
        }
    }

    pub fn trigger(&mut self) {
        self.t = 0.0;
        self.active = true;
    }

    #[inline(always)]
    pub fn tick(&mut self, dt: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        self.t += dt;
        if self.t < self.attack_s {
            // SC Env.perc applies `curve` to both segments: 0 -> level with the same shape.
            let u = self.t * self.inv_attack;
            let shape = if self.inv_den != 0.0 {
                (1.0 - fast::exp(self.curve * u)) * self.inv_den
            } else {
                u
            };
            return self.level * shape;
        }
        let u = (self.t - self.attack_s) * self.inv_release;
        if u >= 1.0 {
            self.active = false;
            return 0.0;
        }
        let shape = if self.inv_den != 0.0 {
            (1.0 - fast::exp(self.curve * u)) * self.inv_den
        } else {
            u
        };
        self.level * (1.0 - shape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 32.0 / 48_000.0;

    #[test]
    fn decay2_impulse_response() {
        let sr = 48_000.0;
        let mut d = Decay2::new(sr, 0.01, 1.0);
        let first = d.tick(1.0);
        assert!(first.abs() < 1e-6, "{first}");
        let (mut peak, mut peak_i) = (0.0f32, 0usize);
        let n = (4.0 * sr) as usize;
        let mut last = 0.0;
        for i in 1..=n {
            let y = d.tick(0.0);
            assert!(y.is_finite());
            if y > peak {
                peak = y;
                peak_i = i;
            }
            last = y;
        }
        let t = peak_i as f32 / sr;
        assert!((0.005..0.06).contains(&t), "peak at {t}");
        assert!(peak > 0.1 && peak < 1.0, "{peak}");
        assert!(last.abs() < 0.05 * peak, "{last}");
    }

    #[test]
    fn env_perc_shape() {
        let mut e = EnvPerc::new(0.01, 4.0, 1.0, -4.0);
        assert_eq!(e.tick(DT), 0.0);
        e.trigger();
        let mut t = 0.0f32;
        let mut at = |e: &mut EnvPerc, until: f32| {
            let mut v = 0.0;
            while t < until {
                v = e.tick(DT);
                t += DT;
            }
            v
        };
        let v = at(&mut e, 0.005);
        // curve -4 attack is concave: at half the attack time it is already past 0.8 of the level
        assert!(v > 0.8 && v < 0.9, "{v}");
        let v = at(&mut e, 0.01);
        assert!((v - 1.0).abs() < 0.02, "{v}");
        let v = at(&mut e, 1.01);
        assert!(v > 0.3 && v < 0.9, "{v}");
        let v = at(&mut e, 4.02);
        assert!(v < 0.01);
        assert_eq!(e.tick(DT), 0.0);

        let mut p = EnvPerc::new(3.0, 10.0, 0.9, -4.0);
        p.trigger();
        let mut peak = 0.0f32;
        for _ in 0..(3.0 / DT) as usize + 2 {
            peak = peak.max(p.tick(DT));
        }
        assert!((peak - 0.9).abs() < 0.01, "{peak}");
        // retrigger mid-release restarts
        for _ in 0..(2.0 / DT) as usize {
            p.tick(DT);
        }
        p.trigger();
        let v = p.tick(DT);
        assert!(v < 0.01, "{v}");
    }
}
