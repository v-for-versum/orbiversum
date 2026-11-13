//! Per-section timing of the audio path (optional, off until a clock is set).
//!
//! The firmware installs a tick counter with `set_clock`; voices time whole
//! sections (a handful of reads per block, never per sample), accumulate in
//! locals and publish once per block. A UI thread reads and resets the totals
//! with `take`.
//!
//! Audio-thread cost per block: two clock reads per `mark` plus about a dozen relaxed atomic
//! operations in `Block::finish`; on non-Xtensa targets without an installed clock
//! `finish` returns immediately. Everything is static: no allocation, no locks. The state is
//! global, so there is one profile per process.
//!
//! ```
//! use orbiversum_core::prof;
//!
//! // Without a clock the host build records nothing.
//! let mut b = prof::Block::start();
//! b.mark(prof::SAW);
//! b.finish();
//! assert_eq!(prof::now(), 0);
//! ```

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Number of timing sections.
pub const SECTIONS: usize = 8;
/// Section index: control-rate work.
pub const CONTROL: usize = 0;
/// Section index: sawtooth oscillators.
pub const SAW: usize = 1;
/// Section index: lowpass filters.
pub const LPF: usize = 2;
/// Section index: delay lines.
pub const DELAY: usize = 3;
/// Section index: mixing.
pub const MIX: usize = 4;
/// Section index: Moog ladder.
pub const MOOG: usize = 5;
/// Section index: highpass filters.
pub const HPF: usize = 6;
/// Section index: everything else (sub oscillator and other).
pub const OTHER: usize = 7;
/// Section names, same order as the totals.
pub const NAMES: [&str; SECTIONS] = ["control", "saw", "lpf", "delay", "mix", "moog", "hpf", "sub+other"];

/// Names of the active voice's sections (address of a `'static` array; 0 = `NAMES`).
static ACTIVE_NAMES: AtomicUsize = AtomicUsize::new(0);

/// Selects the section names the firmware prints (the voice calls this from `process`).
pub fn set_names(names: &'static [&'static str; SECTIONS]) {
    ACTIVE_NAMES.store(names as *const _ as usize, Ordering::Relaxed);
}

/// Names of the voice that ran last (default `NAMES`).
pub fn names() -> &'static [&'static str; SECTIONS] {
    let p = ACTIVE_NAMES.load(Ordering::Relaxed);
    if p == 0 {
        &NAMES
    } else {
        // SAFETY: only `set_names` stores here, and it stores the address of a `'static` array.
        unsafe { &*(p as *const [&'static str; SECTIONS]) }
    }
}

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
/// `slot` must be < [`NOTES`] (panics otherwise).
pub fn note_addr(slot: usize, ptr: *const u8) {
    ADDRS[slot].store(ptr as usize, Ordering::Relaxed);
}

/// The addresses stored by [`note_addr`], one per slot (0 = not set).
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

/// Installs a monotonic tick source (wrapping u32), used by [`now`] on targets other than Xtensa.
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
    /// Starts a block: all sections at zero, clock read now.
    #[inline]
    pub fn start() -> Block {
        Block { acc: [0; SECTIONS], t: now() }
    }

    /// Charges the time since the previous `mark` (or `start`) to `section` (an index < [`SECTIONS`],
    /// e.g. [`SAW`]; panics otherwise). Wrapping `u32` ticks.
    #[inline]
    pub fn mark(&mut self, section: usize) {
        let n = now();
        self.acc[section] = self.acc[section].wrapping_add(n.wrapping_sub(self.t));
        self.t = n;
    }

    /// Publishes this block: adds the per-section ticks to the totals read by [`take`], counts the
    /// block, and records it as the worst block if its total is the largest since the last
    /// [`take_worst`]. Does nothing on non-Xtensa targets until [`set_clock`] was called.
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

#[cfg(test)]
mod tests {
    use super::*;

    static OTHER_NAMES: [&str; SECTIONS] = ["a", "b", "c", "d", "e", "f", "g", "h"];

    #[test]
    fn names_default_and_switch() {
        assert_eq!(names(), &NAMES);
        set_names(&OTHER_NAMES);
        assert_eq!(names(), &OTHER_NAMES);
    }
}
