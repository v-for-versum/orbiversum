//! Pure sample conversions (no FFI, host-testable).

/// Scale a float sample in [-1, 1] to i16: clips out-of-range values, NaN -> 0.
/// Symmetric (+-1.0 -> +-32767); -32768 is never produced.
#[inline]
pub fn f32_to_i16(x: f32) -> i16 {
    if x.is_nan() {
        return 0;
    }
    (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

/// Convert interleaved stereo float frames into i16 (`out.len() == 2 * frames.len()`).
#[inline]
pub fn frames_to_i16(frames: &[[f32; 2]], out: &mut [i16]) {
    for (f, o) in frames.iter().zip(out.chunks_exact_mut(2)) {
        o[0] = f32_to_i16(f[0]);
        o[1] = f32_to_i16(f[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_to_i16_clips_and_scales() {
        assert_eq!(f32_to_i16(1.0), 32767);
        assert_eq!(f32_to_i16(-1.0), -32767);
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(f32_to_i16(2.5), 32767);
        assert_eq!(f32_to_i16(-2.5), -32767);
        assert_eq!(f32_to_i16(f32::NAN), 0);
        assert_eq!(f32_to_i16(f32::INFINITY), 32767);
        assert_eq!(f32_to_i16(f32::NEG_INFINITY), -32767);
        assert_eq!(f32_to_i16(0.5), 16384);
    }
}
