//! Framebuffer display: applets draw into a full RGB565 framebuffer through
//! `os::Surface`; `flush` sends only the dirty regions to the panel in strips
//! that fit one SPI transfer. Everything here is host-testable; the panel
//! itself sits behind the `Panel` trait.
use alloc::vec;
use alloc::vec::Vec;
use embedded_graphics::pixelcolor::{raw::RawU16, Rgb565};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use os::{Dirty, Surface, MAX_DIRTY};

pub const WIDTH: usize = 360;
pub const HEIGHT: usize = 360;
/// Largest strip: 16 rows x 360 px = 11520 bytes, the SPI bus' max transfer
/// (`BSP_LCD_H_RES * CONFIG_BSP_LCD_BUFFER_LINES`).
pub const STRIP_PIXELS: usize = WIDTH * 16;

/// A screen rectangle in unsigned pixel coordinates (already clipped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub const SCREEN: Rect = Rect { x: 0, y: 0, w: WIDTH, h: HEIGHT };
    pub fn pixels(&self) -> usize {
        self.w * self.h
    }
}

/// Intersection of `r` with the `w` x `h` screen; `None` when empty.
pub fn clip_rect(r: &Rectangle, w: usize, h: usize) -> Option<Rect> {
    let x0 = (r.top_left.x as i64).max(0);
    let y0 = (r.top_left.y as i64).max(0);
    let x1 = (r.top_left.x as i64 + r.size.width as i64).min(w as i64);
    let y1 = (r.top_left.y as i64 + r.size.height as i64).min(h as i64);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Rect { x: x0 as usize, y: y0 as usize, w: (x1 - x0) as usize, h: (y1 - y0) as usize })
}

/// The regions to send for `dirty`: the whole screen when `is_full()`,
/// otherwise each listed rectangle clipped to the screen (empty ones dropped).
/// Returns the number of entries written to `out`.
pub fn dirty_regions(dirty: &Dirty, out: &mut [Rect; MAX_DIRTY]) -> usize {
    if dirty.is_full() {
        out[0] = Rect::SCREEN;
        return 1;
    }
    let mut n = 0;
    for r in dirty.rects() {
        if let Some(c) = clip_rect(r, WIDTH, HEIGHT) {
            out[n] = c;
            n += 1;
        }
    }
    n
}

/// Splits `r` into horizontal bands of whole rows, each at most `max_pixels`
/// pixels, in top-to-bottom order. Requires `r.w <= max_pixels`.
pub fn strips(r: Rect, max_pixels: usize) -> impl Iterator<Item = Rect> {
    assert!(r.w > 0 && r.w <= max_pixels, "rect wider than a strip");
    let rows = max_pixels / r.w;
    let mut y = r.y;
    let end = r.y + r.h;
    core::iter::from_fn(move || {
        if y >= end {
            return None;
        }
        let h = rows.min(end - y);
        let s = Rect { x: r.x, y, w: r.w, h };
        y += h;
        Some(s)
    })
}

/// Copies `r` out of the framebuffer into `out` in panel byte order (high
/// byte first in memory, i.e. each little-endian `u16` byte-swapped).
pub fn pack_strip(fb: &[u16], r: Rect, out: &mut [u16]) {
    debug_assert!(r.x + r.w <= WIDTH && r.y + r.h <= HEIGHT, "strip outside the screen: {r:?}");
    debug_assert!(out.len() >= r.pixels(), "strip buffer too small: {} < {}", out.len(), r.pixels());
    for (row, dst) in out.chunks_exact_mut(r.w).take(r.h).enumerate() {
        let start = (r.y + row) * WIDTH + r.x;
        for (d, s) in dst.iter_mut().zip(&fb[start..start + r.w]) {
            *d = s.swap_bytes();
        }
    }
}

/// The panel behind a `FbDisplay`.
pub trait Panel {
    /// Scratch buffer for one strip, at least `STRIP_PIXELS` long (on the
    /// device: internal, DMA-capable RAM).
    fn strip_buf(&mut self) -> &mut [u16];
    /// Sends the first `r.pixels()` entries of `strip_buf()` (already in panel
    /// byte order) to `r` and returns when the transfer is done.
    fn send(&mut self, r: Rect) -> Result<(), i32>;
}

pub struct FbDisplay<P: Panel> {
    fb: Vec<u16>,
    panel: P,
    pixels_sent: u64,
    send_errors: u32,
}

impl<P: Panel> FbDisplay<P> {
    pub fn new(panel: P) -> FbDisplay<P> {
        FbDisplay { fb: vec![0; WIDTH * HEIGHT], panel, pixels_sent: 0, send_errors: 0 }
    }
    pub fn panel(&self) -> &P {
        &self.panel
    }
    /// Pixels sent to the panel since creation.
    pub fn pixels_sent(&self) -> u64 {
        self.pixels_sent
    }
    pub fn send_errors(&self) -> u32 {
        self.send_errors
    }
    /// The framebuffer as raw RGB565 values (row-major, native order).
    pub fn framebuffer(&self) -> &[u16] {
        &self.fb
    }
}

impl<P: Panel> Surface for FbDisplay<P> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
    fn draw_pixels(&mut self, pixels: &mut dyn Iterator<Item = Pixel<Rgb565>>) {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < WIDTH && (p.y as usize) < HEIGHT {
                self.fb[p.y as usize * WIDTH + p.x as usize] = RawU16::from(c).into_inner();
            }
        }
    }
    fn fill_contiguous(&mut self, area: &Rectangle, colors: &mut dyn Iterator<Item = Rgb565>) {
        // Colours belong to the unclipped area, row-major; skip what falls outside.
        for p in area.points() {
            let Some(c) = colors.next() else { break };
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < WIDTH && (p.y as usize) < HEIGHT {
                self.fb[p.y as usize * WIDTH + p.x as usize] = RawU16::from(c).into_inner();
            }
        }
    }
    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) {
        let Some(r) = clip_rect(area, WIDTH, HEIGHT) else { return };
        let v = RawU16::from(color).into_inner();
        for y in r.y..r.y + r.h {
            self.fb[y * WIDTH + r.x..y * WIDTH + r.x + r.w].fill(v);
        }
    }
}

impl<P: Panel> os::Display for FbDisplay<P> {
    fn size(&self) -> (u32, u32) {
        (WIDTH as u32, HEIGHT as u32)
    }
    fn surface(&mut self) -> &mut dyn Surface {
        self
    }
    fn flush(&mut self, dirty: &Dirty) {
        let mut regions = [Rect::SCREEN; MAX_DIRTY];
        let n = dirty_regions(dirty, &mut regions);
        for region in &regions[..n] {
            for strip in strips(*region, STRIP_PIXELS) {
                pack_strip(&self.fb, strip, self.panel.strip_buf());
                match self.panel.send(strip) {
                    Ok(()) => self.pixels_sent += strip.pixels() as u64,
                    Err(_) => self.send_errors += 1,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use os::Display;

    fn rect(x: i32, y: i32, w: u32, h: u32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn dirty_rects_are_clipped_to_screen() {
        let c = |r| clip_rect(&r, WIDTH, HEIGHT);
        assert_eq!(c(rect(10, 20, 30, 40)), Some(Rect { x: 10, y: 20, w: 30, h: 40 }));
        assert_eq!(c(rect(-5, -7, 20, 20)), Some(Rect { x: 0, y: 0, w: 15, h: 13 }));
        assert_eq!(c(rect(350, 350, 50, 50)), Some(Rect { x: 350, y: 350, w: 10, h: 10 }));
        assert_eq!(c(rect(-10, -10, 1000, 1000)), Some(Rect::SCREEN));
        assert_eq!(c(rect(360, 0, 5, 5)), None);
        assert_eq!(c(rect(0, 360, 5, 5)), None);
        assert_eq!(c(rect(-20, 0, 20, 5)), None);
        assert_eq!(c(rect(5, 5, 0, 9)), None);
        // Huge sizes do not overflow.
        assert_eq!(c(rect(i32::MAX, 0, u32::MAX, 1)), None);
        assert_eq!(c(rect(i32::MIN, 0, u32::MAX, 1)), Some(Rect { x: 0, y: 0, w: WIDTH, h: 1 }));

        let mut d = Dirty::new();
        d.add(rect(-5, -5, 10, 10));
        d.add(rect(400, 400, 10, 10));
        d.add(rect(100, 100, 1, 1));
        let mut out = [Rect::SCREEN; MAX_DIRTY];
        let n = dirty_regions(&d, &mut out);
        assert_eq!(&out[..n], &[Rect { x: 0, y: 0, w: 5, h: 5 }, Rect { x: 100, y: 100, w: 1, h: 1 }]);
        let mut full = Dirty::new();
        full.set_full();
        let n = dirty_regions(&full, &mut out);
        assert_eq!(&out[..n], &[Rect::SCREEN]);
        assert_eq!(dirty_regions(&Dirty::new(), &mut out), 0);
    }

    fn coverage(r: Rect, max: usize) -> Vec<u8> {
        let mut grid = vec![0u8; WIDTH * HEIGHT];
        let mut prev_end = r.y;
        for s in strips(r, max) {
            assert!(s.pixels() <= max && s.pixels() > 0, "strip {s:?}");
            assert_eq!((s.x, s.w), (r.x, r.w));
            assert_eq!(s.y, prev_end, "strips must be consecutive");
            prev_end = s.y + s.h;
            for y in s.y..s.y + s.h {
                for x in s.x..s.x + s.w {
                    grid[y * WIDTH + x] += 1;
                }
            }
        }
        assert_eq!(prev_end, r.y + r.h);
        grid
    }

    #[test]
    fn strip_splitting_covers_rect_exactly() {
        let cases = [
            Rect::SCREEN,
            Rect { x: 0, y: 0, w: 1, h: 1 },
            Rect { x: 60, y: 150, w: 240, h: 30 },
            Rect { x: 13, y: 7, w: 347, h: 353 },
            Rect { x: 359, y: 0, w: 1, h: 360 },
            Rect { x: 0, y: 359, w: 360, h: 1 },
            Rect { x: 5, y: 5, w: 100, h: 17 },
        ];
        for r in cases {
            for max in [STRIP_PIXELS, 1000, 360, 100] {
                if r.w > max {
                    continue;
                }
                let grid = coverage(r, max);
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        let inside = x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
                        assert_eq!(grid[y * WIDTH + x], inside as u8, "{r:?} max {max} at {x},{y}");
                    }
                }
            }
        }
        // The full screen is 23 strips of 16 rows (360 = 22*16 + 8).
        assert_eq!(strips(Rect::SCREEN, STRIP_PIXELS).count(), 23);
    }

    #[test]
    fn pack_strip_copies_rows_in_panel_byte_order() {
        let mut fb = vec![0u16; WIDTH * HEIGHT];
        fb[2 * WIDTH + 3] = 0x1234;
        fb[2 * WIDTH + 4] = 0xABCD;
        fb[3 * WIDTH + 3] = 0x00FF;
        let mut out = [0u16; 4];
        pack_strip(&fb, Rect { x: 3, y: 2, w: 2, h: 2 }, &mut out);
        assert_eq!(out, [0x3412, 0xCDAB, 0xFF00, 0x0000]);
    }

    struct MockPanel {
        buf: Vec<u16>,
        sent: Vec<(Rect, Vec<u16>)>,
    }
    impl Panel for MockPanel {
        fn strip_buf(&mut self) -> &mut [u16] {
            &mut self.buf
        }
        fn send(&mut self, r: Rect) -> Result<(), i32> {
            self.sent.push((r, self.buf[..r.pixels()].to_vec()));
            Ok(())
        }
    }
    fn display() -> FbDisplay<MockPanel> {
        FbDisplay::new(MockPanel { buf: vec![0; STRIP_PIXELS], sent: Vec::new() })
    }
    fn px(fb: &FbDisplay<MockPanel>, x: usize, y: usize) -> u16 {
        fb.framebuffer()[y * WIDTH + x]
    }

    #[test]
    fn surface_drawing_is_clipped_and_never_panics() {
        let mut d = display();
        let red = Rgb565::new(31, 0, 0);
        Surface::fill_solid(&mut d, &rect(-10, -10, 20, 20), red);
        assert_eq!(px(&d, 0, 0), RawU16::from(red).into_inner());
        assert_eq!(px(&d, 9, 9), RawU16::from(red).into_inner());
        assert_eq!(px(&d, 10, 10), 0);
        Surface::fill_solid(&mut d, &rect(355, 355, 100, 100), red);
        assert_eq!(px(&d, 359, 359), RawU16::from(red).into_inner());

        let mut d = display();
        let mut pixels = [
            Pixel(Point::new(-1, 5), red),
            Pixel(Point::new(5, -1), red),
            Pixel(Point::new(360, 5), red),
            Pixel(Point::new(5, 360), red),
            Pixel(Point::new(100, 100), red),
        ]
        .into_iter();
        d.draw_pixels(&mut pixels);
        assert_eq!(px(&d, 100, 100), RawU16::from(red).into_inner());
        assert_eq!(px(&d, 5, 5), 0);

        // fill_contiguous: colours map to the *unclipped* area, row-major.
        let mut d = display();
        let cols: Vec<Rgb565> = (0..9u16).map(|i| Rgb565::new(i as u8 + 1, 0, 0)).collect();
        d.fill_contiguous(&rect(-1, -1, 3, 3), &mut cols.iter().copied());
        // Visible part of the 3x3 area is its bottom-right 2x2: colours 5,6,8,9.
        assert_eq!(px(&d, 0, 0), RawU16::from(cols[4]).into_inner());
        assert_eq!(px(&d, 1, 0), RawU16::from(cols[5]).into_inner());
        assert_eq!(px(&d, 0, 1), RawU16::from(cols[7]).into_inner());
        assert_eq!(px(&d, 1, 1), RawU16::from(cols[8]).into_inner());
        assert_eq!(px(&d, 2, 0), 0);
        // Short colour iterator is fine.
        d.fill_contiguous(&rect(0, 0, 10, 10), &mut core::iter::empty());
    }

    #[test]
    fn flush_sends_only_dirty_regions() {
        let mut d = display();
        let white = Rgb565::new(31, 63, 31);
        Surface::fill_solid(&mut d, &rect(60, 150, 240, 30), white);
        let mut dirty = Dirty::new();
        dirty.add(rect(60, 150, 240, 30));
        d.flush(&dirty);
        let sent = &d.panel().sent;
        // 240 x 30 = 7200 px > one strip (5760): 24 rows + 6 rows.
        let rects: Vec<Rect> = sent.iter().map(|(r, _)| *r).collect();
        assert_eq!(
            rects,
            [Rect { x: 60, y: 150, w: 240, h: 24 }, Rect { x: 60, y: 174, w: 240, h: 6 }]
        );
        assert!(sent.iter().all(|(_, p)| p.iter().all(|&p| p == RawU16::from(white).into_inner().swap_bytes())));
        assert_eq!(d.pixels_sent(), 240 * 30);

        // A full redraw sends every pixel exactly once.
        let mut full = Dirty::new();
        full.set_full();
        let before = d.pixels_sent();
        d.flush(&full);
        assert_eq!(d.pixels_sent() - before, (WIDTH * HEIGHT) as u64);
        assert!(d.panel().sent.iter().all(|(r, _)| r.pixels() <= STRIP_PIXELS));
    }
}
