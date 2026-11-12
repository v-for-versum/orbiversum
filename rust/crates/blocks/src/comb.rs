//! Linearly interpolated delay line and feedback comb, like SC DelayL / CombL.

use alloc::vec;
use alloc::vec::Vec;

pub struct DelayL {
    sr: f32,
    buf: Vec<f32>,
    w: usize,
    /// buf.len() - 1; the length is a power of two
    mask: usize,
    /// largest delay in samples
    max_d: f32,
    k: usize,
    f: f32,
}

impl DelayL {
    /// Allocates the buffer once; `tick_fixed` never allocates.
    pub fn new(sr: f32, max_s: f32) -> DelayL {
        let need = (max_s * sr) as usize + 4;
        let len = need.next_power_of_two();
        DelayL { sr, buf: vec![0.0; len], w: 0, mask: len - 1, max_d: (need - 3) as f32, k: 1, f: 0.0 }
    }

    /// Start address of the sample buffer (for memory-placement diagnostics).
    pub fn buf_ptr(&self) -> *const u8 {
        self.buf.as_ptr().cast()
    }

    /// Sets the delay time (clamped to [1, max] samples); control rate.
    /// Returns the effective delay in seconds.
    pub fn set_delay(&mut self, s: f32) -> f32 {
        let mut d = (s * self.sr).clamp(1.0, self.max_d);
        if d.is_nan() {
            d = 1.0;
        }
        let nearest = (d + 0.5) as usize as f32;
        if (d - nearest).abs() < 1e-3 {
            d = nearest;
        }
        self.k = d as usize; // floor, d >= 1
        self.f = d - self.k as f32;
        d / self.sr
    }

    /// Delayed sample (before this step's write); k >= 1 so it never reads the slot being written.
    #[inline(always)]
    fn read(&self) -> f32 {
        let (k, f, mask, w) = (self.k, self.f, self.mask, self.w);
        debug_assert!(self.buf.len() == mask + 1 && w <= mask && k >= 1 && k + 1 <= mask);
        let buf = self.buf.as_ptr();
        // SAFETY: buf.len() == mask + 1 (power of two); every masked index is <= mask.
        let (a, b) = unsafe { (*buf.add(w.wrapping_sub(k) & mask), *buf.add(w.wrapping_sub(k + 1) & mask)) };
        a + f * (b - a)
    }

    #[inline(always)]
    fn write(&mut self, x: f32) {
        debug_assert!(self.w <= self.mask);
        // SAFETY: w <= mask < buf.len().
        unsafe { *self.buf.as_mut_ptr().add(self.w) = x };
        self.w = (self.w + 1) & self.mask;
    }

    /// Sample at the delay set by `set_delay`, linear interpolation.
    #[inline(always)]
    pub fn tick_fixed(&mut self, x: f32) -> f32 {
        let y = self.read();
        self.write(x);
        y
    }
}

/// Feedback comb: returns the delayed value `d`, writes `x + fb * d`.
pub struct CombL {
    line: DelayL,
    fb: f32,
}

impl CombL {
    pub fn new(sr: f32, max_s: f32) -> CombL {
        CombL { line: DelayL::new(sr, max_s), fb: 0.0 }
    }

    pub fn buf_ptr(&self) -> *const u8 {
        self.line.buf_ptr()
    }

    /// Control rate; keeps state. `fb` gives -60 dB after `decay_s`.
    pub fn set(&mut self, delay_s: f32, decay_s: f32) {
        let eff = self.line.set_delay(delay_s);
        let decay = if decay_s.is_nan() { 0.01 } else { decay_s.max(0.01) };
        self.fb = crate::fast::exp2(-9.965_784 * eff * crate::fast::recip(decay));
    }

    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        let d = self.line.read();
        self.line.write(x + self.fb * d);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    #[test]
    fn delayl_integer_delay_is_shift() {
        let mut d = DelayL::new(SR, 0.1);
        d.set_delay(10.0 / SR);
        let xs: alloc::vec::Vec<f32> = (0..200).map(|i| libm::sinf(i as f32 * 0.37) + i as f32 * 1e-3).collect();
        for i in 0..200 {
            let y = d.tick_fixed(xs[i]);
            if i >= 10 {
                assert_eq!(y, xs[i - 10], "i={i}");
            }
        }
        // half-step average of a ramp
        let mut d = DelayL::new(SR, 0.1);
        d.set_delay(10.5 / SR);
        for i in 0..200 {
            let y = d.tick_fixed(i as f32);
            if i >= 12 {
                assert!((y - (i as f32 - 10.5)).abs() < 1e-4, "i={i} y={y}");
            }
        }
        // above max clamps without panic
        let mut d = DelayL::new(SR, 0.01);
        d.set_delay(5.0);
        for i in 0..2000 {
            assert!(d.tick_fixed(i as f32).is_finite());
        }
    }

    #[test]
    fn combl_decay_matches_rt60() {
        let mut c = CombL::new(SR, 0.1);
        c.set(0.005, 1.0);
        let mut echoes = alloc::vec::Vec::new();
        for i in 0..48000 + 1000 {
            let y = c.tick(if i == 0 { 1.0 } else { 0.0 });
            if y != 0.0 {
                assert_eq!(i % 240, 0, "echo off grid at {i}");
                echoes.push(y);
            }
        }
        let want = libm::powf(0.001, 0.005);
        assert!((echoes[0] - 1.0).abs() < 1e-4);
        for w in echoes.windows(2) {
            assert!((w[1] / w[0] - want).abs() < 0.002, "ratio {}", w[1] / w[0]);
        }
        // echo n=200 is at sample 48000 (1 s)
        let at_1s = echoes[199];
        assert!(at_1s > 0.0007 && at_1s < 0.0013, "level at 1 s {at_1s}");

        for (dl, dc) in [(0.005, 0.0), (10.0, 1.0), (0.0, 0.0), (f32::NAN, f32::NAN)] {
            let mut c = CombL::new(SR, 0.1);
            c.set(dl, dc);
            for i in 0..5000 {
                assert!(c.tick(if i == 0 { 1.0 } else { 0.0 }).is_finite());
            }
        }
    }

    #[test]
    fn comb_state_continues_across_set() {
        let mut c = CombL::new(SR, 0.1);
        c.set(0.005, 1.0);
        for i in 0..20000 {
            if i % 32 == 0 {
                c.set(0.002 + 0.001 * ((i / 32) % 7) as f32, 0.5 + (i % 5) as f32);
            }
            assert!(c.tick(if i % 1000 == 0 { 1.0 } else { 0.0 }).is_finite());
        }
    }
}
