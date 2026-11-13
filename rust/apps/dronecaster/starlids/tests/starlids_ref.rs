//! Spectral/RMS comparison of Starlids against a golden captured from the
//! pre-optimisation implementation (libm tanh, 2x-oversampled Moog), seeds 5 and 9,
//! 110 Hz, amp 0.8, 4 s at 48 kHz.

use dronecaster_starlids::Starlids;
use orbiversum_core::voice::Voice;

const SR: f32 = 48_000.0;
const SEEDS: [u64; 2] = [5, 9];
/// DFT probe frequencies (log-spaced, Hz).
const FREQS: [f32; 12] =
    [60.0, 110.0, 220.0, 440.0, 880.0, 1500.0, 2500.0, 4000.0, 6000.0, 9000.0, 13000.0, 18000.0];

fn render(seed: u64) -> Vec<[f32; 2]> {
    let mut v = Starlids::new(seed, SR);
    v.set_params(110.0, 0.8);
    let mut out = vec![[0.0f32; 2]; 4 * 48_000];
    for c in out.chunks_mut(128) {
        v.process(c);
    }
    out
}

/// Mean power (dB) of the left channel at each probe frequency (a direct windowed DFT
/// correlation at that one frequency, not a Goertzel recursion), averaged
/// over 8192-sample Hann windows; plus RMS (dB) of both channels.
fn stats(out: &[[f32; 2]]) -> (Vec<f64>, f64) {
    let x: Vec<f64> = out[24_000..].iter().map(|f| f[0] as f64).collect();
    let n = 8192;
    let win: Vec<f64> =
        (0..n).map(|i| 0.5 - 0.5 * (2.0 * core::f64::consts::PI * i as f64 / n as f64).cos()).collect();
    let mut db = Vec::new();
    for &f in FREQS.iter() {
        let w = 2.0 * core::f64::consts::PI * f as f64 / SR as f64;
        let mut acc = 0.0;
        let mut cnt = 0;
        for seg in x.chunks_exact(n) {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, s) in seg.iter().enumerate() {
                re += s * win[i] * (w * i as f64).cos();
                im += s * win[i] * (w * i as f64).sin();
            }
            acc += re * re + im * im;
            cnt += 1;
        }
        db.push(10.0 * (acc / cnt as f64 + 1e-20).log10());
    }
    let ms: f64 = out.iter().map(|f| (f[0] as f64).powi(2) + (f[1] as f64).powi(2)).sum::<f64>()
        / (2.0 * out.len() as f64);
    (db, 10.0 * (ms + 1e-20).log10())
}

const GOLDEN: [([f64; 12], f64); 2] = [
    ([36.13, 37.02, 30.97, 26.66, 18.21, 3.92, 6.85, -5.78, -15.24, -34.85, -45.95, -64.73], -19.219),
    ([36.15, 34.25, 35.46, 34.21, 22.81, 1.14, -0.78, -24.24, -36.99, -57.71, -72.34, -91.26], -19.967),
];
const RMS_TOL_DB: f64 = 0.5;
/// Two deliberate departures from the golden pipeline (real-time budget on the ESP32-S3):
///  1. the outer `tanh` before the Moog is gone (the ladder saturates its input itself,
///     so the old signal went through tanh twice);
///  2. the Moog runs ONE pass per sample instead of 2x oversampling.
///
/// Up to 6 kHz the spectrum stays within 1 dB. Above 6 kHz is the ladder's stopband
/// (60 dB and more under the peak): there the single-pass ladder rolls off differently
/// (up to ~9 dB at 18 kHz, where the level is -55 dB against +36 dB at the peak), so the
/// per-band bound is 10 dB; a summed-energy check (below) keeps that region honest.
const BAND_TOL_DB: f64 = 1.0;
const STOPBAND_HZ: f32 = 6000.0;
const STOPBAND_TOL_DB: f64 = 10.0;
/// Total power above 6 kHz (sum of the probes) may differ from the golden by at most this.
/// Actual: +2.0 dB (seed 5), +1.9 dB (seed 9).
const STOPBAND_SUM_TOL_DB: f64 = 3.0;

#[test]
fn print_stats() {
    for seed in SEEDS {
        let (db, rms) = stats(&render(seed));
        println!("SEED {seed} {db:.2?} rms {rms:.3}");
    }
}

#[test]
fn matches_golden() {
    for (i, seed) in SEEDS.into_iter().enumerate() {
        let (db, rms) = stats(&render(seed));
        let (g_db, g_rms) = GOLDEN[i];
        assert!((rms - g_rms).abs() < RMS_TOL_DB, "seed {seed}: rms {rms:.2} vs {g_rms:.2} dB");
        let sum = |v: &[f64]| {
            let p: f64 = FREQS.iter().zip(v).filter(|(f, _)| **f > STOPBAND_HZ).map(|(_, d)| 10f64.powf(d / 10.0)).sum();
            10.0 * p.log10()
        };
        let (s_new, s_gold) = (sum(&db), sum(&g_db));
        assert!((s_new - s_gold).abs() < STOPBAND_SUM_TOL_DB, "seed {seed}: >6 kHz power {s_new:.2} vs {s_gold:.2} dB");
        for (j, f) in FREQS.iter().enumerate() {
            let tol = if *f > STOPBAND_HZ { STOPBAND_TOL_DB } else { BAND_TOL_DB };
            assert!(
                (db[j] - g_db[j]).abs() < tol,
                "seed {seed} @ {f} Hz: {:.2} vs {:.2} dB",
                db[j],
                g_db[j]
            );
        }
    }
}
