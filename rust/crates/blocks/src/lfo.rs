//! Slow control-rate-style generators: Noise0, Noise1, VarLagSine.

use crate::rng::Rng;
use libm::cosf;

/// Stepped random (LFNoise0): new value in -1..1 `hz` times per second.
pub struct Noise0 {
    rng: Rng,
    sr: f32,
    phase: f32,
    value: f32,
}

impl Noise0 {
    pub fn new(mut rng: Rng, sr: f32) -> Noise0 {
        let value = rng.range(-1.0, 1.0);
        Noise0 { rng, sr, phase: 0.0, value }
    }

    pub fn tick(&mut self, hz: f32) -> f32 {
        self.phase += hz / self.sr;
        if self.phase >= 1.0 {
            self.phase -= libm::floorf(self.phase);
            self.value = self.rng.range(-1.0, 1.0);
        }
        self.value
    }
}

/// Linearly interpolated random (LFNoise1).
pub struct Noise1 {
    rng: Rng,
    sr: f32,
    phase: f32,
    from: f32,
    to: f32,
}

impl Noise1 {
    pub fn new(mut rng: Rng, sr: f32) -> Noise1 {
        let from = rng.range(-1.0, 1.0);
        let to = rng.range(-1.0, 1.0);
        Noise1 { rng, sr, phase: 0.0, from, to }
    }

    pub fn tick(&mut self, hz: f32) -> f32 {
        self.phase += hz / self.sr;
        if self.phase >= 1.0 {
            self.phase -= libm::floorf(self.phase);
            self.from = self.to;
            self.to = self.rng.range(-1.0, 1.0);
        }
        self.from + (self.to - self.from) * self.phase
    }
}

/// Smooths toward `target` over `time_s` along a sine (raised-cosine) curve.
/// A target change restarts the ramp from the current value.
pub struct VarLagSine {
    sr: f32,
    start: f32,
    target: f32,
    pos: f32,
    value: f32,
    primed: bool,
}

impl VarLagSine {
    pub fn new(sr: f32) -> VarLagSine {
        VarLagSine { sr, start: 0.0, target: 0.0, pos: 1.0, value: 0.0, primed: false }
    }

    pub fn tick(&mut self, target: f32, time_s: f32) -> f32 {
        if !self.primed {
            self.primed = true;
            self.value = target;
            self.start = target;
            self.target = target;
        } else if target != self.target {
            self.start = self.value;
            self.target = target;
            self.pos = 0.0;
        }
        if self.pos < 1.0 {
            let step = if time_s > 0.0 { 1.0 / (time_s * self.sr) } else { 1.0 };
            self.pos = (self.pos + step).min(1.0);
            let w = 0.5 - 0.5 * cosf(self.pos * core::f32::consts::PI);
            self.value = self.start + (self.target - self.start) * w;
        } else {
            self.value = self.target;
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn noise0_holds_between_steps() {
        let mut n = Noise0::new(Rng::new(7), SR);
        let mut prev = n.tick(10.0);
        let mut changes = 0;
        for _ in 0..48_000 {
            let v = n.tick(10.0);
            if v != prev {
                changes += 1;
            }
            prev = v;
        }
        assert!((9..=11).contains(&changes), "changes {changes}");
    }

    #[test]
    fn noise1_is_continuous() {
        let mut n = Noise1::new(Rng::new(9), SR);
        let mut prev = n.tick(10.0);
        for _ in 0..48_000 {
            let v = n.tick(10.0);
            assert!((v - prev).abs() < 0.01, "{prev} -> {v}");
            assert!(v.abs() <= 1.0);
            prev = v;
        }
    }

    #[test]
    fn varlag_reaches_target() {
        let mut l = VarLagSine::new(SR);
        assert_eq!(l.tick(0.0, 0.5), 0.0);
        let mut v = 0.0;
        for _ in 0..(0.5 * SR) as usize + 2 {
            v = l.tick(1.0, 0.5);
        }
        assert!((1.0 - v).abs() < 0.01, "{v}");
    }
}
