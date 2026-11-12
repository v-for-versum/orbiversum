//! Delay line with cubic (Catmull-Rom) interpolation, like SC DelayC.

use alloc::vec;
use alloc::vec::Vec;

pub struct DelayC {
    sr: f32,
    buf: Vec<f32>,
    w: usize,
}

impl DelayC {
    /// Allocates the buffer once; `tick` never allocates.
    pub fn new(sr: f32, max_s: f32) -> DelayC {
        let len = (max_s * sr) as usize + 4;
        DelayC { sr, buf: vec![0.0; len], w: 0 }
    }

    pub fn tick(&mut self, x: f32, delay_s: f32) -> f32 {
        let len = self.buf.len();
        self.buf[self.w] = x;
        let mut d = (delay_s * self.sr).clamp(1.0, (len - 3) as f32);
        // Snap float noise (e.g. n / sr * sr) so integer delays are exact.
        let nearest = (d + 0.5) as usize as f32;
        if (d - nearest).abs() < 1e-3 {
            d = nearest;
        }
        let k = d as usize; // floor, d >= 1
        let f = d - k as f32;
        let at = |delay: usize| self.buf[(self.w + len - delay) % len];
        let y0 = at(k - 1);
        let y1 = at(k);
        let y2 = at(k + 1);
        let y3 = at(k + 2);
        self.w = (self.w + 1) % len;
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
