//! Port of the original dronecaster `classic` scene (`lib/scenes/classic.lua`):
//! antenna, horizons, blinking lights, three birds, wind and a rare UAP, drawn
//! as line art at 2x on a 256x128 patch of the round screen.
//!
//! `Scene` holds the animation state (stepped at 5 Hz by [`Scene::tick`]) and
//! draws itself into any `DrawTarget`; the caller clips the target to the
//! rectangles from [`Scene::dirty_rects`] to repaint only what moves.
#![allow(dead_code)] // used by SceneUi in the next task
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle, Rectangle};

/// Screen position of the scene's top-left corner and the pixel scale.
const ORIGIN: Point = Point::new(52, 60);
const SCALE: i32 = 2;
/// Frame period: the original redraws at 5 Hz.
const FRAME_US: i64 = 200_000;
/// Lua `screen_levels["l"]` (0..=15) as a gray.
const LEVEL: u8 = 5;

/// Where the scene lives on the screen (128x64 original pixels at 2x).
pub(crate) const SCENE_RECT: Rectangle = Rectangle::new(ORIGIN, Size::new(128 * SCALE as u32, 64 * SCALE as u32));

const HOME: (i32, i32) = (25, 25);
const WIND_Y: [i32; 9] = [49, 49, 54, 54, 54, 61, 61, 61, 61];

/// Which of the three tower lights are on and the flare circle radius around each (0 = none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lights {
    pub on: [bool; 3],
    pub flares: [u8; 3],
}

/// The 9-frame light table of `draw_lights`.
pub(crate) fn lights(frame: u32) -> Lights {
    let (on, flares) = match frame % 9 {
        2 => ([false, true, true], [0, 2, 0]),
        3 => ([true, true, true], [0, 3, 0]),
        4 => ([false, false, true], [2, 4, 0]),
        6 => ([false, true, false], [0, 0, 0]),
        7 => ([true, false, true], [0, 0, 5]),
        8 => ([true, true, true], [0, 0, 3]),
        _ => ([true, true, true], [0, 0, 0]), // 0, 1, 5
    };
    Lights { on, flares }
}

pub(crate) struct Scene {
    rng: u32,
    frame: u32,
    last_us: Option<i64>,
    drift: (i32, i32),
    /// Wind dashes of the current frame: x in original pixels, `None` = absent.
    wind: [Option<i32>; 9],
    /// A UAP is in flight.
    uap: bool,
    /// Segment (`frame % 5`) of the UAP drawn this frame.
    uap_seg: Option<u32>,
}

impl Scene {
    pub(crate) fn new(seed: u32) -> Scene {
        Scene { rng: seed | 1, frame: 0, last_us: None, drift: HOME, wind: [None; 9], uap: false, uap_seg: None }
    }

    /// Forgets the clock baseline (call when the screen is repainted from scratch).
    pub(crate) fn reset(&mut self) {
        self.last_us = None;
    }

    fn rand(&mut self, n: u32) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x % n
    }

    /// Advances to the next frame when 200 ms have passed since the last one.
    /// The first call after `new`/`reset` only records the baseline; the clock
    /// going backwards re-baselines; a long gap yields a single frame.
    pub(crate) fn tick(&mut self, now_us: i64) -> bool {
        let Some(last) = self.last_us else {
            self.last_us = Some(now_us);
            return false;
        };
        if now_us < last {
            self.last_us = Some(now_us);
            return false;
        }
        let gap = now_us - last;
        if gap < FRAME_US {
            return false;
        }
        self.last_us = Some(if gap < 2 * FRAME_US { last + FRAME_US } else { now_us });
        self.step();
        true
    }

    fn step(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        let f = self.frame;
        // drift
        let (xc, yc) = (self.rand(2) as i32, self.rand(2) as i32);
        let (tx, ty) = (self.rand(2), self.rand(2));
        let cx = if tx == 0 { self.drift.0 - xc } else { self.drift.0 + xc };
        let cy = if ty == 0 { self.drift.1 - yc } else { self.drift.1 + yc };
        self.drift = (cx.clamp(HOME.0 - 5, HOME.0 + 5), cy.clamp(HOME.1 - 3, HOME.1 + 3));
        // wind (dashes past the right edge are dropped, as the norns screen clips them)
        let (f1, f2) = ((f % 20) as i32, (f % 13) as i32);
        let xs = [
            (self.rand(2) == 1).then_some(f1 + 80),
            (self.rand(3) != 0).then_some(f2 + 10),
            (self.rand(4) != 0).then_some(f1 * 2),
            (self.rand(3) != 1).then_some((f1 + 4) * 3),
            (self.rand(5) != 0).then_some((f1 + 2) * 5 + 28),
            (self.rand(2) == 1).then_some(f2 * 2 + 48),
            (self.rand(2) == 1).then_some((f1 + 6) * 4 + 57),
            (self.rand(2) == 1).then_some((f2 + 2) * 10 + 57),
            (self.rand(2) == 1).then_some((f2 + 3) * 8 + 57),
        ];
        for (w, x) in self.wind.iter_mut().zip(xs) {
            *w = x.filter(|&x| x <= 126);
        }
        // UAP
        if self.rand(8) == 3 {
            self.uap = true;
        }
        let seg = f % 5;
        self.uap_seg = self.uap.then_some(seg);
        if seg == 0 {
            self.uap = false;
        }
    }

    /// The lead bird's position (original pixels).
    pub(crate) fn bird_pos(&self) -> (i32, i32) {
        self.drift
    }

    pub(crate) fn uap_active(&self) -> bool {
        self.uap_seg.is_some()
    }

    /// Regions that change between frames, in screen coordinates: lights,
    /// birds, wind rows, UAP path.
    pub(crate) fn dirty_rects(&self) -> [Rectangle; 4] {
        [orect(58, 11, 76, 30), orect(12, 19, 40, 36), orect(0, 48, 128, 63), orect(84, 17, 102, 31)]
    }

    /// Draws the whole scene for the current frame (no background fill).
    pub(crate) fn draw_all<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D) {
        self.landscape(d);
        self.lights(d);
        self.birds(d);
        for (x, y) in self.wind.iter().zip(WIND_Y) {
            if let Some(x) = *x {
                mlrs(d, x, y, 1, 0);
            }
        }
        if let Some(seg) = self.uap_seg {
            match seg {
                1 => mls(d, 100, 18, 98, 20),
                2 => mls(d, 100, 18, 90, 25),
                3 => mls(d, 94, 22, 89, 26),
                4 => mls(d, 88, 26, 86, 28),
                _ => mlrs(d, 85, 30, 1, 0),
            }
        }
    }

    fn lights<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D) {
        const POS: [(i32, i32); 3] = [(62, 25), (65, 17), (69, 23)];
        const DY: [i32; 3] = [1, 0, 1];
        let l = lights(self.frame);
        for i in 0..3 {
            if l.on[i] {
                mlrs(d, POS[i].0, POS[i].1, 1, DY[i]);
            }
            if l.flares[i] > 0 {
                let r = l.flares[i] as i32;
                let top_left = px(POS[i].0, POS[i].1) - Point::new(r * SCALE, r * SCALE);
                let _ = Circle::new(top_left, (2 * r * SCALE + 1) as u32).into_styled(stroke()).draw(d);
            }
        }
    }

    fn birds<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D) {
        // wing dy per bird for frame % 3 = 0, 1, 2
        const JOE: [i32; 3] = [2, 1, -2];
        const BETH: [i32; 3] = [-2, 2, 1];
        const ALEX: [i32; 3] = [1, -2, 2];
        let k = (self.frame % 3) as usize;
        let (x, y) = self.drift;
        for ((bx, by), dy) in [((x, y), JOE[k]), ((x - 5, y + 5), BETH[k]), ((x + 7, y + 4), ALEX[k])] {
            mlrs(d, bx, by, 2, dy);
            mlrs(d, bx, by, -2, dy);
        }
    }

    fn landscape<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D) {
        // antenna sides, supports
        for (a, b, c, e) in [(62, 52, 66, 20), (70, 53, 66, 20), (62, 52, 70, 44), (70, 52, 62, 44), (70, 44, 63, 37)] {
            mls(d, a, b, c, e);
        }
        // relative lines: antenna horizontals and details, then the four horizons
        for (a, b, c, e) in [
            (64, 34, 3, 0),
            (64, 39, 3, 0),
            (64, 45, 3, 0),
            (65, 19, 2, 0),
            (62, 30, 2, 0),
            (67, 28, 2, 0),
            (62, 27, 1, 2),
            (69, 25, 1, 2),
            (0, 48, 60, 0),
            (72, 48, 50, 0),
            (1, 50, 1, 0),
            (4, 50, 40, 0),
            (46, 50, 9, 0),
            (57, 50, 1, 0),
            (74, 50, 40, 0),
            (116, 50, 2, 0),
            (5, 55, 3, 0),
            (10, 55, 40, 0),
            (55, 55, 20, 0),
            (80, 55, 41, 0),
            (33, 62, 62, 0),
            (100, 62, 5, 0),
            (108, 62, 2, 0),
        ] {
            mlrs(d, a, b, c, e);
        }
    }
}

/// Original-pixel point to screen point.
fn px(x: i32, y: i32) -> Point {
    ORIGIN + Point::new(x * SCALE, y * SCALE)
}

fn orect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rectangle {
    Rectangle::new(px(x0, y0), Size::new(((x1 - x0) * SCALE) as u32, ((y1 - y0) * SCALE) as u32))
}

fn stroke() -> PrimitiveStyle<Rgb565> {
    let g = LEVEL as u32;
    PrimitiveStyle::with_stroke(Rgb565::new((g * 2) as u8, (g * 4) as u8, (g * 2) as u8), SCALE as u32)
}

/// `screen.move(a, b); screen.line_rel(c, e)`.
fn mlrs<D: DrawTarget<Color = Rgb565>>(d: &mut D, a: i32, b: i32, c: i32, e: i32) {
    mls(d, a, b, a + c, b + e);
}

/// `screen.move(a, b); screen.line(c, e)`.
fn mls<D: DrawTarget<Color = Rgb565>>(d: &mut D, a: i32, b: i32, c: i32, e: i32) {
    let _ = Line::new(px(a, b), px(c, e)).into_styled(stroke()).draw(d);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use core::convert::Infallible;

    /// Records a hash and the bounding box of every drawn pixel.
    struct Rec {
        hash: u64,
        min: Point,
        max: Point,
        n: u32,
    }
    impl Rec {
        fn new() -> Rec {
            Rec { hash: 0xcbf29ce484222325, min: Point::new(i32::MAX, i32::MAX), max: Point::new(i32::MIN, i32::MIN), n: 0 }
        }
    }
    impl OriginDimensions for Rec {
        fn size(&self) -> Size {
            Size::new(360, 360)
        }
    }
    impl DrawTarget for Rec {
        type Color = Rgb565;
        type Error = Infallible;
        fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Infallible> {
            for Pixel(p, c) in pixels {
                self.hash = (self.hash ^ (p.x as u64) << 32 ^ (p.y as u64) << 16 ^ c.into_storage() as u64).wrapping_mul(0x100000001b3);
                self.min = Point::new(self.min.x.min(p.x), self.min.y.min(p.y));
                self.max = Point::new(self.max.x.max(p.x), self.max.y.max(p.y));
                self.n += 1;
            }
            Ok(())
        }
    }

    #[test]
    fn lights_follow_the_lua_table() {
        assert_eq!(lights(1), Lights { on: [true; 3], flares: [0; 3] });
        assert_eq!(lights(2), Lights { on: [false, true, true], flares: [0, 2, 0] });
        assert_eq!(lights(6), Lights { on: [false, true, false], flares: [0; 3] });
        assert_eq!(lights(7), Lights { on: [true, false, true], flares: [0, 0, 5] });
        assert_eq!(lights(0), Lights { on: [true; 3], flares: [0; 3] });
        assert_eq!(lights(9), lights(0));
    }

    #[test]
    fn tick_runs_at_5_hz_and_survives_odd_clocks() {
        let mut s = Scene::new(1);
        assert!(!s.tick(1_000_000), "first call only records the baseline");
        assert!(!s.tick(1_100_000));
        assert!(s.tick(1_200_000));
        assert!(!s.tick(1_200_001));
        assert!(s.tick(1_200_000 + 600_000_000), "a long gap yields a frame");
        assert!(!s.tick(1_200_000 + 600_000_001), "but only one, no catch-up");
        assert!(!s.tick(0), "clock going backwards re-baselines");
        assert!(s.tick(FRAME_US));
    }

    #[test]
    fn reset_forgets_the_baseline() {
        let mut s = Scene::new(1);
        s.tick(0);
        s.reset();
        assert!(!s.tick(10_000_000), "after reset the next call is a baseline again");
    }

    #[test]
    fn birds_stay_in_the_drift_box() {
        let mut s = Scene::new(7);
        s.tick(0);
        for i in 1..=2000 {
            s.tick(i * FRAME_US);
            let (x, y) = s.bird_pos();
            assert!((20..=30).contains(&x) && (22..=28).contains(&y), "({x},{y}) at frame {i}");
        }
    }

    #[test]
    fn uap_appears_and_ends() {
        let mut s = Scene::new(3);
        s.tick(0);
        let (mut seen, mut streak, mut longest) = (false, 0, 0);
        for i in 1..=400 {
            s.tick(i * FRAME_US);
            if s.uap_active() {
                seen = true;
            }
            // Segment 0 (the last dot) ends a flight; a new one may start right after.
            if matches!(s.uap_seg, Some(seg) if seg != 0) {
                streak += 1;
                longest = longest.max(streak);
            } else {
                streak = 0;
            }
        }
        assert!(seen, "a UAP shows up within 400 frames");
        assert!(longest <= 4, "a flight ends at its last dot within 4 frames, got {longest}");
    }

    #[test]
    fn same_seed_same_picture_other_seed_differs() {
        let pic = |seed| {
            let mut r = Rec::new();
            let mut s = Scene::new(seed);
            s.tick(0);
            for i in 1..=60 {
                s.tick(i * FRAME_US);
                s.draw_all(&mut r);
            }
            r.hash
        };
        assert_eq!(pic(5), pic(5));
        assert_ne!(pic(5), pic(6));
    }

    #[test]
    fn drawing_stays_inside_the_scene_rect() {
        let area = SCENE_RECT;
        let (lo, hi) = (area.top_left, area.bottom_right().unwrap());
        let mut s = Scene::new(11);
        s.tick(0);
        for i in 1..=300 {
            s.tick(i * FRAME_US);
            let mut r = Rec::new();
            s.draw_all(&mut r);
            assert!(r.n > 0);
            assert!(r.min.x >= lo.x && r.min.y >= lo.y && r.max.x <= hi.x && r.max.y <= hi.y, "frame {i}: {:?}..{:?}", r.min, r.max);
            for d in s.dirty_rects() {
                let br = d.bottom_right().unwrap();
                assert!(d.top_left.x >= lo.x && d.top_left.y >= lo.y && br.x <= hi.x && br.y <= hi.y);
            }
        }
    }

    #[test]
    fn dirty_rects_cover_everything_that_moves() {
        // Pixels that differ between consecutive frames must lie inside the dirty rects.
        let mut s = Scene::new(9);
        s.tick(0);
        let mut prev = Frame::new();
        s.draw_all(&mut prev);
        let mut moved = 0;
        for i in 1..=100 {
            s.tick(i * FRAME_US);
            let mut cur = Frame::new();
            s.draw_all(&mut cur);
            let rects = s.dirty_rects();
            for ((a, b), (x, y)) in prev.0.iter().zip(cur.0.iter()).zip(Frame::coords()) {
                if a != b {
                    moved += 1;
                    let pt = Point::new(x, y);
                    assert!(rects.iter().any(|r| r.contains(pt)), "frame {i}: changed pixel {pt:?} outside dirty rects");
                }
            }
            prev = cur;
        }
        assert!(moved > 0, "the scene animates");
    }

    /// The scene area as a bitmap.
    struct Frame(Vec<u16>);
    impl Frame {
        fn new() -> Frame {
            Frame(alloc::vec![0; 256 * 128])
        }
        fn coords() -> impl Iterator<Item = (i32, i32)> {
            (0..128).flat_map(|y| (0..256).map(move |x| (x + 52, y + 60)))
        }
    }
    impl OriginDimensions for Frame {
        fn size(&self) -> Size {
            Size::new(360, 360)
        }
    }
    impl DrawTarget for Frame {
        type Color = Rgb565;
        type Error = Infallible;
        fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Infallible> {
            for Pixel(p, c) in pixels {
                let (x, y) = (p.x - 52, p.y - 60);
                if (0..256).contains(&x) && (0..128).contains(&y) {
                    self.0[(y * 256 + x) as usize] = c.into_storage();
                }
            }
            Ok(())
        }
    }
}
