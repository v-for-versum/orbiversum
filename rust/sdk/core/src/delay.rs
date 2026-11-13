//! Delay line with cubic (Catmull-Rom) interpolation, like SC DelayC.
//!
//! Audio rate. A power-of-two `f32` ring buffer is allocated once in `new` (length >=
//! `max_s * sr + 4` samples); `tick`/`tick_fixed` do no allocation and no bounds checks
//! and cost four loads plus about a dozen multiply-adds, with no divide. The delay is at
//! least 1 sample.
//!
//! ```
//! use orbiversum_core::delay::DelayC;
//!
//! let mut d = DelayC::new(1000.0, 0.01); // up to 10 ms at 1 kHz
//! d.set_delay(0.003); // 3 samples
//! let out: Vec<f32> = [1.0, 0.0, 0.0, 0.0, 0.0].iter().map(|&x| d.tick_fixed(x)).collect();
//! assert_eq!(out, [0.0, 0.0, 0.0, 1.0, 0.0]);
//! ```

use alloc::vec;
use alloc::vec::Vec;

/// Delay line with cubic (Catmull-Rom) interpolation (SC DelayC).
///
/// With the delay `k + f` samples (`k` integer >= 1, `f` in [0, 1)) and the four taps
/// `y0..y3` at `k-1 .. k+2` samples back, the output is the Catmull-Rom cubic
/// `y1 + 0.5*f*((y2-y0) + f*((2*y0 - 5*y1 + 4*y2 - y3) + f*(3*(y1-y2) + y3 - y0)))`, which
/// passes through `y1` at `f = 0` and `y2` at `f = 1`.
pub struct DelayC {
    sr: f32,
    buf: Vec<f32>,
    w: usize,
    /// buf.len() - 1; the length is a power of two
    mask: usize,
    /// largest delay in samples (as before pow2 rounding)
    max_d: f32,
    /// delay_s the cached (k, f) belong to (NaN = none)
    delay_c: f32,
    k: usize,
    f: f32,
}

impl DelayC {
    /// Allocates the buffer once; `tick` never allocates. `sr` is in Hz, `max_s` is the largest
    /// delay in seconds (longer requests are clamped). The initial delay is 1 sample.
    pub fn new(sr: f32, max_s: f32) -> DelayC {
        let need = (max_s * sr) as usize + 4;
        let len = need.next_power_of_two();
        DelayC { sr, buf: vec![0.0; len], w: 0, mask: len - 1, max_d: (need - 3) as f32, delay_c: f32::NAN, k: 1, f: 0.0 }
    }

    /// Start address of the sample buffer (for memory-placement diagnostics).
    pub fn buf_ptr(&self) -> *const u8 {
        self.buf.as_ptr().cast()
    }

    /// Sets the delay time `delay_s` in seconds; call when it changes (control rate), then use
    /// `tick_fixed`. Clamped to [1 sample, max]; a delay within 1e-3 sample of a whole
    /// number snaps to it. Nothing is recomputed if the value equals the previous one.
    /// NaN is not handled here (unlike [`crate::comb::DelayL::set_delay`]); pass a finite value.
    #[inline(always)]
    pub fn set_delay(&mut self, delay_s: f32) {
        if delay_s != self.delay_c {
            self.delay_c = delay_s;
            let mut d = (delay_s * self.sr).clamp(1.0, self.max_d);
            // Snap float noise (e.g. n / sr * sr) so integer delays are exact.
            let nearest = (d + 0.5) as usize as f32;
            if (d - nearest).abs() < 1e-3 {
                d = nearest;
            }
            self.k = d as usize; // floor, d >= 1
            self.f = d - self.k as f32;
        }
    }

    /// Sets the delay (see [`DelayC::set_delay`]) and processes one sample `x`.
    #[inline(always)]
    pub fn tick(&mut self, x: f32, delay_s: f32) -> f32 {
        self.set_delay(delay_s);
        self.tick_fixed(x)
    }

    /// Stores `x` and returns the sample at the delay set by `set_delay`. No branches or bounds
    /// checks, so several lines can be interleaved by the compiler.
    #[inline(always)]
    pub fn tick_fixed(&mut self, x: f32) -> f32 {
        let (k, f, mask, w) = (self.k, self.f, self.mask, self.w);
        debug_assert!(self.buf.len() == mask + 1 && w <= mask && k >= 1 && k + 2 <= mask);
        let buf = self.buf.as_mut_ptr();
        // SAFETY: buf.len() == mask + 1 (power of two), `w` and every masked index are <= mask.
        let at = |delay: usize| unsafe { *buf.add(w.wrapping_sub(delay) & mask) };
        unsafe { *buf.add(w) = x };
        let y0 = at(k - 1);
        let y1 = at(k);
        let y2 = at(k + 1);
        let y3 = at(k + 2);
        self.w = (w + 1) & mask;
        y1 + 0.5
            * f
            * ((y2 - y0) + f * ((2.0 * y0 - 5.0 * y1 + 4.0 * y2 - y3) + f * (3.0 * (y1 - y2) + y3 - y0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_returns_input_after_n_samples() {
        let sr = 48000.0;
        let n = 100usize;
        let mut d = DelayC::new(sr, 0.1);
        for i in 0..400usize {
            let x = if i == 0 { 1.0 } else { 0.0 } + (i as f32) * 1e-3;
            let y = d.tick(x, n as f32 / sr);
            if i >= n {
                let expect = if i == n { 1.0 } else { 0.0 } + ((i - n) as f32) * 1e-3;
                assert_eq!(y, expect, "i={i}");
            }
        }
    }

    #[test]
    fn delay_fractional_is_smooth() {
        let sr = 48000.0;
        let mut d = DelayC::new(sr, 0.1);
        let mut prev = 0.0f32;
        for i in 0..2000 {
            let x = libm::sinf(2.0 * core::f32::consts::PI * 200.0 * i as f32 / sr);
            let y = d.tick(x, 33.37 / sr);
            if i > 100 {
                assert!((y - prev).abs() < 0.1, "jump at {i}");
                let want = libm::sinf(2.0 * core::f32::consts::PI * 200.0 * (i as f32 - 33.37) / sr);
                assert!((y - want).abs() < 1e-3, "i={i} y={y} want={want}");
            }
            prev = y;
        }
    }
}
