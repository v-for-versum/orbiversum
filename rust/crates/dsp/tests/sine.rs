use dsp::sine::Sine;
use dsp::voice::Voice;

#[test]
fn sine_matches_reference_frequency() {
    let sr = 48_000.0f32;
    let mut s = Sine::new(sr);
    s.set_params(440.0, 1.0);
    let n = 48_000;
    let mut out = vec![[0.0f32; 2]; n];
    s.process(&mut out);
    // direct DFT over 400..480 Hz (1 Hz bins, window = 1 s)
    let mut best = (0usize, 0.0f64);
    for k in 400..=480usize {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, f) in out.iter().enumerate() {
            let a = -2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
            re += f[0] as f64 * a.cos();
            im += f[0] as f64 * a.sin();
        }
        let m = re * re + im * im;
        if m > best.1 {
            best = (k, m);
        }
    }
    assert!((best.0 as i32 - 440).abs() <= 1, "peak bin {}", best.0);
    assert!(out.iter().flatten().all(|x| x.abs() <= 1.0));
}
