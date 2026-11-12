//! UI half of an applet: encoder -> hz (semitones), tap -> amp, partial redraw.
use alloc::format;
use alloc::sync::Arc;
use embedded_graphics::mono_font::{ascii::FONT_10X20, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use os::{AppletInfo, AppletUi, Canvas, Dirty, InputEvent, Params};

pub const MIN_HZ: f32 = 20.0;
pub const MAX_HZ: f32 = 2000.0;
/// Amplitude change per tap.
pub const AMP_STEP: f32 = 0.1;

pub(crate) const NAME_ROW: Rectangle = Rectangle::new(Point::new(60, 60), Size::new(240, 30));
pub(crate) const HZ_ROW: Rectangle = Rectangle::new(Point::new(60, 150), Size::new(240, 30));
pub(crate) const AMP_ROW: Rectangle = Rectangle::new(Point::new(60, 190), Size::new(240, 30));

pub struct ParamUi {
    name: &'static str,
    params: Arc<Params>,
    /// Values last painted; `None` until the first draw.
    drawn: Option<(f32, f32)>,
}

impl ParamUi {
    pub fn new(name: &'static str, params: Arc<Params>) -> ParamUi {
        ParamUi { name, params, drawn: None }
    }
}

fn text(d: &mut Canvas<'_>, row: Rectangle, s: &str) {
    let _ = row.into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK)).draw(d);
    let st = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let _ = Text::with_baseline(s, row.top_left + Point::new(0, 5), st, Baseline::Top).draw(d);
}

impl AppletUi for ParamUi {
    fn info(&self) -> AppletInfo {
        AppletInfo { name: self.name }
    }

    fn input(&mut self, ev: InputEvent) {
        match ev {
            InputEvent::Knob(n) => {
                let hz = self.params.hz() * libm::powf(2.0, n as f32 / 12.0);
                self.params.set_hz(hz.clamp(MIN_HZ, MAX_HZ));
            }
            InputEvent::Tap { y, .. } => {
                // Top half of the round screen raises the level, bottom half lowers it.
                let step = if y < 180 { AMP_STEP } else { -AMP_STEP };
                self.params.set_amp((self.params.amp() + step).clamp(0.0, 1.0));
            }
            InputEvent::Press => {}
        }
    }

    fn draw(&mut self, d: &mut Canvas<'_>, dirty: &mut Dirty) {
        let (hz, amp) = (self.params.hz(), self.params.amp());
        match self.drawn {
            None => {
                let _ = d.clear(Rgb565::BLACK);
                text(d, NAME_ROW, self.name);
                text(d, HZ_ROW, &format!("hz  {hz:.1}"));
                text(d, AMP_ROW, &format!("amp {amp:.2}"));
                dirty.set_full();
            }
            Some((h, a)) => {
                if h != hz {
                    text(d, HZ_ROW, &format!("hz  {hz:.1}"));
                    dirty.add(HZ_ROW);
                }
                if a != amp {
                    text(d, AMP_ROW, &format!("amp {amp:.2}"));
                    dirty.add(AMP_ROW);
                }
            }
        }
        self.drawn = Some((hz, amp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics::prelude::{Pixel, Size};
    use embedded_graphics::primitives::Rectangle;
    use os::{AppletUi, Canvas, Dirty, InputEvent, Params, Surface};

    struct Mock;
    impl Surface for Mock {
        fn size(&self) -> Size {
            Size::new(360, 360)
        }
        fn draw_pixels(&mut self, p: &mut dyn Iterator<Item = Pixel<Rgb565>>) {
            p.for_each(drop);
        }
        fn fill_contiguous(&mut self, _: &Rectangle, c: &mut dyn Iterator<Item = Rgb565>) {
            c.for_each(drop);
        }
        fn fill_solid(&mut self, _: &Rectangle, _: Rgb565) {}
    }

    fn ui() -> (ParamUi, Arc<Params>) {
        let p = Arc::new(Params::new(110.0, 0.5));
        (ParamUi::new("Sine", p.clone()), p)
    }

    #[test]
    fn knob_moves_hz_by_semitone() {
        let (mut u, p) = ui();
        u.input(InputEvent::Knob(12));
        assert!((p.hz() - 220.0).abs() < 0.01, "{}", p.hz());
        u.input(InputEvent::Knob(-1));
        assert!((p.hz() - 220.0 / 2f32.powf(1.0 / 12.0)).abs() < 0.01);
        u.input(InputEvent::Knob(-100));
        assert!(p.hz() >= MIN_HZ);
        u.input(InputEvent::Knob(1000));
        assert!(p.hz() <= MAX_HZ);
    }

    #[test]
    fn amp_is_clamped_0_to_1() {
        let (mut u, p) = ui();
        for _ in 0..100 {
            u.input(InputEvent::Tap { x: 180, y: 100 });
        }
        assert_eq!(p.amp(), 1.0);
        for _ in 0..100 {
            u.input(InputEvent::Tap { x: 180, y: 300 });
        }
        assert_eq!(p.amp(), 0.0);
        u.input(InputEvent::Tap { x: 180, y: 100 });
        assert!(p.amp() > 0.0 && p.amp() < 1.0);
    }

    #[test]
    fn draw_marks_only_changed_regions() {
        let (mut u, _p) = ui();
        let mut s = Mock;
        let mut d = Dirty::new();
        u.draw(&mut Canvas(&mut s), &mut d);
        assert!(d.is_full(), "first draw paints everything");
        d.clear();
        u.draw(&mut Canvas(&mut s), &mut d);
        assert!(d.is_empty(), "nothing changed");
        u.input(InputEvent::Knob(1));
        u.draw(&mut Canvas(&mut s), &mut d);
        assert!(!d.is_full());
        assert_eq!(d.rects(), &[HZ_ROW]);
        d.clear();
        u.input(InputEvent::Tap { x: 180, y: 100 });
        u.draw(&mut Canvas(&mut s), &mut d);
        assert_eq!(d.rects(), &[AMP_ROW]);
    }
}
