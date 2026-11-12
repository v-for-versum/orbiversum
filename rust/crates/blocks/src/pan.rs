//! Equal-power stereo panning (SC Pan2).

use libm::{cosf, sinf};

/// `pos` -1 (left) .. 1 (right).
pub fn pan2(x: f32, pos: f32) -> [f32; 2] {
    let a = (pos.clamp(-1.0, 1.0) + 1.0) * core::f32::consts::FRAC_PI_4;
    [x * cosf(a), x * sinf(a)]
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
