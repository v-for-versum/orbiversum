//! Equal-power stereo panning (SC Pan2).
//!
//! Stateless, audio rate, no allocation; cost is one [`sin_cos_0_pi`] per call.
//!
//! ```
//! use orbiversum_core::pan::pan2;
//!
//! let [l, r] = pan2(1.0, -1.0); // hard left
//! assert!((l - 1.0).abs() < 1e-6 && r.abs() < 1e-6);
//! let [l, r] = pan2(1.0, 0.0); // centre: each side at 1/sqrt(2)
//! assert!((l - r).abs() < 1e-6 && (l * l + r * r - 1.0).abs() < 1e-5);
//! ```

use crate::fast::sin_cos_0_pi;

/// Pans mono sample `x` to `[left, right]`.
///
/// `pos` runs from -1 (left) to 1 (right) and is clamped to that range. With
/// `a = (pos + 1) * pi/4`, the result is `[x * cos(a), x * sin(a)]`, so
/// `left^2 + right^2 = x^2` (equal power): the centre sends `x / sqrt(2)` (-3 dB) to each side.
#[inline(always)]
pub fn pan2(x: f32, pos: f32) -> [f32; 2] {
    let a = (pos.clamp(-1.0, 1.0) + 1.0) * core::f32::consts::FRAC_PI_4;
    let (sn, cs) = sin_cos_0_pi(a);
    [x * cs, x * sn]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan2_equal_power() {
        let mut p = -1.0f32;
        while p <= 1.0 {
            let [l, r] = pan2(1.0, p);
            assert!((l * l + r * r - 1.0).abs() < 1e-5, "pos {p}");
            p += 0.05;
        }
        let [l, r] = pan2(1.0, -1.0);
        assert!(l > 0.999 && r.abs() < 1e-5);
    }
}
