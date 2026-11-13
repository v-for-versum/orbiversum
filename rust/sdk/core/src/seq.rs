//! Control-rate sequencing helpers: Dust, Impulse, Dseq, Dxrand, TChoose, TDelay.
//! `dt` is seconds per control step. No allocation in any `tick`/`next`.
//!
//! All are cheap (a comparison or one random draw per call) and are called once per control
//! step, not per sample. The ones that draw random numbers take an [`Rng`] at construction.
//!
//! ```
//! use orbiversum_core::seq::{Dseq, Impulse};
//!
//! let mut imp = Impulse::new();
//! let fired: Vec<bool> = (0..5).map(|_| imp.tick(10.0, 0.05)).collect(); // 10 Hz, 50 ms steps
//! assert_eq!(fired, [true, false, true, false, true]);
//!
//! let mut seq = Dseq::new();
//! let notes = [0i8, 3, 7];
//! let picked: Vec<i8> = (0..4).map(|_| seq.next(&notes)).collect();
//! assert_eq!(picked, [0, 3, 7, 0]);
//! ```

use crate::rng::Rng;

/// Random trigger (SC Dust): fires with probability `density_hz * dt` per step.
pub struct Dust {
    rng: Rng,
}

impl Dust {
    /// Creates the trigger; `rng` supplies the random numbers.
    pub fn new(rng: Rng) -> Dust {
        Dust { rng }
    }

    /// Returns true with probability `min(density_hz * dt, 1)`; `density_hz` is the mean trigger
    /// rate in Hz, `dt` the step in seconds. `density_hz <= 0` or NaN never fires.
    #[inline(always)]
    pub fn tick(&mut self, density_hz: f32, dt: f32) -> bool {
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "NaN density must return false; keeps exact semantics")]
        let silent = !(density_hz > 0.0);
        if silent {
            return false;
        }
        let p = (density_hz * dt).min(1.0);
        self.rng.f32() < p
    }
}

/// Periodic trigger (SC Impulse): fires on the first call, then every `1/hz` s.
pub struct Impulse {
    phase: f32,
}

impl Impulse {
    /// Creates the trigger, primed so the first `tick` fires.
    pub fn new() -> Impulse {
        Impulse { phase: 1.0 }
    }

    /// Advances by one step of `dt` seconds at rate `hz` (Hz) and returns whether it fired. Fires at
    /// most once per call, so `hz * dt >= 1` fires every call; the period is quantised to
    /// whole steps. `hz <= 0` stops it after the first (priming) trigger.
    #[inline(always)]
    pub fn tick(&mut self, hz: f32, dt: f32) -> bool {
        let fired = self.phase >= 1.0;
        if fired {
            self.phase -= 1.0;
        }
        if hz > 0.0 {
            self.phase = (self.phase + hz * dt).min(1.0);
        }
        fired
    }
}

impl Default for Impulse {
    fn default() -> Self {
        Self::new()
    }
}

/// Cycles through a slice (SC Dseq with infinite repeats).
pub struct Dseq {
    idx: usize,
}

impl Dseq {
    /// Creates a sequencer positioned at the start.
    pub fn new() -> Dseq {
        Dseq { idx: 0 }
    }

    /// Returns the next element of `seq`, wrapping at the end. An empty slice gives 0. The slice
    /// may differ between calls; the position restarts at 0 if it is past the end of the one given.
    #[inline(always)]
    pub fn next(&mut self, seq: &[i8]) -> i8 {
        if seq.is_empty() {
            return 0;
        }
        if self.idx >= seq.len() {
            self.idx = 0;
        }
        let v = seq[self.idx];
        self.idx += 1;
        if self.idx >= seq.len() {
            self.idx = 0;
        }
        v
    }

    /// Restarts from the first element.
    pub fn reset(&mut self) {
        self.idx = 0;
    }
}

impl Default for Dseq {
    fn default() -> Self {
        Self::new()
    }
}

/// Uniform random index in `0..n`, never repeating the previous one (SC Dxrand).
pub struct Dxrand {
    rng: Rng,
    n: usize,
    last: usize,
}

impl Dxrand {
    /// Creates a picker over indices `0..n`. `n <= 1` always yields 0.
    pub fn new(rng: Rng, n: usize) -> Dxrand {
        Dxrand { rng, n, last: usize::MAX }
    }

    /// Returns the next index; for `n > 1` it differs from the previous result.
    #[inline(always)]
    pub fn pick(&mut self) -> usize {
        let n = self.n;
        if n <= 1 {
            return 0;
        }
        let r = if self.last >= n {
            ((self.rng.f32() * n as f32) as usize).min(n - 1)
        } else {
            let mut r = ((self.rng.f32() * (n - 1) as f32) as usize).min(n - 2);
            if r >= self.last {
                r += 1;
            }
            r
        };
        self.last = r;
        r
    }
}

/// Picks a random item from a slice (SC TChoose).
pub struct TChoose {
    rng: Rng,
}

impl TChoose {
    /// Creates the picker; `rng` supplies the random numbers.
    pub fn new(rng: Rng) -> TChoose {
        TChoose { rng }
    }

    /// Returns a uniformly chosen element of `items`; an empty slice gives 0.0.
    #[inline(always)]
    pub fn pick(&mut self, items: &[f32]) -> f32 {
        if items.is_empty() {
            return 0.0;
        }
        let i = ((self.rng.f32() * items.len() as f32) as usize).min(items.len() - 1);
        items[i]
    }
}

/// Delays a trigger by `delay_s`; triggers arriving while one is pending are ignored (SC TDelay).
pub struct TDelay {
    delay_s: f32,
    remaining: f32,
    pending: bool,
}

impl TDelay {
    /// Creates a delay of `delay_s` seconds.
    pub fn new(delay_s: f32) -> TDelay {
        TDelay { delay_s, remaining: 0.0, pending: false }
    }

    /// Advances by `dt` seconds with input trigger `trig`; returns true on the step the delayed
    /// trigger comes out. The output is at least one step after the input (even for
    /// `delay_s` = 0), and a `trig` on the step that fires is ignored.
    #[inline(always)]
    pub fn tick(&mut self, trig: bool, dt: f32) -> bool {
        if self.pending {
            self.remaining -= dt;
            if self.remaining <= 0.0 {
                self.pending = false;
                return true;
            }
        } else if trig {
            self.pending = true;
            self.remaining = self.delay_s;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 32.0 / 48_000.0;

    #[test]
    fn dust_rate_matches_density() {
        let mut d = Dust::new(Rng::new(7));
        let n = (0..1_000_000).filter(|_| d.tick(10.0, DT)).count() as f32;
        let expect = 10.0 * 1e6 * DT;
        assert!((n - expect).abs() < 0.1 * expect, "{n} vs {expect}");
        let mut z = Dust::new(Rng::new(7));
        assert_eq!((0..10_000).filter(|_| z.tick(0.0, DT)).count(), 0);
        let mut a = Dust::new(Rng::new(7));
        let mut b = Dust::new(Rng::new(7));
        for _ in 0..10_000 {
            assert_eq!(a.tick(10.0, DT), b.tick(10.0, DT));
        }
    }

    #[test]
    fn impulse_fires_first_then_periodic() {
        let mut i = Impulse::new();
        assert!(i.tick(2.0, DT));
        let steps = (10.0 / DT) as usize;
        let mut fires = 1;
        let mut prev = true;
        for _ in 0..steps {
            let f = i.tick(2.0, DT);
            assert!(!(f && prev));
            fires += f as usize;
            prev = f;
        }
        assert!(fires == 20 || fires == 21, "{fires}");
        let mut z = Impulse::new();
        assert_eq!((0..1000).filter(|_| z.tick(0.0, DT)).count(), 1);
    }

    #[test]
    fn dseq_cycles_and_resets() {
        let mut d = Dseq::new();
        let s = [1i8, 2, 3];
        assert_eq!([d.next(&s), d.next(&s), d.next(&s), d.next(&s)], [1, 2, 3, 1]);
        d.reset();
        assert_eq!(d.next(&s), 1);
        assert_eq!(d.next(&[]), 0);
    }

    #[test]
    fn dxrand_never_repeats() {
        let mut d = Dxrand::new(Rng::new(7), 4);
        let mut seen = [false; 4];
        let mut prev = usize::MAX;
        for _ in 0..10_000 {
            let v = d.pick();
            assert!(v < 4 && v != prev);
            seen[v] = true;
            prev = v;
        }
        assert!(seen.iter().all(|&s| s));
        let mut one = Dxrand::new(Rng::new(7), 1);
        assert!((0..100).all(|_| one.pick() == 0));
        assert_eq!(Dxrand::new(Rng::new(7), 0).pick(), 0);
    }

    #[test]
    fn tchoose_picks_from_items() {
        let items = [0.5f32, 1.5, 2.5, 7.0];
        let mut t = TChoose::new(Rng::new(7));
        for _ in 0..1000 {
            assert!(items.contains(&t.pick(&items)));
        }
        assert_eq!(t.pick(&[]), 0.0);
    }

    #[test]
    fn tdelay_delays_and_ignores_retrigger() {
        let mut t = TDelay::new(3.0);
        let want = (3.0 / DT).round() as i64;
        let retrig = (1.0 / DT) as usize;
        let mut pulses = alloc::vec::Vec::new();
        for i in 0..(want as usize + 200) {
            let trig = i == 0 || i == retrig;
            if t.tick(trig, DT) {
                pulses.push(i as i64);
            }
        }
        assert_eq!(pulses.len(), 1, "{pulses:?}");
        assert!((pulses[0] - want).abs() <= 1, "{} vs {want}", pulses[0]);
        assert!(!t.tick(false, DT));
        // trigger after the pulse starts a new delay
        let mut got = None;
        for i in 0..(want as usize + 10) {
            if t.tick(i == 0, DT) {
                got = Some(i as i64);
            }
        }
        assert!((got.unwrap() - want).abs() <= 1);
    }
}
