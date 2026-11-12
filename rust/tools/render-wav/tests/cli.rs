use std::process::Command;

#[test]
fn cli_writes_valid_wav() {
    let out = std::env::temp_dir().join(format!("render-wav-test-{}.wav", std::process::id()));
    let status = Command::new(env!("CARGO_BIN_EXE_render-wav"))
        .args(["--drone", "starlids", "--hz", "110", "--amp", "0.5"])
        .args(["--seconds", "1", "--seed", "1", "--out"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());

    let mut reader = hound::WavReader::open(&out).unwrap();
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 48_000);
    assert_eq!(spec.channels, 2);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples.len() / 2, 48_000);
    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
    assert!(peak > 100, "silent render, peak {peak}");
    std::fs::remove_file(&out).ok();
}
