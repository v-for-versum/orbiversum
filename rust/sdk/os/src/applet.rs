//! The `Applet` trait, input events and dirty-rectangle tracking.
use crate::board::Canvas;
use embedded_graphics::primitives::Rectangle;

/// A user input, already decoded by the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// Encoder rotation: number of steps, signed.
    Knob(i32),
    /// A short touch at screen position (`x`, `y`).
    Tap {
        /// Horizontal position in pixels.
        x: i16,
        /// Vertical position in pixels.
        y: i16,
    },
    /// A long press (the board turns a long touch into this event).
    Press,
}

/// Static description of an applet, cached by the dispatcher for menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppletInfo {
    /// Display name.
    pub name: &'static str,
}

/// Capacity of a [`Dirty`] list.
pub const MAX_DIRTY: usize = 8;

/// Up to `MAX_DIRTY` changed rectangles; one more turns it into "whole screen".
#[derive(Debug, Clone)]
pub struct Dirty {
    rects: [Rectangle; MAX_DIRTY],
    len: usize,
    full: bool,
}

impl Dirty {
    /// An empty list: nothing changed.
    pub fn new() -> Dirty {
        Dirty { rects: [Rectangle::zero(); MAX_DIRTY], len: 0, full: false }
    }
    /// Records a changed rectangle; once the list is full, the whole screen is marked dirty instead.
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
    /// Whether the whole screen is marked as changed.
    pub fn is_full(&self) -> bool {
        self.full
    }
    /// The listed rectangles; empty when `is_full()` (the whole screen is dirty).
    pub fn rects(&self) -> &[Rectangle] {
        &self.rects[..self.len]
    }
    /// Whether nothing is marked as changed.
    pub fn is_empty(&self) -> bool {
        !self.full && self.len == 0
    }
    /// Forgets all marked changes (call after the display flush).
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
///
/// Lifecycle: `start` and `stop` run on the UI thread (inside
/// `Dispatcher::service`) while the audio thread is parked; `audio` runs on the
/// audio thread, between a `start` and the matching `stop`.
pub trait Applet {
    /// Static description (name) of this applet.
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
    /// Static description (name) of this applet.
    fn info(&self) -> AppletInfo;
    /// Handles one input event routed to this applet.
    fn input(&mut self, ev: InputEvent);
    /// The framebuffer no longer holds this applet's picture (it was switched
    /// to, or an overlay such as the menu painted over it): the next `draw`
    /// must repaint everything and mark the whole screen dirty.
    fn invalidate(&mut self);
    /// `Canvas` is an `embedded-graphics` `DrawTarget<Color = Rgb565, Error = Infallible>`
    /// (the trait itself is not dyn compatible, so `dyn DrawTarget` is impossible).
    /// Paints what changed into `d` and records the changed rectangles in `dirty`.
    fn draw(&mut self, d: &mut Canvas<'_>, dirty: &mut Dirty);
}
