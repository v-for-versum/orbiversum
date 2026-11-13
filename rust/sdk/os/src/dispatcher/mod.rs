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
mod tests;
