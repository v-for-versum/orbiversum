//! Two-level applet menu. Hold (`InputEvent::Press`) opens it; the root lists
//! groups, a group lists its applets. The knob moves the cursor, a tap on the
//! right part of the screen (or another hold) enters a group or selects an
//! applet, a tap on the left part goes back one level; zone glyphs show both. The menu paints
//! straight into the shared framebuffer, so the caller must `invalidate()`
//! the applet UI that becomes visible when the menu closes.
use alloc::vec::Vec;
use embedded_graphics::mono_font::{ascii::FONT_10X20, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use orbiversum_os::{Canvas, Dirty, Edge, Glyph, InputEvent, TapZone};

/// Rows shown at once (the list scrolls beyond that).
pub const VISIBLE: usize = 6;
const ROW_H: u32 = 36;
const LIST_TOP: i32 = 70;
const ROW_X: i32 = 70;
const ROW_W: u32 = 220;
const TITLE: Rectangle = Rectangle::new(Point::new(ROW_X, 30), Size::new(ROW_W, 30));
/// Left part of the screen: one level up.
const BACK: TapZone = TapZone::new(Rectangle::new(Point::new(0, 0), Size::new(120, 360)), Edge::Left, Glyph::Back);
/// Right part of the screen: enter / select.
const FORWARD: TapZone =
    TapZone::new(Rectangle::new(Point::new(120, 0), Size::new(240, 360)), Edge::Right, Glyph::Forward);

/// A named set of applets shown as one root row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: &'static str,
    /// Applet indices (into the `names` given to [`Menu::new`]), in display order.
    pub items: Vec<usize>,
}

/// What the menu did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The menu is closed and ignored the event: give it to the applet.
    Pass(InputEvent),
    /// Handled by the menu (opened, moved the cursor, changed level).
    Consumed,
    /// Confirmed: select this applet (global index); the menu is closed.
    Select(usize),
}

#[derive(Clone, Copy)]
enum Row {
    Group(usize),
    Item(usize),
}

pub struct Menu {
    names: Vec<&'static str>,
    groups: Vec<Group>,
    open: bool,
    /// `None` = root list of groups, `Some(g)` = inside group `g`.
    level: Option<usize>,
    cursor: usize,
    /// What is on screen: `None` = nothing painted yet.
    painted: Option<(Option<usize>, usize, usize)>, // (level, cursor, offset)
}

impl Menu {
    pub fn new(names: Vec<&'static str>, groups: Vec<Group>) -> Menu {
        Menu { names, groups, open: false, level: None, cursor: 0, painted: None }
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    /// Cursor within the current level.
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    /// `None` at the root, `Some(group)` inside a group.
    pub fn level(&self) -> Option<usize> {
        self.level
    }

    fn row_count(&self) -> usize {
        match self.level {
            None => self.groups.len(),
            Some(g) => self.groups.get(g).map_or(0, |g| g.items.len()),
        }
    }

    fn row(&self, i: usize) -> Option<Row> {
        match self.level {
            None => (i < self.groups.len()).then_some(Row::Group(i)),
            Some(g) => self.groups.get(g)?.items.get(i).map(|&a| Row::Item(a)),
        }
    }

    fn label(&self, i: usize) -> &'static str {
        match self.row(i) {
            Some(Row::Group(g)) => self.groups[g].name,
            Some(Row::Item(a)) => self.names.get(a).copied().unwrap_or(""),
            None => "",
        }
    }

    fn title(&self) -> &'static str {
        self.level.and_then(|g| self.groups.get(g)).map_or("Applets", |g| g.name)
    }

    fn open_at(&mut self, current: usize) {
        self.open = true;
        self.painted = None;
        let found = self
            .groups
            .iter()
            .enumerate()
            .find_map(|(g, grp)| grp.items.iter().position(|&a| a == current).map(|p| (g, p)));
        (self.level, self.cursor) = match found {
            Some((g, p)) => (Some(g), p),
            None => (None, 0),
        };
    }

    fn confirm(&mut self) -> Outcome {
        match self.row(self.cursor) {
            Some(Row::Group(g)) => {
                self.level = Some(g);
                self.cursor = 0;
                self.painted = None;
                Outcome::Consumed
            }
            Some(Row::Item(a)) => {
                self.open = false;
                Outcome::Select(a)
            }
            None => Outcome::Consumed,
        }
    }

    /// `current` is the applet selected now (the cursor starts there).
    pub fn handle(&mut self, ev: InputEvent, current: usize) -> Outcome {
        if !self.open {
            return match ev {
                InputEvent::Press => {
                    self.open_at(current);
                    Outcome::Consumed
                }
                other => Outcome::Pass(other),
            };
        }
        match ev {
            InputEvent::Knob(n) => {
                let len = self.row_count() as i32;
                self.cursor = (self.cursor as i32 + n).rem_euclid(len.max(1)) as usize;
                Outcome::Consumed
            }
            InputEvent::Tap { x, y } if BACK.hit(x, y) => {
                if let Some(g) = self.level.take() {
                    self.cursor = g;
                    self.painted = None;
                }
                Outcome::Consumed
            }
            InputEvent::Tap { .. } | InputEvent::Press => self.confirm(),
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
        if idx < self.row_count() {
            let st = MonoTextStyle::new(&FONT_10X20, fg);
            let _ = Text::with_baseline(self.label(idx), r.top_left + Point::new(10, 6), st, Baseline::Top).draw(d);
        }
    }

    /// Paints what changed since the last call. No-op while closed.
    pub fn draw(&mut self, d: &mut Canvas<'_>, dirty: &mut Dirty) {
        if !self.open {
            return;
        }
        let (cur, off) = (self.cursor, self.offset());
        let shown = self.row_count().saturating_sub(off).min(VISIBLE);
        match self.painted {
            None => {
                let _ = d.clear(Rgb565::BLACK);
                let st = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
                let _ = Text::with_baseline(self.title(), TITLE.top_left, st, Baseline::Top).draw(d);
                for slot in 0..shown {
                    self.paint_row(d, slot, off + slot);
                }
                if self.level.is_some() {
                    BACK.paint(d);
                }
                FORWARD.paint(d);
                dirty.set_full();
            }
            Some((_, _, po)) if po != off => {
                for slot in 0..VISIBLE {
                    self.paint_row(d, slot, off + slot);
                }
                dirty.add(Rectangle::new(
                    Point::new(ROW_X, LIST_TOP),
                    Size::new(ROW_W, VISIBLE as u32 * ROW_H),
                ));
            }
            Some((_, pc, _)) if pc != cur => {
                for idx in [pc, cur] {
                    let slot = idx - off;
                    self.paint_row(d, slot, idx);
                    dirty.add(Self::row_rect(slot));
                }
            }
            Some(_) => {}
        }
        self.painted = Some((self.level, cur, off));
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

    /// Collects the points of every drawn pixel (text goes through `draw_pixels`).
    struct Px(Vec<Point>);
    impl Surface for Px {
        fn size(&self) -> Size {
            Size::new(360, 360)
        }
        fn draw_pixels(&mut self, p: &mut dyn Iterator<Item = Pixel<Rgb565>>) {
            self.0.extend(p.map(|Pixel(pt, _)| pt));
        }
        fn fill_contiguous(&mut self, _: &Rectangle, c: &mut dyn Iterator<Item = Rgb565>) {
            c.for_each(drop);
        }
        fn fill_solid(&mut self, _: &Rectangle, _: Rgb565) {}
    }

    fn painted_in(m: &mut Menu, area: &Rectangle) -> bool {
        let mut s = Px(Vec::new());
        let mut d = Dirty::new();
        m.draw(&mut Canvas(&mut s), &mut d);
        s.0.iter().any(|p| area.contains(*p))
    }

    #[test]
    fn edge_glyphs_show_what_the_taps_do() {
        let mut m = menu();
        m.handle(InputEvent::Press, usize::MAX);
        assert!(painted_in(&mut m, &FORWARD.glyph_rect()), "root shows the forward glyph");
        m.handle(RIGHT, usize::MAX);
        assert!(painted_in(&mut m, &FORWARD.glyph_rect()), "and inside a group");
        m.handle(LEFT, usize::MAX);
        let mut root = Px(Vec::new());
        m.draw(&mut Canvas(&mut root), &mut Dirty::new());
        assert!(!root.0.iter().any(|p| BACK.glyph_rect().contains(*p)), "no back glyph at the root");
        m.handle(RIGHT, usize::MAX);
        assert!(painted_in(&mut m, &BACK.glyph_rect()), "inside a group the back glyph is shown");
    }

    #[test]
    fn left_tap_at_the_root_changes_nothing() {
        let mut m = menu();
        m.handle(InputEvent::Press, usize::MAX);
        m.handle(InputEvent::Knob(1), usize::MAX);
        assert_eq!(m.handle(LEFT, usize::MAX), Outcome::Consumed);
        assert_eq!(m.level(), None);
        assert_eq!(m.cursor(), 1);
    }

    const LEFT: InputEvent = InputEvent::Tap { x: 119, y: 0 };
    const RIGHT: InputEvent = InputEvent::Tap { x: 120, y: 0 };

    /// Applets a, b, c; group g0 = [b, a], group g1 = [c].
    fn menu() -> Menu {
        Menu::new(
            vec!["a", "b", "c"],
            vec![Group { name: "g0", items: vec![1, 0] }, Group { name: "g1", items: vec![2] }],
        )
    }

    fn long_menu(n: usize) -> Menu {
        let names = vec!["a", "b", "c", "d", "e", "f", "g", "h"];
        Menu::new(names[..n].to_vec(), vec![Group { name: "all", items: (0..n).collect() }])
    }

    #[test]
    fn closed_menu_passes_everything_but_press() {
        let mut m = menu();
        assert_eq!(m.handle(RIGHT, 0), Outcome::Pass(RIGHT));
        assert_eq!(m.handle(InputEvent::Knob(3), 0), Outcome::Pass(InputEvent::Knob(3)));
        assert_eq!(m.handle(InputEvent::Press, 0), Outcome::Consumed);
        assert!(m.is_open());
    }

    #[test]
    fn root_lists_groups_and_enter_goes_down() {
        let mut m = menu();
        m.handle(InputEvent::Press, usize::MAX); // current applet in no group
        assert_eq!(m.level(), None);
        assert_eq!(m.cursor(), 0);
        assert_eq!(m.row_count(), 2, "root shows the two groups");
        assert_eq!(m.handle(RIGHT, usize::MAX), Outcome::Consumed);
        assert_eq!(m.level(), Some(0));
        assert_eq!(m.cursor(), 0);
        assert_eq!(m.row_count(), 2, "g0 has two items");
    }

    #[test]
    fn tap_on_item_selects_global_index() {
        let mut m = menu();
        m.handle(InputEvent::Press, usize::MAX);
        m.handle(RIGHT, usize::MAX);
        m.handle(InputEvent::Knob(1), usize::MAX);
        assert_eq!(m.handle(RIGHT, usize::MAX), Outcome::Select(0));
        assert!(!m.is_open());
    }

    #[test]
    fn left_tap_goes_back_and_root_back_is_noop() {
        let mut m = menu();
        m.handle(InputEvent::Press, usize::MAX);
        m.handle(InputEvent::Knob(1), usize::MAX); // g1
        m.handle(RIGHT, usize::MAX);
        assert_eq!(m.level(), Some(1));
        assert_eq!(m.handle(LEFT, usize::MAX), Outcome::Consumed);
        assert_eq!(m.level(), None);
        assert_eq!(m.cursor(), 1, "cursor returns to the group we left");
        assert_eq!(m.handle(LEFT, usize::MAX), Outcome::Consumed);
        assert!(m.is_open());
        assert_eq!(m.level(), None);
    }

    #[test]
    fn press_confirms_like_right_tap() {
        let mut m = menu();
        m.handle(InputEvent::Press, usize::MAX);
        assert_eq!(m.handle(InputEvent::Press, usize::MAX), Outcome::Consumed);
        assert_eq!(m.level(), Some(0));
        assert_eq!(m.handle(InputEvent::Press, usize::MAX), Outcome::Select(1));
    }

    #[test]
    fn knob_wraps_within_level() {
        let mut m = menu();
        m.handle(InputEvent::Press, usize::MAX);
        m.handle(InputEvent::Knob(1), usize::MAX);
        assert_eq!(m.cursor(), 1);
        m.handle(InputEvent::Knob(1), usize::MAX);
        assert_eq!(m.cursor(), 0);
        m.handle(InputEvent::Knob(-1), usize::MAX);
        assert_eq!(m.cursor(), 1);
    }

    #[test]
    fn reopen_starts_in_group_of_current_applet() {
        let mut m = menu();
        m.handle(InputEvent::Press, 2);
        assert_eq!(m.level(), Some(1));
        assert_eq!(m.cursor(), 0);
        m.handle(InputEvent::Press, 0);
        m.handle(InputEvent::Press, 0);
        let mut m = menu();
        m.handle(InputEvent::Press, 0);
        assert_eq!(m.level(), Some(0));
        assert_eq!(m.cursor(), 1, "applet 0 is the second item of g0");
    }

    #[test]
    fn empty_group_and_ungrouped_applet_do_not_panic() {
        let mut m = Menu::new(vec!["a", "b"], vec![Group { name: "empty", items: vec![] }]);
        m.handle(InputEvent::Press, 1); // ungrouped applet: root, cursor 0
        assert_eq!(m.level(), None);
        assert_eq!(m.handle(RIGHT, 1), Outcome::Consumed);
        assert_eq!(m.level(), Some(0));
        assert_eq!(m.row_count(), 0);
        assert_eq!(m.handle(InputEvent::Knob(2), 1), Outcome::Consumed);
        assert_eq!(m.handle(InputEvent::Press, 1), Outcome::Consumed);
        let mut s = Null;
        let mut c = Canvas(&mut s);
        let mut d = Dirty::new();
        m.draw(&mut c, &mut d);
        assert_eq!(m.handle(LEFT, 1), Outcome::Consumed);
        assert_eq!(m.level(), None);
        let mut m0 = Menu::new(vec![], vec![]);
        m0.handle(InputEvent::Press, 0);
        assert_eq!(m0.handle(InputEvent::Knob(1), 0), Outcome::Consumed);
        m0.draw(&mut c, &mut d);
    }

    #[test]
    fn first_paint_is_full_then_only_two_rows() {
        let mut m = long_menu(3);
        let mut s = Null;
        let mut c = Canvas(&mut s);
        m.handle(InputEvent::Press, usize::MAX);
        m.handle(RIGHT, usize::MAX);
        let mut d = Dirty::new();
        m.draw(&mut c, &mut d);
        assert!(d.is_full());
        d.clear();
        m.draw(&mut c, &mut d);
        assert!(d.is_empty(), "nothing changed");
        m.handle(InputEvent::Knob(1), usize::MAX);
        m.draw(&mut c, &mut d);
        assert!(!d.is_full());
        assert_eq!(d.rects().len(), 2);
    }

    #[test]
    fn entering_and_leaving_a_level_repaints_fully() {
        let mut m = menu();
        let mut s = Null;
        let mut c = Canvas(&mut s);
        m.handle(InputEvent::Press, usize::MAX);
        let mut d = Dirty::new();
        m.draw(&mut c, &mut d);
        d.clear();
        m.handle(RIGHT, usize::MAX);
        m.draw(&mut c, &mut d);
        assert!(d.is_full());
        d.clear();
        m.handle(LEFT, usize::MAX);
        m.draw(&mut c, &mut d);
        assert!(d.is_full());
        // Re-opening repaints everything.
        m.handle(RIGHT, usize::MAX);
        m.handle(RIGHT, usize::MAX); // select
        m.handle(InputEvent::Press, 0);
        let mut d2 = Dirty::new();
        m.draw(&mut c, &mut d2);
        assert!(d2.is_full());
    }

    #[test]
    fn long_group_scrolls_without_leaving_the_list_area() {
        let mut m = long_menu(8);
        let mut s = Null;
        let mut c = Canvas(&mut s);
        m.handle(InputEvent::Press, usize::MAX);
        m.handle(RIGHT, usize::MAX);
        let mut d = Dirty::new();
        m.draw(&mut c, &mut d);
        d.clear();
        m.handle(InputEvent::Knob(7), usize::MAX);
        m.draw(&mut c, &mut d);
        assert_eq!(d.rects().len(), 1);
        let r = d.rects()[0];
        assert!(r.top_left.y + r.size.height as i32 <= 360);
    }
}
