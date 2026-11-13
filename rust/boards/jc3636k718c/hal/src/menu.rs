//! Applet list. Hold (`InputEvent::Press`) opens it, the knob moves the
//! cursor, a tap or another hold confirms. The menu paints straight into the
//! shared framebuffer, so the caller must `invalidate()` the applet UI that
//! becomes visible when the menu closes.
use alloc::vec::Vec;
use embedded_graphics::mono_font::{ascii::FONT_10X20, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use orbiversum_os::{Canvas, Dirty, InputEvent};

/// Rows shown at once (the list scrolls beyond that).
pub const VISIBLE: usize = 6;
const ROW_H: u32 = 36;
const LIST_TOP: i32 = 70;
const ROW_X: i32 = 70;
const ROW_W: u32 = 220;
const TITLE: Rectangle = Rectangle::new(Point::new(ROW_X, 30), Size::new(ROW_W, 30));

/// What the menu did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The menu is closed and ignored the event: give it to the applet.
    Pass(InputEvent),
    /// Handled by the menu (opened, moved the cursor).
    Consumed,
    /// Confirmed: select this applet; the menu is closed.
    Select(usize),
}

pub struct Menu {
    names: Vec<&'static str>,
    open: bool,
    cursor: usize,
    /// What is on screen: `None` = nothing painted yet.
    painted: Option<(usize, usize)>, // (cursor, offset)
}

impl Menu {
    pub fn new(names: Vec<&'static str>) -> Menu {
        Menu { names, open: false, cursor: 0, painted: None }
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// `current` is the applet selected now (the cursor starts there).
    pub fn handle(&mut self, ev: InputEvent, current: usize) -> Outcome {
        if !self.open {
            return match ev {
                InputEvent::Press => {
                    self.open = true;
                    self.cursor = current.min(self.names.len().saturating_sub(1));
                    self.painted = None;
                    Outcome::Consumed
                }
                other => Outcome::Pass(other),
            };
        }
        match ev {
            InputEvent::Knob(n) => {
                let len = self.names.len() as i32;
                self.cursor = (self.cursor as i32 + n).rem_euclid(len.max(1)) as usize;
                Outcome::Consumed
            }
            InputEvent::Tap { .. } | InputEvent::Press => {
                self.open = false;
                Outcome::Select(self.cursor)
            }
        }
    }

    fn offset(&self) -> usize {
        (self.cursor + 1).saturating_sub(VISIBLE)
    }

    fn row_rect(slot: usize) -> Rectangle {
        Rectangle::new(Point::new(ROW_X, LIST_TOP + (slot as u32 * ROW_H) as i32), Size::new(ROW_W, ROW_H - 4))
    }

    fn paint_row(&self, d: &mut Canvas<'_>, slot: usize, idx: usize) {
        let r = Self::row_rect(slot);
        let sel = idx == self.cursor;
        let (bg, fg) = if sel { (Rgb565::WHITE, Rgb565::BLACK) } else { (Rgb565::BLACK, Rgb565::WHITE) };
        let _ = r.into_styled(PrimitiveStyle::with_fill(bg)).draw(d);
        if let Some(name) = self.names.get(idx) {
            let st = MonoTextStyle::new(&FONT_10X20, fg);
            let _ = Text::with_baseline(name, r.top_left + Point::new(10, 6), st, Baseline::Top).draw(d);
        }
    }

    /// Paints what changed since the last call. No-op while closed.
    pub fn draw(&mut self, d: &mut Canvas<'_>, dirty: &mut Dirty) {
        if !self.open {
            return;
        }
        let (cur, off) = (self.cursor, self.offset());
        let shown = self.names.len().saturating_sub(off).min(VISIBLE);
        match self.painted {
            None => {
                let _ = d.clear(Rgb565::BLACK);
                let st = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
                let _ = Text::with_baseline("Applets", TITLE.top_left, st, Baseline::Top).draw(d);
                for slot in 0..shown {
                    self.paint_row(d, slot, off + slot);
                }
                dirty.set_full();
            }
            Some((pc, po)) if po != off => {
                let _ = pc;
                for slot in 0..VISIBLE {
                    self.paint_row(d, slot, off + slot);
                }
                dirty.add(Rectangle::new(
                    Point::new(ROW_X, LIST_TOP),
                    Size::new(ROW_W, VISIBLE as u32 * ROW_H),
                ));
            }
            Some((pc, _)) if pc != cur => {
                for idx in [pc, cur] {
                    let slot = idx - off;
                    self.paint_row(d, slot, idx);
                    dirty.add(Self::row_rect(slot));
                }
            }
            Some(_) => {}
        }
        self.painted = Some((cur, off));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use embedded_graphics::pixelcolor::Rgb565;
    use orbiversum_os::Surface;

    struct Null;
    impl Surface for Null {
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

    fn menu(n: usize) -> Menu {
        Menu::new(vec!["a", "b", "c", "d", "e", "f", "g", "h"][..n].to_vec())
    }

    #[test]
    fn closed_menu_passes_everything_but_press() {
        let mut m = menu(2);
        let tap = InputEvent::Tap { x: 1, y: 2 };
        assert_eq!(m.handle(tap, 0), Outcome::Pass(tap));
        assert_eq!(m.handle(InputEvent::Knob(3), 0), Outcome::Pass(InputEvent::Knob(3)));
        assert_eq!(m.handle(InputEvent::Press, 1), Outcome::Consumed);
        assert!(m.is_open());
        assert_eq!(m.cursor(), 1, "cursor starts at the current applet");
    }

    #[test]
    fn knob_wraps_and_tap_or_press_confirms() {
        let mut m = menu(2);
        m.handle(InputEvent::Press, 0);
        assert_eq!(m.handle(InputEvent::Knob(1), 0), Outcome::Consumed);
        assert_eq!(m.cursor(), 1);
        m.handle(InputEvent::Knob(1), 0);
        assert_eq!(m.cursor(), 0);
        m.handle(InputEvent::Knob(-1), 0);
        assert_eq!(m.cursor(), 1);
        assert_eq!(m.handle(InputEvent::Tap { x: 0, y: 0 }, 0), Outcome::Select(1));
        assert!(!m.is_open());
        m.handle(InputEvent::Press, 1);
        assert_eq!(m.handle(InputEvent::Press, 1), Outcome::Select(1));
    }

    #[test]
    fn first_paint_is_full_then_only_two_rows() {
        let mut m = menu(3);
        let mut s = Null;
        let mut c = Canvas(&mut s);
        m.handle(InputEvent::Press, 0);
        let mut d = Dirty::new();
        m.draw(&mut c, &mut d);
        assert!(d.is_full());
        d.clear();
        m.draw(&mut c, &mut d);
        assert!(d.is_empty(), "nothing changed");
        m.handle(InputEvent::Knob(1), 0);
        m.draw(&mut c, &mut d);
        assert!(!d.is_full());
        assert_eq!(d.rects().len(), 2);
        // Re-opening repaints everything.
        m.handle(InputEvent::Tap { x: 0, y: 0 }, 0);
        m.handle(InputEvent::Press, 1);
        let mut d2 = Dirty::new();
        m.draw(&mut c, &mut d2);
        assert!(d2.is_full());
    }

    #[test]
    fn long_list_scrolls_without_leaving_the_list_area() {
        let mut m = menu(8);
        let mut s = Null;
        let mut c = Canvas(&mut s);
        m.handle(InputEvent::Press, 0);
        let mut d = Dirty::new();
        m.draw(&mut c, &mut d);
        d.clear();
        m.handle(InputEvent::Knob(7), 0);
        m.draw(&mut c, &mut d);
        assert_eq!(d.rects().len(), 1);
        let r = d.rects()[0];
        assert!(r.top_left.y + r.size.height as i32 <= 360);
    }
}
