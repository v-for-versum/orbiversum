//! Deterministic xorshift64* generator, no std.
//!
//! One `u64` of state, no allocation, a few integer ops per draw (shifts `12, 25, 27`,
//! multiply by `0x2545F4914F6CDD1D`). The same seed always gives the same sequence, which
//! the golden-WAV tests rely on. Not cryptographic. Usable at audio or control rate.
//!
//! ```
//! use orbiversum_core::rng::Rng;
//!
//! let mut a = Rng::new(42);
//! let mut b = Rng::new(42);
//! assert_eq!(a.f32(), b.f32());
//! let x = a.range(-1.0, 1.0);
//! assert!((-1.0..1.0).contains(&x));
//! ```

/// xorshift64* pseudo-random generator; `Clone` copies the state, so a clone replays the
/// same sequence.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a generator from any `u64` seed (0 is fine).
    ///
    /// The seed is scrambled with a splitmix64-style mix so that small, similar seeds give
    /// unrelated streams; a scrambled value of 0 (invalid for xorshift) is replaced by a
    /// constant.
    pub fn new(seed: u64) -> Rng {
        // splitmix-style scramble so small seeds work; state must be non-zero
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng { state: if z == 0 { 0x9E37_79B9_7F4A_7C15 } else { z } }
    }

    #[inline(always)]
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1): the top 24 bits of the next `u64` times 2^-24, so values are exact
    /// multiples of 2^-24 and never reach 1.0.
    #[inline(always)]
    pub fn f32(&mut self) -> f32 {
        (((self.next_u64() >> 40) as u32) as f32) * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[lo, hi)`: `lo + (hi - lo) * f32()`. `lo <= hi` is not checked (a reversed
    /// range just mirrors it); with `hi` very close to `lo` float rounding may return `hi`.
    #[inline(always)]
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_per_seed() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let mut c = Rng::new(43);
        let mut differs = false;
        for _ in 0..100 {
            let (x, y, z) = (a.f32(), b.f32(), c.f32());
            assert_eq!(x, y);
            assert!((0.0..1.0).contains(&x));
            if x != z {
                differs = true;
            }
        }
        assert!(differs);
        let r = Rng::new(1).range(-2.0, 3.0);
        assert!((-2.0..3.0).contains(&r));
    }
}
