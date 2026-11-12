//! Lightweight Freeverb (as SC FreeVerb): per channel 4 lowpass-feedback combs
//! in parallel, then 2 allpasses in series. Channels are independent.
//! Buffers have exact lengths and indices wrap by compare (no modulo, no division in `tick`).

use alloc::vec;
use alloc::vec::Vec;

const COMB_LEN: [usize; 4] = [1116, 1277, 1422, 1557];
const AP_LEN: [usize; 2] = [556, 341];
const FIXED_GAIN: f32 = 0.015;
const AP_FB: f32 = 0.5;

fn scaled(n: usize, sr: f32) -> usize {
    let r = (n as f32 * sr * (1.0 / 44100.0) + 0.5) as usize;
    r.max(2)
}

struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Comb {
        Comb { buf: vec![0.0; len], i: 0, store: 0.0 }
    }

    #[inline(always)]
    fn tick(&mut self, x: f32, fb: f32, d1: f32, d2: f32) -> f32 {
        let out = self.buf[self.i];
        self.store = out * d2 + self.store * d1;
        self.buf[self.i] = x + self.store * fb;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    fn new(len: usize) -> Allpass {
        Allpass { buf: vec![0.0; len], i: 0 }
    }

    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = x + b * AP_FB;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        b - x
    }
}

struct Channel {
    combs: [Comb; 4],
    aps: [Allpass; 2],
}

impl Channel {
    fn new(sr: f32) -> Channel {
        Channel {
            combs: [
                Comb::new(scaled(COMB_LEN[0], sr)),
                Comb::new(scaled(COMB_LEN[1], sr)),
                Comb::new(scaled(COMB_LEN[2], sr)),
                Comb::new(scaled(COMB_LEN[3], sr)),
            ],
            aps: [Allpass::new(scaled(AP_LEN[0], sr)), Allpass::new(scaled(AP_LEN[1], sr))],
        }
    }

    #[inline(always)]
    fn tick(&mut self, x: f32, fb: f32, d1: f32, d2: f32) -> f32 {
        let inp = x * FIXED_GAIN;
        let mut s = 0.0;
        for c in self.combs.iter_mut() {
            s += c.tick(inp, fb, d1, d2);
        }
        for a in self.aps.iter_mut() {
            s = a.tick(s);
        }
        s
    }

    fn mem_bytes(&self) -> usize {
        let n: usize = self.combs.iter().map(|c| c.buf.len()).sum::<usize>() + self.aps.iter().map(|a| a.buf.len()).sum::<usize>();
        n * 4
    }
}

pub struct ReverbLite {
    ch: [Channel; 2],
    mix: f32,
    fb: f32,
    d1: f32,
    d2: f32,
}

impl ReverbLite {
    /// Allocates all buffers; `tick` never allocates. `room` and `damp` are clamped to [0, 1].
    pub fn new(sr: f32, mix: f32, room: f32, damp: f32) -> ReverbLite {
        let room = if room.is_nan() { 0.0 } else { room.clamp(0.0, 1.0) };
        let damp = if damp.is_nan() { 0.0 } else { damp.clamp(0.0, 1.0) };
        let mix = if mix.is_nan() { 0.0 } else { mix.clamp(0.0, 1.0) };
        let d1 = damp * 0.4;
        ReverbLite { ch: [Channel::new(sr), Channel::new(sr)], mix, fb: room * 0.28 + 0.7, d1, d2: 1.0 - d1 }
    }

    /// Start address of the first delay buffer (for memory-placement diagnostics).
    pub fn buf_ptr(&self) -> *const u8 {
        self.ch[0].combs[0].buf.as_ptr().cast()
    }

    /// Total bytes of delay memory.
    pub fn mem_bytes(&self) -> usize {
        self.ch[0].mem_bytes() + self.ch[1].mem_bytes()
    }

    #[inline(always)]
    pub fn tick(&mut self, l: f32, r: f32) -> [f32; 2] {
        let (fb, d1, d2, m) = (self.fb, self.d1, self.d2, self.mix);
        let wl = self.ch[0].tick(l, fb, d1, d2);
        let wr = self.ch[1].tick(r, fb, d1, d2);
        [l * (1.0 - m) + wl * m, r * (1.0 - m) + wr * m]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    #[test]
    fn reverb_dry_wet() {
        let mut v = ReverbLite::new(SR, 0.0, 0.5, 0.5);
        for i in 0..3000 {
            let (a, b) = (libm::sinf(i as f32 * 0.1), libm::cosf(i as f32 * 0.07));
            assert_eq!(v.tick(a, b), [a, b]);
        }
        let mut v = ReverbLite::new(SR, 1.0, 0.5, 0.5);
        assert_eq!(v.tick(1.0, 1.0), [0.0, 0.0]);
    }

    #[test]
    fn reverb_tail_decays_and_stays_finite() {
        let mut v = ReverbLite::new(SR, 0.45, 2.0, 0.5);
        let n = 20 * 48000;
        let mut e = [0.0f64; 3]; // first 0.5 s, 0..10 s, 10..20 s
        let mut first = None;
        for i in 0..n {
            let y = v.tick(if i == 0 { 1.0 } else { 0.0 }, 0.0)[0];
            assert!(y.is_finite(), "i={i}");
            let w = (y as f64) * (y as f64);
            if i < 24000 {
                e[0] += w;
            }
            if i < 480000 {
                e[1] += w;
            } else {
                e[2] += w;
            }
            // skip i == 0: the dry part of the impulse itself
            if i > 0 && first.is_none() && y != 0.0 {
                first = Some(i);
            }
        }
        assert!(e[2] < e[1], "{e:?}");
        assert!(e[2] < 1e-3 * e[0], "{e:?}");
        let f = first.unwrap();
        assert!((f as i64 - 1214).abs() <= 2, "first echo at {f}");
    }

    #[test]
    fn reverb_channels_independent() {
        let mut v = ReverbLite::new(SR, 0.45, 2.0, 0.5);
        for i in 0..50000 {
            let [_, r] = v.tick(if i == 0 { 1.0 } else { 0.0 }, 0.0);
            assert_eq!(r, 0.0, "i={i}");
        }
    }

    #[test]
    fn reverb_loud_input_stays_bounded() {
        let mut v = ReverbLite::new(SR, 0.45, 2.0, 0.5);
        let mut s = 0x1234_5678u32;
        let mut peak = 0.0f32;
        for _ in 0..60 * 48000 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let a = (s as i32) as f32 * (1.0 / 2147483648.0);
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let b = (s as i32) as f32 * (1.0 / 2147483648.0);
            let [l, r] = v.tick(a, b);
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak < 4.0, "peak {peak}");
    }

    #[test]
    fn reverb_memory_budget() {
        let v = ReverbLite::new(SR, 0.45, 2.0, 0.5);
        assert!(v.mem_bytes() <= 64 * 1024, "{}", v.mem_bytes());
    }

    #[test]
    fn reverb_other_sample_rates() {
        for sr in [44100.0, 8000.0] {
            let mut v = ReverbLite::new(sr, 0.45, 2.0, 0.5);
            for i in 0..20000 {
                let [l, r] = v.tick(if i % 500 == 0 { 1.0 } else { 0.0 }, 0.5);
                assert!(l.is_finite() && r.is_finite());
            }
        }
    }
}
