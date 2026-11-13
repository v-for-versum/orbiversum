use orbiversum_core::{biquad::Biquad, delay::DelayC, moog::MoogLadder};

const SR: f32 = 48000.0;
const N: usize = 8192;

fn input() -> Vec<f32> {
    (0..N)
        .map(|i| {
            let t = i as f32 / SR;
            (2.0 * core::f32::consts::PI * 330.0 * t).sin() * 0.6
                + (((i * 2654435761usize) >> 7) % 2000) as f32 / 1000.0 * 0.2
                - 0.2
        })
        .collect()
}

struct Chain {
    lp: Biquad,
    hp: Biquad,
    moog: MoogLadder,
    dl: DelayC,
}

impl Chain {
    fn new() -> Chain {
        Chain {
            lp: Biquad::lowpass(SR, 3000.0, 0.9),
            hp: Biquad::highpass(SR, 80.0, 0.7),
            moog: MoogLadder::new(SR),
            dl: DelayC::new(SR, 0.05),
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let a = self.lp.tick(x);
        let b = self.hp.tick(a);
        let c = self.moog.tick(b, 1200.0, 2.5);
        self.dl.tick(c, 12.3 / SR)
    }
}

fn run_chunked(chunk: usize) -> Vec<f32> {
    let x = input();
    let mut c = Chain::new();
    let mut out = Vec::with_capacity(N);
    for block in x.chunks(chunk) {
        let mut buf = block.to_vec();
        for s in buf.iter_mut() {
            *s = c.tick(*s);
        }
        out.extend_from_slice(&buf);
    }
    out
}

#[test]
fn block_size_independence() {
    let whole = run_chunked(N);
    assert!(whole.iter().any(|v| v.abs() > 1e-3), "output is silent");
    for chunk in [1usize, 7, 1024] {
        let got = run_chunked(chunk);
        assert_eq!(got.len(), whole.len());
        for (i, (a, b)) in got.iter().zip(&whole).enumerate() {
            assert_eq!(a, b, "chunk {chunk} differs at {i}");
        }
    }
}
