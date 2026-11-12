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
/// Worst block since the last `take`: total ticks and its per-section split.
static WORST_TOTAL: AtomicU32 = AtomicU32::new(0);
static WORST: [AtomicU32; SECTIONS] = [const { AtomicU32::new(0) }; SECTIONS];
/// Addresses of buffers noted by voices (slot meaning is up to the caller).
pub const NOTES: usize = 4;
static ADDRS: [AtomicUsize; NOTES] = [const { AtomicUsize::new(0) }; NOTES];

/// Remembers where a buffer lives so the firmware can report internal RAM vs PSRAM.
pub fn note_addr(slot: usize, ptr: *const u8) {
    ADDRS[slot].store(ptr as usize, Ordering::Relaxed);
}

pub fn addrs() -> [usize; NOTES] {
    let mut o = [0; NOTES];
    for (a, s) in o.iter_mut().zip(ADDRS.iter()) {
        *a = s.load(Ordering::Relaxed);
    }
    o
}

/// Worst block (total ticks, per-section ticks) since the last call; resets it.
pub fn take_worst() -> (u32, [u32; SECTIONS]) {
    let mut o = [0u32; SECTIONS];
    for (a, s) in o.iter_mut().zip(WORST.iter()) {
        *a = s.load(Ordering::Relaxed);
    }
    (WORST_TOTAL.swap(0, Ordering::Relaxed), o)
}

/// Installs a monotonic tick source (wrapping u32).
pub fn set_clock(f: fn() -> u32) {
    CLOCK.store(f as usize, Ordering::Relaxed);
}

/// Current tick. On the ESP32-S3 this is the CPU cycle counter read inline
/// (`rsr.ccount`, no call: a call into flash code can stall behind the flash cache);
/// elsewhere the installed clock, or 0.
#[inline(always)]
pub fn now() -> u32 {
    #[cfg(target_arch = "xtensa")]
    {
        let v: u32;
        // SAFETY: reads a special register, no memory or side effects.
        unsafe { core::arch::asm!("rsr.ccount {0}", out(reg) v, options(nomem, nostack, preserves_flags)) };
        v
    }
    #[cfg(not(target_arch = "xtensa"))]
    {
        let c = CLOCK.load(Ordering::Relaxed);
        if c == 0 {
            0
        } else {
            // SAFETY: only `set_clock` stores here, and it stores a valid `fn() -> u32`.
            let f: fn() -> u32 = unsafe { core::mem::transmute::<usize, fn() -> u32>(c) };
            f()
        }
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
        #[cfg(not(target_arch = "xtensa"))]
        if CLOCK.load(Ordering::Relaxed) == 0 {
            return;
        }
        let total = self.acc.iter().fold(0u32, |a, b| a.wrapping_add(*b));
        if total > WORST_TOTAL.load(Ordering::Relaxed) {
            // single writer (the audio thread); readers may see a torn split, fine for diagnostics
            WORST_TOTAL.store(total, Ordering::Relaxed);
            for (a, w) in self.acc.iter().zip(WORST.iter()) {
                w.store(*a, Ordering::Relaxed);
            }
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
