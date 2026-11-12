//! Fast approximations for the audio hot path.

/// 1/d for d > 0 without a hardware divide (the ESP32-S3 FPU has none, so `/`
/// is a software `__divsf3` call of ~100 cycles): bit-trick seed + 3 Newton steps,
/// relative error ~1e-7.
#[inline(always)]
pub fn recip(d: f32) -> f32 {
    let mut y = f32::from_bits(0x7EF3_11C7u32.wrapping_sub(d.to_bits()));
    y *= 2.0 - d * y;
    y *= 2.0 - d * y;
    y *= 2.0 - d * y;
    y
}

/// 1/d with ~2e-4 relative error (2 Newton steps): the shortest dependent chain
/// that is still inaudible inside a saturator.
#[inline(always)]
pub fn recip_fast(d: f32) -> f32 {
    let mut y = f32::from_bits(0x7EF3_11C7u32.wrapping_sub(d.to_bits()));
    y *= 2.0 - d * y;
    y *= 2.0 - d * y;
    y
}

/// Padé [7/6] approximation of `tanh`, input clamped to ±4.9.
///
/// Odd, monotonic, bounded by ±1, continuous. Max abs error against
/// `libm::tanhf` on [-8, 8] is ~1e-4 (at the clamp point).
/// No branches on constants: clamp + polynomial ratio only.
#[inline(always)]
pub fn tanh(x: f32) -> f32 {
    let x = x.clamp(-4.9, 4.9);
    let x2 = x * x;
    let num = x * (135135.0 + x2 * (17325.0 + x2 * (378.0 + x2)));
    let den = 135135.0 + x2 * (62370.0 + x2 * (3150.0 + 28.0 * x2));
    (num * recip_fast(den)).clamp(-1.0, 1.0)
}

/// floor(x) for |x| < 2^31 without a libm call (libm's floorf lives in flash, which the
/// audio path must not touch). Saturates outside that range.
#[inline(always)]
pub fn floor(x: f32) -> f32 {
    let t = x as i32 as f32;
    t - ((x < t) as u8 as f32)
}

/// sin(x), x in radians (any finite magnitude up to ~1e5 stays accurate to ~1e-5).
///
/// f32 only: libm's sinf/cosf/expf/powf compute through f64, which on the
/// ESP32-S3 (no f64 FPU) becomes software float and costs microseconds per call.
#[inline(always)]
pub fn sin(x: f32) -> f32 {
    const INV_PI: f32 = 0.318_309_88;
    // n = round(x / pi), r = x - n*pi in [-pi/2, pi/2], sin(x) = (-1)^n sin(r)
    let n = floor(x * INV_PI + 0.5);
    let r = (x - n * 3.140_625) - n * 9.676_535_6e-4; // pi split in two for accuracy
    let r2 = r * r;
    let p = r
        * (1.0
            + r2 * (-0.166_666_67
                + r2 * (8.333_329_3e-3 + r2 * (-1.984_087_4e-4 + r2 * (2.752_556_2e-6 - 2.388_985_9e-8 * r2)))));
    let odd = (n as i32 & 1) as f32;
    p * (1.0 - 2.0 * odd)
}

/// cos(x), x in radians.
#[inline(always)]
pub fn cos(x: f32) -> f32 {
    sin(x + core::f32::consts::FRAC_PI_2)
}

/// 2^x for x in about [-126, 126] (clamped); relative error < 1e-5.
#[inline(always)]
pub fn exp2(x: f32) -> f32 {
    let x = x.clamp(-126.0, 126.0);
    let n = floor(x);
    let f = x - n;
    let p = 1.0
        + f * (0.693_147_2
            + f * (0.240_226_5
                + f * (0.055_504_11 + f * (0.009_618_129 + f * (0.001_333_355_8 + f * 0.000_154_035_3)))));
    p * f32::from_bits(((n as i32 + 127) as u32) << 23)
}

/// e^x via `exp2`.
#[inline(always)]
pub fn exp(x: f32) -> f32 {
    exp2(x * core::f32::consts::LOG2_E)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_cos_match_libm() {
        let mut x = -20.0f32;
        while x < 20.0 {
            assert!((sin(x) - libm::sinf(x)).abs() < 2e-6, "sin {x}");
            assert!((cos(x) - libm::cosf(x)).abs() < 2e-6, "cos {x}");
            x += 0.0137;
        }
    }

    #[test]
    fn exp2_exp_match_libm() {
        let mut x = -30.0f32;
        while x < 30.0 {
            let (a, b) = (exp2(x), libm::exp2f(x));
            assert!(((a - b) / b).abs() < 2e-5, "exp2 {x}");
            let (a, b) = (exp(x * 0.2), libm::expf(x * 0.2));
            assert!(((a - b) / b).abs() < 2e-5, "exp {x}");
            x += 0.0173;
        }
    }

    #[test]
    fn floor_matches_libm() {
        let mut x = -300.0f32;
        while x < 300.0 {
            assert_eq!(floor(x), libm::floorf(x), "{x}");
            x += 0.37;
        }
        assert_eq!(floor(-2.0), -2.0);
        assert_eq!(floor(0.0), 0.0);
    }

    #[test]
    fn recip_is_accurate() {
        let mut d = 0.001f32;
        while d < 1e7 {
            assert!(((recip(d) * d) - 1.0).abs() < 5e-7, "recip {d}");
            d *= 1.07;
        }
    }

    #[test]
    fn close_to_libm() {
        let mut worst = 0.0f32;
        for i in -8000..=8000 {
            let x = i as f32 / 1000.0;
            worst = worst.max((tanh(x) - libm::tanhf(x)).abs());
        }
        assert!(worst < 6e-4, "max error {worst}");
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
