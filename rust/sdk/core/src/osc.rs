//! Oscillators: SawDPW, Pulse, SinOsc, LFTri.
//!
//! Audio-rate, one sample per `tick`. Each keeps a phase accumulator (and, for
//! [`SawDpw`], one previous-square value); none allocates after construction and all
//! are cheap (a few multiply-adds; `SinOsc` adds one [`crate::fast::sin`]). Phases
//! advance by `hz / sr` per call and wrap by subtracting `floor`, so negative `hz` runs the
//! phase backwards. Only [`SawDpw`] is band-limited; [`Pulse`] and [`LfTri`] are naive
//! shapes and alias at high pitch.
//!
//! ```
//! use orbiversum_core::osc::SawDpw;
//!
//! let mut saw = SawDpw::new(48_000.0);
//! for _ in 0..1000 {
//!     assert!(saw.tick(220.0).abs() <= 1.0);
//! }
//! ```

use crate::fast::sin as sinf;

const TAU: f32 = core::f32::consts::TAU;

#[inline(always)]
fn advance(phase: f32, inc: f32) -> f32 {
    let p = phase + inc;
    p - crate::fast::floor(p)
}

/// Band-limited saw (2nd-order DPW), output -1..1.
///
/// Algorithm: a bipolar phase `x` in [-1, 1) rises by `dx = 2 * |hz| / sr` per sample
/// and wraps by subtracting 2. The output is the difference of the squared phase,
/// `(x^2 - x_prev^2) / (2 * dx)`, clamped to [-1, 1]: the derivative of a parabola is a
/// saw, and differencing a smooth parabola instead of the saw's jump limits aliasing.
/// When `|hz|` is so low that `dx <= 1e-9` the oscillator is treated as stopped and
/// outputs the raw phase `x`. The sign of `hz` is ignored (`abs`).
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
    /// Creates an oscillator for sample rate `sr` (Hz, must be > 0; not checked), phase 0.
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

    /// One sample at pitch `hz` (Hz; `|hz|` above ~0.99 * sr is capped to `dx = 1.98`).
    /// Equivalent to `set_hz(hz)` then [`SawDpw::tick_fixed`]; the coefficients are only
    /// recomputed when `hz` differs from the previous call.
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

/// Naive (not band-limited) pulse wave, output +1 or -1.
pub struct Pulse {
    inv_sr: f32,
    phase: f32,
}

impl Pulse {
    /// Creates an oscillator for sample rate `sr` (Hz, must be > 0; not checked), phase 0.
    pub fn new(sr: f32) -> Pulse {
        Pulse { inv_sr: 1.0 / sr, phase: 0.0 }
    }

    /// One sample at `hz` (Hz). `width` is the duty (0..1); output is +1 while phase < width,
    /// else -1. The phase is advanced first, then compared. `width` is not clamped: <= 0
    /// gives a constant -1 and >= 1 a constant +1.
    #[inline(always)]
    pub fn tick(&mut self, hz: f32, width: f32) -> f32 {
        self.phase = advance(self.phase, hz * self.inv_sr);
        // Arithmetic select: the xtensa LLVM backend ICEs on the constant-pool
        // form of `if .. { 1.0 } else { -1.0 }` (cannot select PCREL_WRAPPER).
        2.0 * ((self.phase < width) as u8 as f32) - 1.0
    }
}

/// Sine oscillator: `sin(2 * pi * phase)` through [`crate::fast::sin`].
pub struct SinOsc {
    inv_sr: f32,
    phase: f32,
}

impl SinOsc {
    /// Creates an oscillator for sample rate `sr` (Hz, must be > 0; not checked).
    /// `phase` is the starting phase in cycles; it is wrapped into [0, 1).
    pub fn new(sr: f32, phase: f32) -> SinOsc {
        SinOsc { inv_sr: 1.0 / sr, phase: phase - crate::fast::floor(phase) }
    }

    /// One sample at `hz` (Hz). The output is taken from the phase *before* it advances, so the
    /// first sample is `sin(2 * pi * start_phase)`.
    #[inline(always)]
    pub fn tick(&mut self, hz: f32) -> f32 {
        let out = sinf(self.phase * TAU);
        self.phase = advance(self.phase, hz * self.inv_sr);
        out
    }
}

/// Naive (not band-limited) triangle, -1..1. Phase 0 is 0 and rising, 0.25 is +1,
/// 0.75 is -1.
pub struct LfTri {
    inv_sr: f32,
    phase: f32,
}

impl LfTri {
    /// Creates an oscillator for sample rate `sr` (Hz, must be > 0; not checked).
    /// `phase` is the starting phase in cycles; it is wrapped into [0, 1).
    pub fn new(sr: f32, phase: f32) -> LfTri {
        LfTri { inv_sr: 1.0 / sr, phase: phase - crate::fast::floor(phase) }
    }

    /// One sample at `hz` (Hz); like [`SinOsc::tick`] the output uses the phase before the
    /// advance.
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
