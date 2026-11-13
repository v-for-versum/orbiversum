//! Starlids and Eno applets: an audio half (`VoiceApplet`) and a UI half
//! (`SceneUi`) that share only `orbiversum_os::Params`.
#![cfg_attr(not(test), no_std)]
extern crate alloc;

mod scene;

use alloc::boxed::Box;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use dronecaster_eno::Eno;
use dronecaster_starlids::Starlids;
use embedded_graphics::mono_font::{ascii::FONT_10X20, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use orbiversum_os::{Applet, AppletInfo, AppletUi, Canvas, Dirty, Glyph, InputEvent, Params, ParamUi, VoiceApplet, GLYPH};
use scene::Scene;

/// Where the hz/amp line is drawn (10x20 font), right under the scene.
const TEXT_ROW: Rectangle = Rectangle::new(Point::new(60, 198), Size::new(240, 26));
/// Knob glyph, centred under the value line; drawn with the full repaint like
/// the tap-zone glyphs of [`ParamUi::ZONES`]. None of them overlaps the scene.
const KNOB_GLYPH: Rectangle = Rectangle::new(Point::new(172, 232), Size::new(GLYPH, GLYPH));

/// UI half of a dronecaster voice: the animated `classic` scene with an
/// hz/amp line under it. Input is handled like [`ParamUi`] (knob = semitones,
/// tap = amp). Frames advance by the injected clock, not by `draw` calls.
struct SceneUi {
    input: ParamUi,
    params: Arc<Params>,
    scene: Scene,
    now_us: fn() -> i64,
    /// Bits of the (hz, amp) last painted; `None` until the first draw.
    drawn: Option<(u32, u32)>,
}

impl SceneUi {
    fn new(name: &'static str, params: Arc<Params>, seed: u32, now_us: fn() -> i64) -> SceneUi {
        SceneUi { input: ParamUi::new(name, params.clone()), params, scene: Scene::new(seed), now_us, drawn: None }
    }

    fn paint_text(&self, d: &mut Canvas<'_>) {
        let _ = TEXT_ROW.into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK)).draw(d);
        let mut row = d.clipped(&TEXT_ROW);
        let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
        let ts = TextStyleBuilder::new().alignment(Alignment::Center).baseline(Baseline::Top).build();
        let s = format!("hz {:.1}   amp {:.2}", self.params.hz(), self.params.amp());
        let at = TEXT_ROW.top_left + Point::new(TEXT_ROW.size.width as i32 / 2, 3);
        let _ = Text::with_text_style(&s, at, style, ts).draw(&mut row);
    }

    fn paint_glyphs(d: &mut Canvas<'_>) {
        for z in ParamUi::ZONES {
            z.paint(d);
        }
        Glyph::Knob.paint(d, KNOB_GLYPH);
    }
}

impl AppletUi for SceneUi {
    fn info(&self) -> AppletInfo {
        self.input.info()
    }

    fn input(&mut self, ev: InputEvent) {
        self.input.input(ev);
    }

    fn invalidate(&mut self) {
        self.drawn = None;
    }

    fn draw(&mut self, d: &mut Canvas<'_>, dirty: &mut Dirty) {
        let now = (self.params.hz().to_bits(), self.params.amp().to_bits());
        match self.drawn {
            None => {
                let _ = d.clear(Rgb565::BLACK);
                self.scene.draw_all(d);
                self.paint_text(d);
                Self::paint_glyphs(d);
                dirty.set_full();
                self.scene.reset();
            }
            Some(prev) => {
                if self.scene.tick((self.now_us)()) {
                    for r in self.scene.dirty_rects() {
                        let mut c = d.clipped(&r);
                        let _ = c.clear(Rgb565::BLACK);
                        self.scene.draw_all(&mut c);
                        dirty.add(r);
                    }
                }
                if prev != now {
                    self.paint_text(d);
                    dirty.add(TEXT_ROW);
                }
            }
        }
        self.drawn = Some(now);
    }
}

/// Starlids and Eno as (audio half, UI half) pairs sharing `params`.
/// `now_us` is a monotonic microsecond clock that paces the scene.
pub fn all(
    seed: u64,
    sr: f32,
    params: Arc<Params>,
    now_us: fn() -> i64,
) -> Vec<(Box<dyn Applet + Send>, Box<dyn AppletUi>)> {
    let ui_seed = (seed ^ (seed >> 32)) as u32;
    let pair = |name, a: Box<dyn Applet + Send>| -> (Box<dyn Applet + Send>, Box<dyn AppletUi>) {
        (a, Box::new(SceneUi::new(name, params.clone(), ui_seed, now_us)))
    };
    alloc::vec![
        pair("Starlids", Box::new(VoiceApplet::new("Starlids", Starlids::new(seed, sr), params.clone()))),
        pair("Eno", Box::new(VoiceApplet::new("Eno", Eno::new(seed, sr), params.clone()))),
    ]
}

/// Menu groups over the indices of [`all`]: (group name, applet indices in display order).
pub fn groups() -> Vec<(&'static str, Vec<usize>)> {
    alloc::vec![("dronecaster", alloc::vec![1, 0])]
}

#[cfg(test)]
fn glyph_rects() -> Vec<Rectangle> {
    let mut v: Vec<Rectangle> = ParamUi::ZONES.iter().map(|z| z.glyph_rect()).collect();
    v.push(KNOB_GLYPH);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics::prelude::{Pixel, Point, Size};
    use embedded_graphics::primitives::Rectangle;
    use orbiversum_os::{Canvas, Dirty, InputEvent, Surface, MAX_HZ, MIN_HZ};

    thread_local! {
        static NOW: Cell<i64> = const { Cell::new(0) };
    }
    fn fake_clock() -> i64 {
        NOW.with(|n| n.get())
    }
    fn advance(us: i64) {
        NOW.with(|n| n.set(n.get() + us));
    }

    /// Records the bounding box of everything drawn.
    struct Rec {
        min: Point,
        max: Point,
        px: Vec<Point>,
    }
    impl Rec {
        fn new() -> Rec {
            Rec { min: Point::new(i32::MAX, i32::MAX), max: Point::new(i32::MIN, i32::MIN), px: Vec::new() }
        }
        fn grow(&mut self, r: &Rectangle) {
            if let Some(br) = r.bottom_right() {
                self.min = Point::new(self.min.x.min(r.top_left.x), self.min.y.min(r.top_left.y));
                self.max = Point::new(self.max.x.max(br.x), self.max.y.max(br.y));
            }
        }
    }
    impl Surface for Rec {
        fn size(&self) -> Size {
            Size::new(360, 360)
        }
        fn draw_pixels(&mut self, p: &mut dyn Iterator<Item = Pixel<Rgb565>>) {
            for Pixel(pt, _) in p {
                self.px.push(pt);
                self.grow(&Rectangle::new(pt, Size::new(1, 1)));
            }
        }
        fn fill_contiguous(&mut self, a: &Rectangle, c: &mut dyn Iterator<Item = Rgb565>) {
            c.for_each(drop);
            self.grow(a);
        }
        fn fill_solid(&mut self, a: &Rectangle, _: Rgb565) {
            self.grow(a);
        }
    }

    fn eno() -> (Arc<Params>, Box<dyn AppletUi>) {
        let params = Arc::new(Params::default());
        params.set_hz(110.0);
        params.set_amp(0.5);
        NOW.with(|n| n.set(0));
        let (_, ui) = all(1, 48_000.0, params.clone(), fake_clock).remove(1);
        (params, ui)
    }

    fn draw(ui: &mut Box<dyn AppletUi>, s: &mut Rec) -> Dirty {
        let mut d = Dirty::new();
        ui.draw(&mut Canvas(s), &mut d);
        d
    }

    #[test]
    fn all_returns_matching_pairs() {
        let v = all(1, 48_000.0, Arc::new(Params::default()), fake_clock);
        let names: Vec<_> = v.iter().map(|(a, u)| (a.info().name, u.info().name)).collect();
        assert_eq!(names, [("Starlids", "Starlids"), ("Eno", "Eno")]);
    }

    #[test]
    fn groups_point_at_real_applets() {
        let v = all(1, 48_000.0, Arc::new(Params::default()), fake_clock);
        let g = groups();
        assert_eq!(g[0].0, "dronecaster");
        assert!(g.iter().all(|(_, items)| items.iter().all(|&i| i < v.len())));
        let names: Vec<_> = g[0].1.iter().map(|&i| v[i].1.info().name).collect();
        assert_eq!(names, ["Eno", "Starlids"]);
    }

    #[test]
    fn full_paint_draws_glyphs_not_text_hints() {
        let (_, mut ui) = eno();
        let mut s = Rec::new();
        draw(&mut ui, &mut s);
        assert!(s.px.iter().any(|p| TEXT_ROW.contains(*p)), "values drawn in TEXT_ROW");
        assert!(TEXT_ROW.size.height >= 20, "values use the 10x20 font");
        let rects = glyph_rects();
        assert_eq!(rects.len(), 3, "plus, minus and knob");
        for r in rects {
            assert!(s.px.iter().any(|p| r.contains(*p)), "glyph at {r:?} drawn");
        }
    }

    #[test]
    fn knob_glyph_is_inside_the_circle_and_clear_of_text_and_other_glyphs() {
        let br = KNOB_GLYPH.bottom_right().unwrap();
        for c in [KNOB_GLYPH.top_left, br, Point::new(br.x, KNOB_GLYPH.top_left.y), Point::new(KNOB_GLYPH.top_left.x, br.y)] {
            assert!((c.x - 180).pow(2) + (c.y - 180).pow(2) <= 174 * 174, "corner {c:?} outside the circle");
        }
        assert!(KNOB_GLYPH.intersection(&TEXT_ROW).is_zero_sized(), "overlaps the value text");
        for z in ParamUi::ZONES {
            assert!(KNOB_GLYPH.intersection(&z.glyph_rect()).is_zero_sized(), "overlaps {:?}", z.edge);
        }
    }

    #[test]
    fn scene_never_redraws_over_a_glyph() {
        // Glyphs are painted once, with the full repaint; this holds only while
        // no animated region reaches them.
        let scene = Scene::new(1);
        for g in glyph_rects() {
            for r in scene.dirty_rects() {
                assert!(r.intersection(&g).is_zero_sized(), "{r:?} overlaps glyph {g:?}");
            }
        }
    }

    #[test]
    fn every_applet_is_in_a_group() {
        let n = all(1, 48_000.0, Arc::new(Params::default()), fake_clock).len();
        let mut listed: Vec<usize> = groups().into_iter().flat_map(|(_, items)| items).collect();
        listed.sort_unstable();
        assert_eq!(listed, (0..n).collect::<Vec<_>>(), "an applet in no group is unreachable from the menu");
    }

    #[test]
    fn first_draw_is_full_then_ticks_dirty_small_rects() {
        let (_, mut ui) = eno();
        let mut s = Rec::new();
        assert!(draw(&mut ui, &mut s).is_full());
        assert!(draw(&mut ui, &mut s).is_empty(), "second draw only records the clock baseline");
        assert!(draw(&mut ui, &mut s).is_empty(), "clock has not moved");
        advance(200_000);
        let d = draw(&mut ui, &mut s);
        assert!(!d.is_full() && !d.is_empty());
        assert!(d.rects().iter().all(|r| scene::SCENE_RECT.contains(r.top_left) && scene::SCENE_RECT.contains(r.bottom_right().unwrap())));
    }

    #[test]
    fn input_still_edits_params_and_text_row_repaints_alone() {
        let (params, mut ui) = eno();
        let mut s = Rec::new();
        draw(&mut ui, &mut s);
        draw(&mut ui, &mut s);
        ui.input(InputEvent::Knob(12));
        ui.input(InputEvent::Tap { x: 180, y: 0 });
        assert!((params.hz() - 220.0).abs() < 0.01, "an octave up, got {}", params.hz());
        assert!((params.amp() - 0.6).abs() < 1e-5);
        let d = draw(&mut ui, &mut s);
        assert_eq!(d.rects(), [TEXT_ROW]);
    }

    #[test]
    fn invalidate_repaints_everything_and_forgets_the_clock() {
        let (_, mut ui) = eno();
        let mut s = Rec::new();
        draw(&mut ui, &mut s);
        draw(&mut ui, &mut s);
        ui.invalidate();
        assert!(draw(&mut ui, &mut s).is_full());
        advance(600_000_000);
        assert!(draw(&mut ui, &mut s).is_empty(), "no catch-up after a long absence");
    }

    #[test]
    fn extreme_values_stay_inside_the_text_row() {
        for hz in [MIN_HZ, MAX_HZ, f32::NAN, f32::INFINITY, 1e30] {
            let (params, mut ui) = eno();
            let mut s = Rec::new();
            draw(&mut ui, &mut s);
            draw(&mut ui, &mut s);
            params.set_hz(hz);
            params.set_amp(f32::NAN);
            let mut s = Rec::new();
            let d = draw(&mut ui, &mut s);
            assert_eq!(d.rects(), [TEXT_ROW], "hz = {hz}");
            assert!(TEXT_ROW.contains(s.min) && TEXT_ROW.contains(s.max), "hz = {hz}: drew {:?}..{:?}", s.min, s.max);
            assert!(draw(&mut ui, &mut Rec::new()).is_empty(), "unchanged NaN must not repaint forever");
        }
    }
}
