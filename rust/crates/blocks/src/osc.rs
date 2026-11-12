//! Oscillators: SawDPW, Pulse, SinOsc, LFTri.

use libm::sinf;

const TAU: f32 = core::f32::consts::TAU;

fn advance(phase: f32, inc: f32) -> f32 {
    let p = phase + inc;
    p - libm::floorf(p)
}

/// Band-limited saw (2nd-order DPW), output -1..1.
pub struct SawDpw {
    sr: f32,
    /// bipolar phase in [-1, 1)
    x: f32,
    prev_sq: f32,
}

impl SawDpw {
    pub fn new(sr: f32) -> SawDpw {
        SawDpw { sr, x: 0.0, prev_sq: 0.0 }
    }

    pub fn tick(&mut self, hz: f32) -> f32 {
        let dx = 2.0 * hz.abs() / self.sr;
        let mut x = self.x + dx;
        if x >= 1.0 {
            x -= 2.0 * libm::floorf((x + 1.0) * 0.5);
        }
        self.x = x;
        let sq = x * x;
        let out = if dx > 1e-9 { (sq - self.prev_sq) / (2.0 * dx) } else { x };
        self.prev_sq = sq;
        out.clamp(-1.0, 1.0)
    }
}

pub struct Pulse {
    sr: f32,
    phase: f32,
}

impl Pulse {
    pub fn new(sr: f32) -> Pulse {
        Pulse { sr, phase: 0.0 }
    }

    /// `width` is the duty (0..1); output is +1 while phase < width, else -1.
    pub fn tick(&mut self, hz: f32, width: f32) -> f32 {
        self.phase = advance(self.phase, hz / self.sr);
        if self.phase < width { 1.0 } else { -1.0 }
    }
}

pub struct SinOsc {
    sr: f32,
    phase: f32,
}

impl SinOsc {
    /// `phase` in cycles (0..1).
    pub fn new(sr: f32, phase: f32) -> SinOsc {
        SinOsc { sr, phase: phase - libm::floorf(phase) }
    }

    pub fn tick(&mut self, hz: f32) -> f32 {
        let out = sinf(self.phase * TAU);
        self.phase = advance(self.phase, hz / self.sr);
        out
    }
}

/// Naive triangle, -1..1.
pub struct LfTri {
    sr: f32,
    phase: f32,
}

impl LfTri {
    pub fn new(sr: f32, phase: f32) -> LfTri {
        LfTri { sr, phase: phase - libm::floorf(phase) }
    }

    pub fn tick(&mut self, hz: f32) -> f32 {
        let p = self.phase;
        self.phase = advance(p, hz / self.sr);
        // 0 -> 0, .25 -> 1, .75 -> -1
        if p < 0.25 {
            4.0 * p
        } else if p < 0.75 {
            2.0 - 4.0 * p
        } else {
            4.0 * p - 4.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn saw_dpw_stays_in_range() {
        for hz in [55.0, 440.0, 5000.0] {
            let mut o = SawDpw::new(SR);
            for _ in 0..48_000 {
                let v = o.tick(hz);
                assert!(v.abs() <= 1.0, "{hz}: {v}");
            }
        }
    }

    #[test]
    fn saw_dpw_period() {
        let mut o = SawDpw::new(SR);
        let mut prev = o.tick(440.0);
        let mut last: Option<usize> = None;
        let mut periods = 0;
        for i in 1..4800 {
            let v = o.tick(440.0);
            if prev < 0.0 && v >= 0.0 {
                if let Some(l) = last {
                    let d = i - l;
                    assert!((108..=110).contains(&d), "period {d}");
                    periods += 1;
                }
                last = Some(i);
            }
            prev = v;
        }
        assert!(periods > 30);
    }

    #[test]
    fn pulse_width_changes_duty() {
        let duty = |w: f32| {
            let mut p = Pulse::new(SR);
            let hi = (0..48_000).filter(|_| p.tick(100.0, w) > 0.0).count();
            hi as f32 / 48_000.0
        };
        assert!((duty(0.5) - 0.5).abs() < 0.02);
        assert!((duty(0.1) - 0.1).abs() < 0.02);
        assert!((duty(0.9) - 0.9).abs() < 0.02);
    }

    #[test]
    fn sin_and_tri_basic() {
        let mut s = SinOsc::new(SR, 0.25);
        assert!((s.tick(1.0) - 1.0).abs() < 1e-5);
        let mut t = LfTri::new(SR, 0.0);
        let mut max = f32::MIN;
        let mut min = f32::MAX;
        for _ in 0..48_000 {
            let v = t.tick(10.0);
            max = max.max(v);
            min = min.min(v);
        }
        assert!(max > 0.99 && min < -0.99 && max <= 1.0 && min >= -1.0);
    }
}
