//! Board abstraction: input, audio output and display.
use crate::applet::{Dirty, InputEvent};
use alloc::boxed::Box;
use core::convert::Infallible;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::{DrawTarget, OriginDimensions, Pixel, Size};
use embedded_graphics::primitives::Rectangle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// A backend call failed (raw driver error code).
    Backend(i32),
    /// `AudioOut::start` called twice.
    AlreadyStarted,
    /// The requested sample rate is not supported by the hardware.
    UnsupportedSampleRate(u32),
    /// The audio task could not be spawned.
    Spawn,
}

impl core::fmt::Display for AudioError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for AudioError {}

pub type RenderFn = Box<dyn FnMut(&mut [[f32; 2]]) + Send>;

pub trait AudioOut {
    /// Starts a realtime task that repeatedly calls `render` with stereo frames.
    fn start(&mut self, sr: u32, render: RenderFn) -> Result<(), AudioError>;
}

/// Dyn-compatible drawing surface (`DrawTarget` has generic methods, so it is not).
pub trait Surface {
    fn size(&self) -> Size;
    fn draw_pixels(&mut self, pixels: &mut dyn Iterator<Item = Pixel<Rgb565>>);
    fn fill_contiguous(&mut self, area: &Rectangle, colors: &mut dyn Iterator<Item = Rgb565>);
    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565);
}

/// `DrawTarget<Color = Rgb565, Error = Infallible>` over a `&mut dyn Surface`.
pub struct Canvas<'a>(pub &'a mut dyn Surface);

impl OriginDimensions for Canvas<'_> {
    fn size(&self) -> Size {
        self.0.size()
    }
}

impl DrawTarget for Canvas<'_> {
    type Color = Rgb565;
    type Error = Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Infallible> {
        self.0.draw_pixels(&mut pixels.into_iter());
        Ok(())
    }
    fn fill_contiguous<I: IntoIterator<Item = Rgb565>>(
        &mut self,
        area: &Rectangle,
        colors: I,
    ) -> Result<(), Infallible> {
        self.0.fill_contiguous(area, &mut colors.into_iter());
        Ok(())
    }
    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Infallible> {
        self.0.fill_solid(area, color);
        Ok(())
    }
}

pub trait Display {
    fn size(&self) -> (u32, u32);
    fn surface(&mut self) -> &mut dyn Surface;
    /// Sends the changed regions to the panel.
    fn flush(&mut self, dirty: &Dirty);
}

pub trait Board {
    fn poll_input(&mut self) -> Option<InputEvent>;
    fn audio_out(&mut self) -> &mut dyn AudioOut;
    fn display(&mut self) -> &mut dyn Display;
}
