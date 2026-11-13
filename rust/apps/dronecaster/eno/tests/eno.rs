use dronecaster_eno::Eno;
use orbiversum_core::voice::Voice;

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
    // multiples of the control step are bit-exact; others only differ by f32 rounding noise
    for b in [32usize, 64, 256] {
        assert!(base == run(b), "block {b} not bit-exact");
    }
    for b in [1usize, 37, 1000] {
        let o = run(b);
        for (k, (x, y)) in base.iter().zip(o.iter()).enumerate() {
            assert!((x[0] - y[0]).abs() <= 3e-5 && (x[1] - y[1]).abs() <= 3e-5, "block {b} sample {k}");
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
            assert!((x[0] - y[0]).abs() <= 3e-5 && (x[1] - y[1]).abs() <= 3e-5, "block {b} sample {k}");
        }
    }
}

fn rms(x: &[[f32; 2]]) -> f64 {
    let s: f64 = x.iter().map(|f| (f[0] as f64).powi(2) + (f[1] as f64).powi(2)).sum();
    (s / (2 * x.len()) as f64).sqrt()
}

fn sec(x: &[[f32; 2]], a: f32, b: f32) -> &[[f32; 2]] {
    &x[(a * SR) as usize..(b * SR) as usize]
}

#[test]
fn eno_intro_is_silent_then_fades_in() {
    let out = render(1, 330.0, 1.0, 5 * 48_000, 128);
    let peak = sec(&out, 0.0, 0.5).iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak < 1e-4, "intro peak {peak}");
    assert!(rms(sec(&out, 3.5, 4.5)) > rms(sec(&out, 0.5, 1.0)));
}

#[test]
fn eno_long_render_stays_finite() {
    let n = 180 * 48_000;
    let mut v = Eno::new(5, SR);
    v.set_params(330.0, 1.0);
    let mut out = vec![[0.0f32; 2]; n];
    for c in out.chunks_mut(128) {
        v.process(c);
    }
    let mut worst = 0.0f32;
    for w in out.chunks(48_000) {
        let mut peak = 0.0f32;
        for s in w.iter().flatten() {
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        worst = worst.max(peak);
        assert!(peak < 1.5, "window peak {peak}");
    }
    println!("long render peak {worst}, rms 10-20s {}, last 10s {}", rms(sec(&out, 10.0, 20.0)), rms(sec(&out, 170.0, 180.0)));
    let (a, b) = (rms(sec(&out, 10.0, 20.0)), rms(sec(&out, 170.0, 180.0)));
    assert!(b > 0.05 * a && b < 4.0 * a, "rms drift {a} -> {b}");
}

#[test]
fn eno_survives_sample_rate_change() {
    let mut v = Eno::new(2, SR);
    v.set_params(330.0, 1.0);
    v.set_sample_rate(44_100.0);
    let mut out = vec![[0.0f32; 2]; 5 * 44_100];
    for c in out.chunks_mut(128) {
        v.process(c);
    }
    assert!(out.iter().flatten().all(|x| x.is_finite()));
    let tail = &out[4 * 44_100..];
    assert!(tail.iter().flatten().any(|x| x.abs() > 1e-5), "silent after intro");
}

#[test]
fn eno_survives_param_changes() {
    let mut v = Eno::new(4, SR);
    let sets = [(330.0, 1.0), (110.0, 0.3), (0.0, 0.0), (660.0, 1.0)];
    let mut buf = [[0.0f32; 2]; 128];
    let mut peak = 0.0f32;
    for b in 0..(30 * 48_000 / 128) {
        let (h, a) = sets[(b / 20) % 4];
        v.set_params(h, a);
        v.process(&mut buf);
        assert!(buf.iter().flatten().all(|x| x.is_finite()), "block {b}");
        peak = peak.max(buf.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs())));
    }
    println!("param changes peak {peak}");
    assert!(peak <= 1.0, "peak {peak}");
}

#[test]
fn eno_dc_and_lows_are_controlled() {
    let out = render(6, 330.0, 1.0, 60 * 48_000, 128);
    let x: Vec<f64> = out[5 * 48_000..].iter().map(|f| f[0] as f64).collect();
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    assert!(mean.abs() < 0.01, "mean {mean}");
    let rms = (x.iter().map(|s| s * s).sum::<f64>() / x.len() as f64).sqrt();
    for f in [20.0f64, 30.0] {
        let w = 2.0 * core::f64::consts::PI * f / SR as f64;
        let n = 8192;
        let (mut acc, mut cnt) = (0.0, 0);
        for seg in x.chunks_exact(n) {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, s) in seg.iter().enumerate() {
                let h = 0.5 - 0.5 * (2.0 * core::f64::consts::PI * i as f64 / n as f64).cos();
                re += s * h * (w * i as f64).cos();
                im += s * h * (w * i as f64).sin();
            }
            acc += (re * re + im * im) / (n as f64 * n as f64 * 0.375);
            cnt += 1;
        }
        let amp = (acc / cnt as f64).sqrt();
        let db = 20.0 * (amp / rms).log10();
        assert!(db < -30.0, "{f} Hz at {db} dB rel RMS");
    }
}

#[test]
fn eno_return_from_silence_is_not_louder_than_steady() {
    let peak = |x: &[[f32; 2]]| x.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()));
    let mut v = Eno::new(8, SR);
    v.set_params(330.0, 1.0);
    let mut buf = [[0.0f32; 2]; 128];
    let mut steady = 0.0f32;
    for b in 0..(30 * 48_000 / 128) {
        v.process(&mut buf);
        if b >= 20 * 48_000 / 128 {
            steady = steady.max(peak(&buf));
        }
    }
    v.set_params(0.0, 0.0);
    for _ in 0..(20 * 48_000 / 128) {
        v.process(&mut buf);
        assert!(buf.iter().flatten().all(|&x| x == 0.0));
    }
    v.set_params(330.0, 1.0);
    let mut first = 0.0f32;
    for _ in 0..(48_000 / 128) {
        v.process(&mut buf);
        first = first.max(peak(&buf));
    }
    println!("steady {steady} first-second after silence {first}");
    assert!(first < 1.5 * steady.max(0.05), "first {first} steady {steady}");
}
