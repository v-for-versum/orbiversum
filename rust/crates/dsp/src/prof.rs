//! Per-section timing of the audio path (optional, off until a clock is set).
//!
//! The firmware installs a tick counter with `set_clock`; voices time whole
//! sections (a handful of reads per block, never per sample), accumulate in
//! locals and publish once per block. A UI thread reads and resets the totals
//! with `take`.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub const SECTIONS: usize = 8;
pub const CONTROL: usize = 0;
pub const SAW: usize = 1;
pub const LPF: usize = 2;
pub const DELAY: usize = 3;
pub const MIX: usize = 4;
pub const MOOG: usize = 5;
pub const HPF: usize = 6;
pub const OTHER: usize = 7;
/// Section names, same order as the totals.
pub const NAMES: [&str; SECTIONS] = ["control", "saw", "lpf", "delay", "mix", "moog", "hpf", "sub+other"];

static CLOCK: AtomicUsize = AtomicUsize::new(0);
static TOTALS: [AtomicU32; SECTIONS] = [const { AtomicU32::new(0) }; SECTIONS];
static BLOCKS: AtomicU32 = AtomicU32::new(0);

/// Installs a monotonic tick source (wrapping u32).
pub fn set_clock(f: fn() -> u32) {
    CLOCK.store(f as usize, Ordering::Relaxed);
}

#[inline]
pub fn now() -> u32 {
    let c = CLOCK.load(Ordering::Relaxed);
    if c == 0 {
        0
    } else {
        // SAFETY: only `set_clock` stores here, and it stores a valid `fn() -> u32`.
        let f: fn() -> u32 = unsafe { core::mem::transmute::<usize, fn() -> u32>(c) };
        f()
    }
}

/// Local accumulator for one `process` call.
pub struct Block {
    acc: [u32; SECTIONS],
    t: u32,
}

impl Block {
    #[inline]
    pub fn start() -> Block {
        Block { acc: [0; SECTIONS], t: now() }
    }

    /// Charges the time since the previous `mark` to `section`.
    #[inline]
    pub fn mark(&mut self, section: usize) {
        let n = now();
        self.acc[section] = self.acc[section].wrapping_add(n.wrapping_sub(self.t));
        self.t = n;
    }

    pub fn finish(self) {
        if CLOCK.load(Ordering::Relaxed) == 0 {
            return;
        }
        for (a, t) in self.acc.iter().zip(TOTALS.iter()) {
            t.fetch_add(*a, Ordering::Relaxed);
        }
        BLOCKS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Returns (blocks, per-section tick totals) since the last call and resets them.
pub fn take() -> (u32, [u32; SECTIONS]) {
    let mut out = [0u32; SECTIONS];
    for (o, t) in out.iter_mut().zip(TOTALS.iter()) {
        *o = t.swap(0, Ordering::Relaxed);
    }
    (BLOCKS.swap(0, Ordering::Relaxed), out)
}
