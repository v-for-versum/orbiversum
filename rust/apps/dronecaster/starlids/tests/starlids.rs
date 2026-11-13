use dronecaster_starlids::Starlids;
use orbiversum_core::voice::Voice;

const SR: f32 = 48_000.0;

fn render(seed: u64, hz: f32, n: usize, block: usize) -> Vec<[f32; 2]> {
    let mut v = Starlids::new(seed, SR);
    v.set_params(hz, 0.8);
    let mut out = vec![[0.0f32; 2]; n];
    for c in out.chunks_mut(block) {
        v.process(c);
    }
    out
}

#[test]
fn starlids_is_deterministic_per_seed() {
    assert_eq!(render(5, 110.0, 20_000, 128), render(5, 110.0, 20_000, 128));
}

#[test]
fn starlids_seeds_differ() {
    assert_ne!(render(5, 110.0, 20_000, 128), render(6, 110.0, 20_000, 128));
}

#[test]
fn starlids_output_is_finite_and_bounded() {
    // 10 minutes per pitch in release; shorter in debug to keep `cargo test` usable.
    let secs: usize = if cfg!(debug_assertions) { 20 } else { 600 };
    for hz in [55.0f32, 110.0, 440.0] {
        let mut v = Starlids::new(1, SR);
        v.set_params(hz, 1.0);
        let mut buf = [[0.0f32; 2]; 512];
        let mut peak = 0.0f32;
        for _ in 0..(secs * 48_000 / 512) {
            v.process(&mut buf);
            for s in buf.iter().flatten() {
                assert!(s.is_finite(), "non-finite at {hz} Hz");
                peak = peak.max(s.abs());
            }
        }
        assert!(peak <= 1.0, "peak {peak} at {hz} Hz");
        assert!(peak > 0.01, "silent at {hz} Hz: {peak}");
    }
}

#[test]
fn starlids_handles_extreme_hz() {
    for hz in [0.0f32, 1.0, 20_000.0, 24_000.0] {
        let out = render(3, hz, 96_000, 256);
        for o in out.iter().flatten() {
            assert!(o.is_finite() && o.abs() <= 1.0, "hz {hz}: {o}");
        }
        if hz == 0.0 {
            assert!(out.iter().flatten().all(|&x| x == 0.0), "hz 0 must be silent");
        }
    }
}

#[test]
fn starlids_block_size_independence() {
    // 6 s; params change every 10_000 samples at the same absolute positions for every chunking.
    let n = 288_000;
    let seg = 10_000;
    let run = |block: usize| {
        let mut v = Starlids::new(9, SR);
        let mut out = vec![[0.0f32; 2]; n];
        for (i, s) in out.chunks_mut(seg).enumerate() {
            v.set_params(110.0 + 37.0 * (i % 5) as f32, 0.4 + 0.1 * (i % 5) as f32);
            for c in s.chunks_mut(block) {
                v.process(c);
            }
        }
        out
    };
    let base = run(1024);
    for b in [1usize, 7, 128] {
        assert!(run(b) == base, "block {b}");
    }
}
