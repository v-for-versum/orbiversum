//! Oscillators: SawDPW, Pulse, SinOsc, LFTri.

use crate::fast::sin as sinf;

const TAU: f32 = core::f32::consts::TAU;

#[inline(always)]
fn advance(phase: f32, inc: f32) -> f32 {
    let p = phase + inc;
    p - crate::fast::floor(p)
}

/// Band-limited saw (2nd-order DPW), output -1..1.
pub struct SawDpw {
    inv_sr: f32,
    /// bipolar phase in [-1, 1)
    x: f32,
    prev_sq: f32,
    /// hz the cached increments belong to (NaN = none)
    hz_c: f32,
    dx: f32,
    inv_2dx: f32,
    /// 1.0 when the oscillator is stopped (output is x), else 0.0
    flat: f32,
}

impl SawDpw {
    pub fn new(sr: f32) -> SawDpw {
        SawDpw { inv_sr: 1.0 / sr, x: 0.0, prev_sq: 0.0, hz_c: f32::NAN, dx: 0.0, inv_2dx: 0.0, flat: 0.0 }
    }

    /// Sets the pitch; call when it changes (control rate), then use `tick_fixed`.
    #[inline(always)]
    pub fn set_hz(&mut self, hz: f32) {
        if hz != self.hz_c {
            self.hz_c = hz;
            // dx < 2 keeps a single wrap enough (hz above ~0.99 sr is aliased noise anyway).
            self.dx = (2.0 * hz.abs() * self.inv_sr).min(1.98);
            let live = self.dx > 1e-9;
            self.inv_2dx = if live { 0.5 * crate::fast::recip(self.dx) } else { 0.0 };
            self.flat = if live { 0.0 } else { 1.0 };
        }
    }

    #[inline(always)]
    pub fn tick(&mut self, hz: f32) -> f32 {
        self.set_hz(hz);
        self.tick_fixed()
    }

    /// Branch-free sample (hot loop): no calls, so the compiler can interleave several oscillators.
    #[inline(always)]
    pub fn tick_fixed(&mut self) -> f32 {
        let mut x = self.x + self.dx;
        x -= 2.0 * ((x >= 1.0) as u8 as f32);
        self.x = x;
        let sq = x * x;
        let out = (sq - self.prev_sq) * self.inv_2dx + x * self.flat;
        self.prev_sq = sq;
        out.clamp(-1.0, 1.0)
    }
}

pub struct Pulse {
    inv_sr: f32,
    phase: f32,
}

impl Pulse {
    pub fn new(sr: f32) -> Pulse {
        Pulse { inv_sr: 1.0 / sr, phase: 0.0 }
    }

    /// `width` is the duty (0..1); output is +1 while phase < width, else -1.
    #[inline(always)]
    pub fn tick(&mut self, hz: f32, width: f32) -> f32 {
        self.phase = advance(self.phase, hz * self.inv_sr);
        // Arithmetic select: the xtensa LLVM backend ICEs on the constant-pool
        // form of `if .. { 1.0 } else { -1.0 }` (cannot select PCREL_WRAPPER).
        2.0 * ((self.phase < width) as u8 as f32) - 1.0
    }
}

pub struct SinOsc {
    inv_sr: f32,
    phase: f32,
}

impl SinOsc {
    /// `phase` in cycles (0..1).
    pub fn new(sr: f32, phase: f32) -> SinOsc {
        SinOsc { inv_sr: 1.0 / sr, phase: phase - crate::fast::floor(phase) }
    }

    #[inline(always)]
    pub fn tick(&mut self, hz: f32) -> f32 {
        let out = sinf(self.phase * TAU);
        self.phase = advance(self.phase, hz * self.inv_sr);
        out
    }
}

/// Naive triangle, -1..1.
pub struct LfTri {
    inv_sr: f32,
    phase: f32,
}

impl LfTri {
    pub fn new(sr: f32, phase: f32) -> LfTri {
        LfTri { inv_sr: 1.0 / sr, phase: phase - crate::fast::floor(phase) }
    }

    #[inline(always)]
    pub fn tick(&mut self, hz: f32) -> f32 {
        let p = self.phase;
        self.phase = advance(p, hz * self.inv_sr);
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
