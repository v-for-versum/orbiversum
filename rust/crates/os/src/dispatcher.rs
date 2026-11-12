//! Dispatcher: lock-free applet switching between the UI and audio threads.
//!
//! Design. All applets live in `Shared` slots. A `phase` atomic is the single
//! ownership token for the *active* slot:
//!
//! * `RUNNING`: the audio thread owns the active slot (and only it).
//! * `PARKED`: the audio thread has faded the active applet out and gave it
//!   back; it outputs silence. The UI thread (`service`) owns the slots.
//! * `READY`: the UI thread has stopped the old applet, started the new one
//!   and written `active`; the audio thread picks it up and fades in.
//!
//! `select` only stores the requested index. `audio` notices it at a block
//! boundary, fades the old applet out (`fade` samples, sample accurate), then
//! parks. `service` (UI thread, call it every UI tick) does `stop` + `start`,
//! so every allocation and free of applet memory happens on the UI thread.
//! `audio` takes no locks, never allocates or frees and never blocks.
use crate::applet::{Applet, AppletInfo};
use crate::params::Params;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// ~20 ms at the default sample rate.
const DEFAULT_FADE: usize = (DEFAULT_SAMPLE_RATE as usize) / 50;

const RUNNING: u8 = 0;
const PARKED: u8 = 1;
const READY: u8 = 2;

struct Shared {
    slots: Vec<UnsafeCell<Box<dyn Applet + Send>>>,
    infos: Vec<AppletInfo>,
    phase: AtomicU8,
    /// Applet that is (or is about to be) running; written by the UI thread
    /// while `phase == PARKED`, read by audio after seeing `READY`.
    active: AtomicUsize,
    /// Last valid request from `select`.
    target: AtomicUsize,
    fade: AtomicUsize,
    params: Arc<Params>,
}

// SAFETY (Sync; Send is automatic): a slot is accessed by at most one thread at a time: the audio thread
// while `phase == RUNNING`, the UI thread while `phase == PARKED`; the
// hand-offs are Release stores / Acquire loads of `phase`.
unsafe impl Sync for Shared {}

pub struct Dispatcher {
    shared: Arc<Shared>,
    taken: bool,
}

#[derive(Clone, Copy)]
enum Mode {
    Steady,
    FadeIn(usize),
    FadeOut(usize),
    Parked,
}

/// Audio-thread half of the dispatcher (`Send`, move it into the render closure).
pub struct AudioPath {
    shared: Arc<Shared>,
    cur: usize,
    mode: Mode,
}

impl Dispatcher {
    /// Starts applet 0. Panics on an empty list.
    pub fn new(applets: Vec<Box<dyn Applet + Send>>) -> Dispatcher {
        Self::with_params(applets, Arc::new(Params::default()))
    }

    /// Like `new`, with the `Params` the applets' audio and UI halves share
    /// (the dispatcher only hands it out via `params()`).
    pub fn with_params(applets: Vec<Box<dyn Applet + Send>>, params: Arc<Params>) -> Dispatcher {
        assert!(!applets.is_empty(), "Dispatcher needs at least one applet");
        let infos: Vec<AppletInfo> = applets.iter().map(|a| a.info()).collect();
        let mut slots: Vec<_> = applets.into_iter().map(UnsafeCell::new).collect();
        slots[0].get_mut().start();
        Dispatcher {
            shared: Arc::new(Shared {
                slots,
                infos,
                phase: AtomicU8::new(RUNNING),
                active: AtomicUsize::new(0),
                target: AtomicUsize::new(0),
                fade: AtomicUsize::new(DEFAULT_FADE),
                params,
            }),
            taken: false,
        }
    }

    /// Fade length in samples (default ~20 ms at 48 kHz). Tests use 0 to disable fades.
    pub fn set_fade_samples(&mut self, n: usize) {
        self.shared.fade.store(n, Ordering::Relaxed);
    }

    /// The audio half; can be taken once.
    pub fn take_audio(&mut self) -> AudioPath {
        assert!(!self.taken, "audio path already taken");
        self.taken = true;
        AudioPath { shared: self.shared.clone(), cur: 0, mode: Mode::Steady }
    }

    /// Requests applet `idx`; out of range is ignored. Takes effect at the
    /// next audio block, completed by `service`.
    pub fn select(&mut self, idx: usize) {
        if idx < self.shared.slots.len() {
            self.shared.target.store(idx, Ordering::Relaxed);
        }
    }

    /// UI-thread tick. **Must be called every UI tick**: the switch only
    /// completes here, and the output stays silent until it does.
    /// If the audio path has faded the old applet out, stop it
    /// and start the requested one (this is where memory is freed/allocated).
    pub fn service(&mut self) {
        let s = &*self.shared;
        if s.phase.load(Ordering::Acquire) != PARKED {
            return;
        }
        let old = s.active.load(Ordering::Relaxed);
        let new = s.target.load(Ordering::Relaxed);
        // SAFETY: phase == PARKED, so the audio thread does not touch any slot.
        unsafe {
            (*s.slots[old].get()).stop();
            (*s.slots[new].get()).start();
        }
        s.active.store(new, Ordering::Relaxed);
        s.phase.store(READY, Ordering::Release);
    }

    /// Number of applets.
    pub fn len(&self) -> usize {
        self.shared.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// Cached `AppletInfo` of applet `idx` (for menus); `None` out of range.
    pub fn info(&self, idx: usize) -> Option<AppletInfo> {
        self.shared.infos.get(idx).copied()
    }

    /// Most recently requested (valid) applet.
    pub fn selected(&self) -> usize {
        self.shared.target.load(Ordering::Relaxed)
    }

    pub fn params(&self) -> Arc<Params> {
        self.shared.params.clone()
    }
}

impl AudioPath {
    /// Realtime: no locks, no allocation, no blocking. Overwrites `out`.
    #[cfg_attr(target_arch = "xtensa", link_section = ".iram1.audio_path")]
    pub fn audio(&mut self, out: &mut [[f32; 2]]) {
        let shared = self.shared.clone(); // refcount bump only: no allocation
        let s = &*shared;
        let fade = s.fade.load(Ordering::Relaxed);
        if matches!(self.mode, Mode::Steady) && s.target.load(Ordering::Relaxed) != self.cur {
            self.mode = Mode::FadeOut(0);
        }
        let mut i = 0;
        while i < out.len() {
            match self.mode {
                Mode::Steady => {
                    self.applet().audio(&mut out[i..]);
                    i = out.len();
                }
                Mode::FadeIn(p) => {
                    if p >= fade {
                        self.mode = Mode::Steady;
                        continue;
                    }
                    let m = (fade - p).min(out.len() - i);
                    let seg = &mut out[i..i + m];
                    self.applet().audio(seg);
                    for (k, f) in seg.iter_mut().enumerate() {
                        let g = (p + k + 1) as f32 / fade as f32;
                        f[0] *= g;
                        f[1] *= g;
                    }
                    i += m;
                    self.mode = Mode::FadeIn(p + m);
                }
                Mode::FadeOut(p) => {
                    if p >= fade {
                        // Give the applet back to the UI thread; silence until READY.
                        self.mode = Mode::Parked;
                        s.phase.store(PARKED, Ordering::Release);
                        continue;
                    }
                    let m = (fade - p).min(out.len() - i);
                    let seg = &mut out[i..i + m];
                    self.applet().audio(seg);
                    for (k, f) in seg.iter_mut().enumerate() {
                        let g = 1.0 - (p + k + 1) as f32 / fade as f32;
                        f[0] *= g;
                        f[1] *= g;
                    }
                    i += m;
                    self.mode = Mode::FadeOut(p + m);
                    if p + m >= fade {
                        // Park right away so `service` need not wait for the next block.
                        self.mode = Mode::Parked;
                        s.phase.store(PARKED, Ordering::Release);
                    }
                }
                Mode::Parked => {
                    if s.phase.load(Ordering::Acquire) == READY {
                        self.cur = s.active.load(Ordering::Relaxed);
                        s.phase.store(RUNNING, Ordering::Relaxed);
                        self.mode = Mode::FadeIn(0);
                    } else {
                        out[i..].fill([0.0; 2]);
                        i = out.len();
                    }
                }
            }
        }
    }

    fn applet(&mut self) -> &mut (dyn Applet + Send) {
        // SAFETY: only called in modes where phase == RUNNING (audio owns the slot).
        unsafe { &mut **self.shared.slots[self.cur].get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applet::{AppletInfo, Dirty};
    use core::alloc::{GlobalAlloc, Layout};
    use core::cell::Cell;
    use embedded_graphics::prelude::{Point, Size};
    use embedded_graphics::primitives::Rectangle;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::alloc::System;

    // Per-thread allocation accounting (tests run in parallel threads).
    thread_local! {
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static ALLOCS: Cell<usize> = const { Cell::new(0) };
    }
    struct Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let _ = LIVE.try_with(|c| c.set(c.get() + l.size() as isize));
            let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
            System.alloc(l)
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            let _ = LIVE.try_with(|c| c.set(c.get() - l.size() as isize));
            System.dealloc(p, l)
        }
    }
    #[global_allocator]
    static A: Counting = Counting;
    fn live() -> isize {
        LIVE.with(|c| c.get())
    }
    fn allocs() -> usize {
        ALLOCS.with(|c| c.get())
    }

    const SR: f32 = 48_000.0;
    const BUF: usize = 4096;

    /// Sine voice; allocates a buffer in `start`, frees it in `stop`.
    struct Mock {
        hz: f32,
        amp: f32,
        phase: f32,
        buf: Vec<u8>,
    }
    impl Mock {
        fn boxed(hz: f32, amp: f32) -> Box<dyn Applet + Send> {
            Box::new(Mock { hz, amp, phase: 0.0, buf: Vec::new() })
        }
    }
    impl Applet for Mock {
        fn info(&self) -> AppletInfo {
            AppletInfo { name: "mock" }
        }
        fn start(&mut self) {
            self.buf = vec![0u8; BUF];
            self.phase = 0.0;
        }
        fn audio(&mut self, out: &mut [[f32; 2]]) {
            for f in out {
                let s = self.amp * (self.phase * core::f32::consts::TAU).sin();
                self.phase += self.hz / SR;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                *f = [s, s];
            }
        }
        fn stop(&mut self) {
            self.buf = Vec::new();
        }
    }

    /// Constant DC applet: a switch without fade makes a big step.
    struct Dc(f32);
    impl Applet for Dc {
        fn info(&self) -> AppletInfo {
            AppletInfo { name: "dc" }
        }
        fn start(&mut self) {}
        fn audio(&mut self, out: &mut [[f32; 2]]) {
            out.fill([self.0, self.0]);
        }
        fn stop(&mut self) {}
    }

    fn render(a: &mut AudioPath, blocks: usize, bs: usize) -> Vec<[f32; 2]> {
        let mut all = Vec::new();
        let mut b = vec![[0.0f32; 2]; bs];
        for _ in 0..blocks {
            a.audio(&mut b);
            all.extend_from_slice(&b);
        }
        all
    }

    fn max_step(s: &[[f32; 2]]) -> f32 {
        s.windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0, f32::max)
    }

    fn switch_run(fade: Option<usize>) -> Vec<[f32; 2]> {
        let mut d = Dispatcher::new(vec![Box::new(Dc(0.8)), Box::new(Dc(-0.8))]);
        if let Some(n) = fade {
            d.set_fade_samples(n);
        }
        let mut a = d.take_audio();
        let mut out = render(&mut a, 8, 64);
        d.select(1);
        out.extend(render(&mut a, 40, 64));
        d.service();
        out.extend(render(&mut a, 40, 64));
        out
    }

    #[test]
    fn switch_does_not_click() {
        let out = switch_run(None);
        // Reached the new level and left the old one.
        assert!((out[0][0] - 0.8).abs() < 1e-6);
        assert!((out.last().unwrap()[0] + 0.8).abs() < 1e-6);
        // 20 ms fade down and up: per-sample step is at most 0.8/960 plus slack.
        assert!(max_step(&out) < 0.005, "max step {}", max_step(&out));
    }

    #[test]
    fn switch_without_fade_would_click() {
        // Guards the click test: with zero-length fades the same harness sees a jump.
        let out = switch_run(Some(0));
        assert!(max_step(&out) > 0.5, "max step {}", max_step(&out));
    }

    #[test]
    fn switch_frees_old_applet_memory() {
        let base = live();
        let mut d = Dispatcher::new(vec![Mock::boxed(220.0, 0.5), Mock::boxed(330.0, 0.5)]);
        let mut a = d.take_audio();
        let one_started = live() - base;
        let mut b = [[0.0f32; 2]; 128];
        a.audio(&mut b);
        d.select(1);
        // Audio thread must never allocate or free: count across the whole switch.
        let (al0, l0) = (allocs(), live());
        for _ in 0..20 {
            a.audio(&mut b);
        }
        assert_eq!(allocs(), al0, "audio() allocated");
        assert_eq!(live(), l0, "audio() freed or allocated memory");
        d.service();
        // Old applet stopped (buffer freed), new one running: same footprint as before.
        assert_eq!(live() - base, one_started);
        assert_eq!(d.selected(), 1);
        drop(a);
        drop(d);
        assert_eq!(live(), base, "leak after dropping dispatcher");
    }

    #[test]
    fn audio_independent_of_block_size() {
        let mk = || {
            let mut d = Dispatcher::new(vec![Mock::boxed(440.0, 0.5)]);
            let a = d.take_audio();
            (d, a)
        };
        let (_d1, mut a1) = mk();
        let (_d2, mut a2) = mk();
        let x = render(&mut a1, 24, 128);
        let mut y = render(&mut a2, 24, 1);
        y.extend(render(&mut a2, 1, 3072 - 24));
        let z = &y[..3072];
        assert_eq!(x.len(), 3072);
        assert_eq!(&x[..], z);
        // Also across a switch: fade-out ends mid-block (960 < 1000), `service`
        // runs, then the new applet fades in. Block sizes divide 1000 and 4000.
        let run = |bs: usize| {
            let mut d = Dispatcher::new(vec![Mock::boxed(440.0, 0.5), Mock::boxed(300.0, 0.5)]);
            let mut a = d.take_audio();
            let mut o = render(&mut a, 1000 / bs, bs);
            d.select(1);
            o.extend(render(&mut a, 1000 / bs, bs));
            d.service();
            o.extend(render(&mut a, 4000 / bs, bs));
            o
        };
        let r40 = run(40);
        assert_eq!(r40, run(100));
        assert_eq!(r40, run(8));
        // Non-vacuous: fade-in really happened (silence, then a ramp, then 300 Hz at 0.5).
        let fade_in = &r40[2000..2000 + 960];
        assert!(fade_in.iter().any(|f| f[0].abs() > 0.01), "no fade-in after service");
        assert!(r40[2000..].iter().map(|f| f[0].abs()).fold(0.0, f32::max) > 0.45);
        assert!(r40[1960..2000].iter().all(|f| *f == [0.0, 0.0]), "gap before service must be silent");
    }

    #[test]
    fn dirty_overflow_becomes_full_screen() {
        let r = Rectangle::new(Point::new(1, 2), Size::new(3, 4));
        let mut dirty = Dirty::new();
        for _ in 0..8 {
            dirty.add(r);
        }
        assert!(!dirty.is_full());
        assert_eq!(dirty.rects().len(), 8);
        dirty.add(r);
        assert!(dirty.is_full());
        assert!(dirty.rects().is_empty());
        dirty.add(r);
        assert!(dirty.is_full());
        dirty.clear();
        assert!(dirty.is_empty());
    }

    #[test]
    fn select_out_of_range_is_ignored() {
        let mut d = Dispatcher::new(vec![Mock::boxed(220.0, 0.5), Mock::boxed(330.0, 0.5)]);
        let mut a = d.take_audio();
        let mut with_bad = render(&mut a, 8, 64);
        d.select(2);
        d.select(usize::MAX);
        with_bad.extend(render(&mut a, 8, 64));
        d.service();
        assert_eq!(d.selected(), 0);
        let mut d2 = Dispatcher::new(vec![Mock::boxed(220.0, 0.5), Mock::boxed(330.0, 0.5)]);
        let mut a2 = d2.take_audio();
        assert_eq!(with_bad, render(&mut a2, 16, 64));
    }

    /// DC applet counting its `start` calls.
    struct Counted(f32, Arc<AtomicUsize>);
    impl Applet for Counted {
        fn info(&self) -> AppletInfo {
            AppletInfo { name: "counted" }
        }
        fn start(&mut self) {
            self.1.fetch_add(1, Ordering::Relaxed);
        }
        fn audio(&mut self, out: &mut [[f32; 2]]) {
            out.fill([self.0, self.0]);
        }
        fn stop(&mut self) {}
    }

    #[test]
    fn rapid_selects_end_on_last() {
        let starts: Vec<_> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();
        let mut d = Dispatcher::new(
            (0..3)
                .map(|i| Box::new(Counted(0.1 * (i + 1) as f32, starts[i].clone())) as Box<dyn Applet + Send>)
                .collect(),
        );
        let mut a = d.take_audio();
        d.select(1);
        d.select(2);
        d.select(0);
        d.select(2);
        let mut out = Vec::new();
        for _ in 0..40 {
            out.extend(render(&mut a, 1, 64));
            d.service();
        }
        // Real output ends on applet 2 at full level; applet 1 was never started.
        assert!((out.last().unwrap()[0] - 0.3).abs() < 1e-6);
        let n: Vec<_> = starts.iter().map(|s| s.load(Ordering::Relaxed)).collect();
        assert_eq!(n, [1, 0, 1]);
    }

    #[test]
    fn service_works_right_after_fade_out_block() {
        // Fade-out ends exactly at a block end: service must already see PARKED.
        let mut d = Dispatcher::new(vec![Box::new(Dc(0.8)), Box::new(Dc(-0.8))]);
        let mut a = d.take_audio();
        render(&mut a, 2, 64);
        d.select(1);
        render(&mut a, 15, 64); // 960 samples = whole fade
        d.service();
        let out = render(&mut a, 20, 64);
        assert!((out.last().unwrap()[0] + 0.8).abs() < 1e-6, "new applet never started");
    }

    #[test]
    fn info_and_len_are_cached() {
        let d = Dispatcher::new(vec![Mock::boxed(1.0, 0.5), Box::new(Dc(0.0))]);
        assert_eq!(d.len(), 2);
        assert_eq!(d.info(0).unwrap().name, "mock");
        assert_eq!(d.info(1).unwrap().name, "dc");
        assert!(d.info(2).is_none());
        let p = Arc::new(Params::new(5.0, 0.25));
        let d = Dispatcher::with_params(vec![Mock::boxed(1.0, 0.5)], p.clone());
        assert!(Arc::ptr_eq(&p, &d.params()));
    }

    /// Applet that panics if its methods overlap or are called out of order.
    struct Guard {
        in_use: Arc<core::sync::atomic::AtomicBool>,
        running: bool,
        audio_calls: Arc<AtomicUsize>,
    }
    impl Guard {
        fn enter(&self) {
            assert!(!self.in_use.swap(true, Ordering::SeqCst), "concurrent applet access");
        }
        fn leave(&self) {
            self.in_use.store(false, Ordering::SeqCst);
        }
    }
    impl Applet for Guard {
        fn info(&self) -> AppletInfo {
            AppletInfo { name: "guard" }
        }
        fn start(&mut self) {
            self.enter();
            assert!(!self.running, "start while running");
            self.running = true;
            self.leave();
        }
        fn audio(&mut self, out: &mut [[f32; 2]]) {
            self.enter();
            assert!(self.running, "audio on a stopped applet");
            out.fill([0.1, 0.1]);
            self.audio_calls.fetch_add(1, Ordering::Relaxed);
            self.leave();
        }
        fn stop(&mut self) {
            self.enter();
            assert!(self.running, "stop while not running");
            self.running = false;
            self.leave();
        }
    }

    /// Hammers the unsafe hand-off: audio thread renders, UI thread selects at
    /// random and ticks `service`. Any overlap or out-of-order call panics.
    #[test]
    fn stress_handoff_never_overlaps() {
        use std::sync::atomic::AtomicBool;
        let iters: usize = if cfg!(miri) { 300 } else { 100_000 };
        let calls = Arc::new(AtomicUsize::new(0));
        let applets: Vec<Box<dyn Applet + Send>> = (0..4)
            .map(|_| {
                Box::new(Guard {
                    in_use: Arc::new(AtomicBool::new(false)),
                    running: false,
                    audio_calls: calls.clone(),
                }) as Box<dyn Applet + Send>
            })
            .collect();
        let mut d = Dispatcher::new(applets);
        d.set_fade_samples(32);
        let mut a = d.take_audio();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let audio = std::thread::spawn(move || {
            let mut b = [[0.0f32; 2]; 16];
            while !stop2.load(Ordering::Relaxed) {
                a.audio(&mut b);
                std::thread::yield_now();
            }
            a
        });
        let mut x: u64 = 0x2545F4914F6CDD1D;
        for _ in 0..iters {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if x.is_multiple_of(3) {
                d.select((x >> 8) as usize % 6); // includes out-of-range
            }
            d.service();
            if x.is_multiple_of(5) {
                std::thread::yield_now();
            }
        }
        stop.store(true, Ordering::Relaxed);
        let a = audio.join().expect("audio thread panicked: hand-off violated");
        assert!(calls.load(Ordering::Relaxed) > 0);
        drop(a);
        drop(d);
    }
}
