use orbiversum_core::prof;

/// Profiler section: the control step (plane, LFOs, cutoffs, note layer).
pub const P_CONTROL: usize = 0;
/// Profiler section: sub sines, shared saws and the eight pad voices (lowpass, delay, pan mix).
pub const P_PAD: usize = 1;
/// Profiler section: the stereo Moog ladder on the pad.
pub const P_MOOG: usize = 2;
/// Profiler section: the Klank resonators (4 per channel).
pub const P_KLANK: usize = 3;
/// Profiler section: the piano string (noise burst, two combs, filters).
pub const P_PIANO: usize = 4;
/// Profiler section: PolyPerc (pulse through a Moog ladder, percussive envelope).
pub const P_PERC: usize = 5;
/// Profiler section: the stereo reverb and the output gain.
pub const P_REVERB: usize = 6;
/// Profiler section: final lowpass/highpass and the intro fade.
pub const P_FINAL: usize = 7;
/// Profile section names, same order as the `P_*` consts.
pub static ENO_NAMES: [&str; prof::SECTIONS] = ["control", "pad", "moog", "klank", "piano", "perc", "reverb", "final"];
