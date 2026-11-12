use dsp::starlids::Starlids;
use dsp::voice::Voice;

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
        for o in render(3, hz, 96_000, 256).iter().flatten() {
            assert!(o.is_finite() && o.abs() <= 1.0, "hz {hz}: {o}");
        }
    }
}

#[test]
fn starlids_block_size_independence() {
    let n = 30_000;
    let base = render(9, 220.0, n, 1024);
    for b in [1usize, 7, 128] {
        assert_eq!(render(9, 220.0, n, b), base, "block {b}");
    }
}
