//! The `Applet` trait, input events and dirty-rectangle tracking.
use crate::board::Canvas;
use embedded_graphics::primitives::Rectangle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Knob(i32),
    Tap { x: i16, y: i16 },
    Press,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppletInfo {
    pub name: &'static str,
}

pub const MAX_DIRTY: usize = 8;

/// Up to `MAX_DIRTY` changed rectangles; one more turns it into "whole screen".
#[derive(Debug, Clone)]
pub struct Dirty {
    rects: [Rectangle; MAX_DIRTY],
    len: usize,
    full: bool,
}

impl Dirty {
    pub fn new() -> Dirty {
        Dirty { rects: [Rectangle::zero(); MAX_DIRTY], len: 0, full: false }
    }
    pub fn add(&mut self, r: Rectangle) {
        if self.full {
            return;
        }
        if self.len == MAX_DIRTY {
            self.full = true;
            self.len = 0;
            return;
        }
        self.rects[self.len] = r;
        self.len += 1;
    }
    /// Marks the whole screen as changed.
    pub fn set_full(&mut self) {
        self.full = true;
        self.len = 0;
    }
    pub fn is_full(&self) -> bool {
        self.full
    }
    /// The listed rectangles; empty when `is_full()` (the whole screen is dirty).
    pub fn rects(&self) -> &[Rectangle] {
        &self.rects[..self.len]
    }
    pub fn is_empty(&self) -> bool {
        !self.full && self.len == 0
    }
    pub fn clear(&mut self) {
        self.full = false;
        self.len = 0;
    }
}

impl Default for Dirty {
    fn default() -> Dirty {
        Dirty::new()
    }
}

/// Audio half of an applet: owned by the dispatcher and run on the audio
/// thread (`Send`). Talks to its `AppletUi` half only through `Params` atomics.
pub trait Applet {
    fn info(&self) -> AppletInfo;
    /// May allocate. Called off the audio thread by `Dispatcher::service`.
    fn start(&mut self);
    /// Realtime: overwrites `out` with stereo frames. No allocation, no locks.
    fn audio(&mut self, out: &mut [[f32; 2]]);
    /// May free memory. Called off the audio thread by `Dispatcher::service`.
    fn stop(&mut self);
}

/// UI half of an applet: lives on the UI thread, need not be `Send`.
pub trait AppletUi {
    fn info(&self) -> AppletInfo;
    fn input(&mut self, ev: InputEvent);
    /// `Canvas` is an `embedded-graphics` `DrawTarget<Color = Rgb565, Error = Infallible>`
    /// (the trait itself is not dyn compatible, so `dyn DrawTarget` is impossible).
    fn draw(&mut self, d: &mut Canvas<'_>, dirty: &mut Dirty);
}
