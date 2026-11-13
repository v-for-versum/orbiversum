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
//!
//! # Threads
//!
//! * UI thread: owns the [`Dispatcher`]. It calls `select` (any time) and
//!   `service` (every UI tick). Applet `start` and `stop` run here.
//! * Audio thread: owns the [`AudioPath`], obtained once with
//!   `Dispatcher::take_audio` and moved into the board's render callback
//!   (on the JC3636K718C firmware: `board.audio_out().start(sr, Box::new(move |out| path.audio(out)))`,
//!   run by the audio task of the board's HAL). It calls `AudioPath::audio` once per block.
//!
//! # State machine
//!
//! ```text
//!            select(i) seen by audio, fade-out done
//!   RUNNING ------------------------------------------> PARKED
//!      ^                                                   |
//!      |  audio sees READY, starts fade-in                 | service(): stop old, start new,
//!      |                                                   |            write `active`
//!      +------------------------ READY <-------------------+
//! ```
//!
//! Only the `phase` stores that hand a slot over are `Release` (and loaded
//! with `Acquire` by the receiving side): `audio` stores `PARKED` and `service`
//! stores `READY`. The `READY -> RUNNING` store by the audio thread is
//! `Relaxed`, because nothing is handed over by it. `select`, `target` and
//! `fade` are `Relaxed` too: they are plain requests, not ownership.
//!
//! While `PARKED` (and until `service` runs) `audio` writes silence, so a
//! switch costs the fade-out, one or more silent blocks (until the next
//! `service` tick) and the fade-in. A `select` that arrives while the audio
//! path is not steady is picked up when it is: `service` reads the latest
//! `target`, so the last request wins.
//!
//! # What is allowed on the audio thread
//!
//! `AudioPath::audio` and the `Applet::audio` it calls: no allocation, no
//! freeing, no locks, no blocking. The only shared-memory operations are
//! atomic loads/stores and a refcount increment at the start of each
//! `AudioPath::audio` call and the matching decrement at its end, on the
//! internal `Arc`. The decrement would free `Shared` (applets and `Params`) if
//! it were the last reference, so the `Dispatcher` must outlive the audio
//! path; the firmware keeps it alive for the whole run. Anything that allocates
//! or frees applet memory belongs in `Applet::start` / `Applet::stop`, which
//! only run on the UI thread inside `service`.
use crate::applet::{Applet, AppletInfo};
use crate::params::Params;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Sample rate the applets are built for and the default fade length is
/// computed from, Hz. The firmware passes it to both the applets and the board's `AudioOut`.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// 20 ms at the default sample rate: `48_000 / 50 = 960` samples.
const DEFAULT_FADE: usize = (DEFAULT_SAMPLE_RATE as usize) / 50;

/// The audio thread owns the active slot.
const RUNNING: u8 = 0;
/// The audio thread gave the active slot back (applet faded out); the UI thread owns it.
const PARKED: u8 = 1;
/// The UI thread has started the new applet; the audio thread may take over.
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
    /// Fade length in samples, for both fade-out and fade-in.
    fade: AtomicUsize,
    params: Arc<Params>,
}

// SAFETY (Sync; Send is automatic): a slot is accessed by at most one thread at a time: the audio thread
// while `phase == RUNNING`, the UI thread while `phase == PARKED`; the
// hand-offs are Release stores / Acquire loads of `phase`.
unsafe impl Sync for Shared {}

/// UI-thread half of the dispatcher: owns the applets, requests switches and
/// completes them in `service`. Create it with `new` or `with_params`, take the
/// audio half once with `take_audio`.
pub struct Dispatcher {
    shared: Arc<Shared>,
    taken: bool,
}

/// What the audio thread is doing with the current applet. The number is the
/// count of samples of the fade already done.
#[derive(Clone, Copy)]
enum Mode {
    /// Running at full level.
    Steady,
    /// Gain ramps up from 0 to 1.
    FadeIn(usize),
    /// Gain ramps down from 1 to 0; at the end the slot is given back (`PARKED`).
    FadeOut(usize),
    /// Silence until the UI thread sets `READY`.
    Parked,
}

/// Audio-thread half of the dispatcher (`Send`, move it into the render closure).
pub struct AudioPath {
    shared: Arc<Shared>,
    cur: usize,
    mode: Mode,
}

impl Dispatcher {
    /// Starts applet 0 with default `Params`. Panics on an empty list.
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

    /// Always `false`: the dispatcher needs at least one applet.
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

    /// The `Params` shared by the applets' audio and UI halves.
    pub fn params(&self) -> Arc<Params> {
        self.shared.params.clone()
    }
}

impl AudioPath {
    /// Renders one block into `out` (overwrites it). Realtime: no locks, no
    /// allocation, no blocking.
    ///
    /// Called by the board's audio task once per block (the render callback
    /// holds the `AudioPath`). `out` may have any length: fades are sample
    /// accurate and span block boundaries. Steady state it just forwards to the
    /// current applet; on a pending `select` it fades the applet out over
    /// `fade` samples, parks, writes silence until `service` sets `READY`, then
    /// fades the new applet in (the audio side of the hand-over described in the
    /// module docs). On xtensa the function is placed in IRAM.
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
