//! Host-side renderer: drone voice -> 48 kHz stereo 16-bit WAV.

use dsp::{sine::Sine, starlids::Starlids, voice::Voice};
use std::process::exit;

const SR: u32 = 48_000;
const BLOCK: usize = 128;

struct Args {
    drone: String,
    hz: f32,
    amp: f32,
    seconds: f32,
    seed: u64,
    out: String,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        drone: "starlids".into(),
        hz: 110.0,
        amp: 0.5,
        seconds: 10.0,
        seed: 1,
        out: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let v = it.next().ok_or(format!("missing value for {flag}"))?;
        let bad = |e: &dyn std::fmt::Display| format!("{flag}: {e}");
        match flag.as_str() {
            "--drone" => a.drone = v,
            "--hz" => a.hz = v.parse().map_err(|e| bad(&e))?,
            "--amp" => a.amp = v.parse().map_err(|e| bad(&e))?,
            "--seconds" => a.seconds = v.parse().map_err(|e| bad(&e))?,
            "--seed" => a.seed = v.parse().map_err(|e| bad(&e))?,
            "--out" => a.out = v,
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    if a.out.is_empty() {
        return Err("--out is required".into());
    }
    Ok(a)
}

fn run(a: Args) -> Result<f32, String> {
    let sr = SR as f32;
    let mut voice: Box<dyn Voice> = match a.drone.as_str() {
        "starlids" => Box::new(Starlids::new(a.seed, sr)),
        "sine" => Box::new(Sine::new(sr)),
        other => return Err(format!("unknown drone {other} (starlids|sine)")),
    };
    voice.set_sample_rate(sr);
    voice.set_params(a.hz, a.amp);

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&a.out, spec).map_err(|e| e.to_string())?;
    let total = (a.seconds * sr).round() as usize;
    let mut buf = [[0.0f32; 2]; BLOCK];
    let (mut done, mut peak) = (0usize, 0.0f32);
    while done < total {
        let n = BLOCK.min(total - done);
        voice.process(&mut buf[..n]);
        for f in &buf[..n] {
            for &s in f {
                peak = peak.max(s.abs());
                let q = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                w.write_sample(q).map_err(|e| e.to_string())?;
            }
        }
        done += n;
    }
    w.finalize().map_err(|e| e.to_string())?;
    Ok(peak)
}

fn main() {
    let res = parse().and_then(run);
    match res {
        Ok(peak) => eprintln!("peak {peak:.4}"),
        Err(e) => {
            eprintln!("error: {e}");
            exit(2);
        }
    }
}
