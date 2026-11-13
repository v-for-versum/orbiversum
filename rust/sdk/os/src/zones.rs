//! Tap zones: one description of "a tap here does this" that serves both the
//! hit-test and the glyph that tells the user about it. Glyphs are placed from
//! the geometry of the round screen, not by hand.
use embedded_graphics::draw_target::DrawTargetExt;
use embedded_graphics::geometry::AngleUnit;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Arc, Line, PrimitiveStyle, Rectangle, Triangle};
use crate::Canvas;

/// Glyph side, px.
pub const GLYPH: u32 = 16;

/// Side of the screen a zone sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// Top of the screen.
    Top,
    /// Bottom of the screen.
    Bottom,
    /// Left of the screen.
    Left,
    /// Right of the screen.
    Right,
}

/// Centre of the round screen, px.
const CENTRE: i32 = 180;
/// Glyph corners must stay within this radius (screen radius 180 minus a 6 px margin).
const LIMIT: i32 = 174;
/// Glyph colour: dim, so it does not compete with the animation.
const COLOR: Rgb565 = Rgb565::new(16, 32, 16);

/// What a glyph depicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `<`
    Back,
    /// `>`
    Forward,
    /// Arc with an arrowhead: the knob.
    Knob,
}

impl Glyph {
    /// Draws the glyph inside `rect` (a `GLYPH`x`GLYPH` square); nothing leaves it.
    pub fn paint(self, d: &mut Canvas<'_>, rect: Rectangle) {
        let mut d = d.clipped(&rect);
        let o = rect.top_left;
        let p = |x: i32, y: i32| o + Point::new(x, y);
        let stroke = PrimitiveStyle::with_stroke(COLOR, 2);
        let line = |a: Point, b: Point| Line::new(a, b).into_styled(stroke);
        match self {
            Glyph::Plus => {
                let _ = line(p(2, 8), p(13, 8)).draw(&mut d);
                let _ = line(p(8, 2), p(8, 13)).draw(&mut d);
            }
            Glyph::Minus => {
                let _ = line(p(2, 8), p(13, 8)).draw(&mut d);
            }
            Glyph::Back => {
                let _ = line(p(10, 2), p(4, 8)).draw(&mut d);
                let _ = line(p(4, 8), p(10, 14)).draw(&mut d);
            }
            Glyph::Forward => {
                let _ = line(p(5, 2), p(11, 8)).draw(&mut d);
                let _ = line(p(11, 8), p(5, 14)).draw(&mut d);
            }
            Glyph::Knob => {
                let _ = Arc::new(p(2, 2), 12, 30.0.deg(), 270.0.deg()).into_styled(stroke).draw(&mut d);
                let _ = Triangle::new(p(14, 5), p(13, 1), p(9, 6))
                    .into_styled(PrimitiveStyle::with_fill(COLOR))
                    .draw(&mut d);
            }
        }
    }
}

/// An area of the screen where a tap means something, with its glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapZone {
    /// Where a tap counts as a hit.
    pub area: Rectangle,
    /// Which edge the glyph is drawn at.
    pub edge: Edge,
    /// The glyph.
    pub glyph: Glyph,
}

impl TapZone {
    /// A zone over `area` whose glyph sits at `edge`.
    pub const fn new(area: Rectangle, edge: Edge, glyph: Glyph) -> TapZone {
        TapZone { area, edge, glyph }
    }
    /// Whether a tap at `(x, y)` is in this zone.
    pub fn hit(&self, x: i16, y: i16) -> bool {
        self.area.contains(Point::new(x as i32, y as i32))
    }
    /// The square the glyph occupies.
    pub fn glyph_rect(&self) -> Rectangle {
        let g = GLYPH as i32;
        let half = g / 2;
        // Largest distance `d` from the centre to the glyph's outer side with
        // its outer corners (`d`, `half`) still inside the limit circle.
        let mut d = 0;
        while (d + 1) * (d + 1) + half * half <= LIMIT * LIMIT {
            d += 1;
        }
        let at = match self.edge {
            Edge::Top => Point::new(CENTRE - half, CENTRE - d),
            Edge::Bottom => Point::new(CENTRE - half, CENTRE + d - g),
            Edge::Left => Point::new(CENTRE - d, CENTRE - half),
            Edge::Right => Point::new(CENTRE + d - g, CENTRE - half),
        };
        Rectangle::new(at, Size::new(GLYPH, GLYPH))
    }
    /// Erases `glyph_rect` and draws the glyph.
    pub fn paint(&self, d: &mut Canvas<'_>) {
        let r = self.glyph_rect();
        let _ = r.into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK)).draw(d);
        self.glyph.paint(d, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use embedded_graphics::primitives::Rectangle;
    use crate::Surface;

    /// Records every pixel touched, however it is drawn.
    struct Px(Vec<Point>);
    impl Surface for Px {
        fn size(&self) -> Size {
            Size::new(360, 360)
        }
        fn draw_pixels(&mut self, p: &mut dyn Iterator<Item = Pixel<Rgb565>>) {
            self.0.extend(p.map(|Pixel(pt, _)| pt));
        }
        fn fill_contiguous(&mut self, a: &Rectangle, c: &mut dyn Iterator<Item = Rgb565>) {
            c.for_each(drop);
            self.0.extend(a.points());
        }
        fn fill_solid(&mut self, a: &Rectangle, _: Rgb565) {
            self.0.extend(a.points());
        }
    }

    const EDGES: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];
    const ANY: Rectangle = Rectangle::new(Point::new(0, 0), Size::new(360, 360));
    const LIMIT: i32 = 174;

    fn rect_of(e: Edge) -> Rectangle {
        TapZone::new(ANY, e, Glyph::Plus).glyph_rect()
    }

    fn corners(r: &Rectangle) -> [Point; 4] {
        let br = r.bottom_right().unwrap();
        [r.top_left, Point::new(br.x, r.top_left.y), Point::new(r.top_left.x, br.y), br]
    }

    fn dist2(p: Point) -> i32 {
        (p.x - 180).pow(2) + (p.y - 180).pow(2)
    }

    #[test]
    fn glyph_rect_corners_are_inside_the_circle() {
        for e in EDGES {
            let r = rect_of(e);
            assert_eq!(r.size, Size::new(GLYPH, GLYPH), "{e:?}");
            for c in corners(&r) {
                // Pixel centres: the far side of a pixel is 1 further out.
                assert!(dist2(c) <= LIMIT * LIMIT, "{e:?} corner {c:?}");
            }
        }
    }

    #[test]
    fn glyph_rect_hugs_its_edge() {
        let c = |r: Rectangle| r.center();
        // An even-sized square straddles the axis: centre pixel is 179 or 180.
        let on_axis = |v: i32| (179..=180).contains(&v);
        assert!(on_axis(c(rect_of(Edge::Top)).x));
        assert!(c(rect_of(Edge::Top)).y < 40);
        assert!(on_axis(c(rect_of(Edge::Bottom)).x));
        assert!(c(rect_of(Edge::Bottom)).y > 320);
        assert!(on_axis(c(rect_of(Edge::Left)).y));
        assert!(c(rect_of(Edge::Left)).x < 40);
        assert!(on_axis(c(rect_of(Edge::Right)).y));
        assert!(c(rect_of(Edge::Right)).x > 320);
        // Maximal: one pixel further out would leave the limit circle.
        let r = rect_of(Edge::Top);
        let out = Rectangle::new(r.top_left - Point::new(0, 1), r.size);
        assert!(corners(&out).iter().any(|p| dist2(*p) > LIMIT * LIMIT));
    }

    #[test]
    fn hit_is_half_open_at_the_boundary() {
        let top = TapZone::new(Rectangle::new(Point::new(0, 0), Size::new(360, 180)), Edge::Top, Glyph::Plus);
        assert!(top.hit(10, 179));
        assert!(!top.hit(10, 180));
        let right = TapZone::new(Rectangle::new(Point::new(120, 0), Size::new(240, 360)), Edge::Right, Glyph::Forward);
        assert!(right.hit(120, 5));
        assert!(!right.hit(119, 5));
        assert!(!top.hit(-1, 10) && !top.hit(10, -1));
    }

    #[test]
    fn every_glyph_paints_something_and_stays_in_its_rect() {
        let rect = Rectangle::new(Point::new(100, 100), Size::new(GLYPH, GLYPH));
        for g in [Glyph::Plus, Glyph::Minus, Glyph::Back, Glyph::Forward, Glyph::Knob] {
            let mut s = Px(Vec::new());
            g.paint(&mut Canvas(&mut s), rect);
            assert!(!s.0.is_empty(), "{g:?} draws nothing");
            assert!(s.0.iter().all(|p| rect.contains(*p)), "{g:?} leaves its rect");
        }
    }

    #[test]
    fn zone_paint_draws_in_its_glyph_rect_only() {
        for e in EDGES {
            let z = TapZone::new(ANY, e, Glyph::Plus);
            let mut s = Px(Vec::new());
            z.paint(&mut Canvas(&mut s));
            assert!(!s.0.is_empty());
            assert!(s.0.iter().all(|p| z.glyph_rect().contains(*p)), "{e:?}");
        }
    }
}
