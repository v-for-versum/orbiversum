//! Deterministic xorshift64* generator, no std.

#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
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

    /// Uniform in [0, 1).
    #[inline(always)]
    pub fn f32(&mut self) -> f32 {
        (((self.next_u64() >> 40) as u32) as f32) * (1.0 / 16_777_216.0)
    }

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
