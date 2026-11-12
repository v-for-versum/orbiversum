//! Fast approximations for the audio hot path.

/// Padé [7/6] approximation of `tanh`, input clamped to ±4.9.
///
/// Odd, monotonic, bounded by ±1, continuous. Max abs error against
/// `libm::tanhf` on [-8, 8] is ~1e-4 (at the clamp point).
/// No branches on constants: clamp + polynomial ratio only.
#[inline]
pub fn tanh(x: f32) -> f32 {
    let x = x.clamp(-4.9, 4.9);
    let x2 = x * x;
    let num = x * (135135.0 + x2 * (17325.0 + x2 * (378.0 + x2)));
    let den = 135135.0 + x2 * (62370.0 + x2 * (3150.0 + 28.0 * x2));
    num / den
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_to_libm() {
        let mut worst = 0.0f32;
        for i in -8000..=8000 {
            let x = i as f32 / 1000.0;
            worst = worst.max((tanh(x) - libm::tanhf(x)).abs());
        }
        assert!(worst < 2e-4, "max error {worst}");
    }

    #[test]
    fn zero_odd_and_bounded() {
        assert_eq!(tanh(0.0), 0.0);
        for i in 1..=20000 {
            let x = i as f32 / 1000.0;
            assert_eq!(tanh(-x), -tanh(x));
            assert!(tanh(x) <= 1.0);
        }
        assert!(tanh(1e30) <= 1.0 && tanh(f32::INFINITY) <= 1.0);
    }

    #[test]
    fn monotonic() {
        let mut prev = tanh(-10.0);
        for i in -99_999..=100_000 {
            let y = tanh(i as f32 / 10_000.0);
            // f32 rounding noise near saturation allows up to ~1e-6 of backsliding (f32 evaluation noise; the exact function is monotonic)
            assert!(y >= prev - 1e-6, "not monotonic at {} ({y} < {prev})", i as f32 / 10_000.0);
            prev = y;
        }
    }
}
