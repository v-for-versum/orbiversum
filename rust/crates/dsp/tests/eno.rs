use dsp::eno::Eno;
use dsp::voice::Voice;

const SR: f32 = 48_000.0;

fn render(seed: u64, hz: f32, amp: f32, n: usize, block: usize) -> Vec<[f32; 2]> {
    let mut v = Eno::new(seed, SR);
    v.set_params(hz, amp);
    let mut out = vec![[0.0f32; 2]; n];
    for c in out.chunks_mut(block) {
        v.process(c);
    }
    out
}

#[test]
fn eno_is_deterministic_per_seed() {
    assert_eq!(render(1, 330.0, 0.8, 4 * 48_000, 128), render(1, 330.0, 0.8, 4 * 48_000, 128));
}

#[test]
fn eno_seeds_differ() {
    assert_ne!(render(1, 330.0, 0.8, 4 * 48_000, 128), render(2, 330.0, 0.8, 4 * 48_000, 128));
}

#[test]
fn eno_block_size_independence() {
    // 8 s: the Impulse(0) trigger at t=0 plus TDelay 3 s switches the plane at ~3 s,
    // so every chunking crosses a chord change.
    let n = 8 * 48_000;
    let run = |block: usize| {
        let mut v = Eno::new(7, SR);
        v.set_params(330.0, 0.8);
        let mut out = vec![[0.0f32; 2]; n];
        let before = v.plane_notes();
        assert_eq!(before, [0; 4]);
        for c in out.chunks_mut(block) {
            v.process(c);
        }
        assert_ne!(v.plane_notes(), before, "plane must have switched (block {block})");
        out
    };
    let base = run(128);
    for b in [1usize, 37, 1000] {
        let o = run(b);
        for (k, (x, y)) in base.iter().zip(o.iter()).enumerate() {
            assert!((x[0] - y[0]).abs() <= 1e-6 && (x[1] - y[1]).abs() <= 1e-6, "block {b} sample {k}");
        }
    }
}

#[test]
fn eno_output_finite_and_bounded() {
    let out = render(1, 330.0, 1.0, 20 * 48_000, 512);
    let mut peak = 0.0f32;
    for s in out.iter().flatten() {
        assert!(s.is_finite());
        peak = peak.max(s.abs());
    }
    // pad is tanh-bounded; the note layers add headroom (peak ~0.93 at amp 1 after the output trim)
    assert!(peak < 1.0, "peak {peak}");
    assert!(peak > 0.01, "silent: {peak}");
}

#[test]
fn eno_handles_extreme_hz() {
    for hz in [0.0f32, 1.0, 8.0, 40.0, 330.0, 20_000.0, 1e9, f32::NAN, f32::INFINITY] {
        let out = render(3, hz, 1.0, 48_000, 128);
        assert!(out.iter().flatten().all(|x| x.is_finite()), "hz {hz}");
        if hz == 0.0 {
            assert!(out.iter().flatten().all(|&x| x == 0.0), "hz 0 must be silent");
        }
    }
}

#[test]
fn eno_amp_zero_is_silent() {
    let out = render(3, 330.0, 0.0, 48_000, 128);
    assert!(out.iter().flatten().all(|&x| x == 0.0));
}

#[test]
fn eno_notes_audible_over_time() {
    let out = render(5, 330.0, 1.0, 60 * 48_000, 512);
    let rms: Vec<f32> = out[5 * 48_000..]
        .chunks(48_000)
        .map(|w| (w.iter().map(|x| x[0] * x[0]).sum::<f32>() / w.len() as f32).sqrt())
        .collect();
    let max = rms.iter().cloned().fold(0.0f32, f32::max);
    let min = rms.iter().cloned().fold(f32::MAX, f32::min);
    assert!(min > 0.0, "silent window");
    assert!(max / min > 1.5, "no variation: min {min} max {max}");
}

#[test]
fn eno_extreme_hz_with_notes() {
    for hz in [8.0f32, 20_000.0, 1e9] {
        let out = render(9, hz, 1.0, 20 * 48_000, 512);
        assert!(out.iter().flatten().all(|x| x.is_finite() && x.abs() < 10.0), "hz {hz}");
    }
}

#[test]
fn eno_determinism_and_block_size_still_hold() {
    let n = 30 * 48_000;
    let base = render(4, 330.0, 0.8, n, 128);
    assert_eq!(base, render(4, 330.0, 0.8, n, 128));
    for b in [1usize, 37] {
        let o = render(4, 330.0, 0.8, n, b);
        for (k, (x, y)) in base.iter().zip(o.iter()).enumerate() {
            assert!((x[0] - y[0]).abs() <= 1e-5 && (x[1] - y[1]).abs() <= 1e-5, "block {b} sample {k}");
        }
    }
}
