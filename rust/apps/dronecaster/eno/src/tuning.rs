//! Tunable constants and CPU-budget switches.

pub(crate) const CTRL: usize = 32;
/// Output trim: the layers peak at ~1.56 at amp 1, which the i16 conversion would hard-clip.
pub(crate) const OUT_TRIM: f32 = 0.6;
/// Sacrifice switches for the CPU budget (flipped by hand after a measurement).
/// Reverb on the mono sum with the same wet on both channels. Note: the current
/// `ReverbLite` has no mono tick, so this saves no CPU until it gets one.
pub(crate) const REVERB_SINGLE_CHANNEL: bool = false;
pub(crate) const KLANK_ENABLED: bool = true;
pub(crate) const REVERB_MIX: f32 = 0.45;
pub(crate) const KLANK_DECAY: f32 = 1.0;
pub(crate) const KLANK_EXCITE: f32 = 0.004;
pub(crate) const KLANK_GAIN: f32 = 0.55;
pub(crate) const INTRO_DELAY_S: f32 = 0.5;
pub(crate) const INTRO_RAMP_S: f32 = 3.0;
pub(crate) const HPF_HZ: f32 = 120.0;
pub(crate) const VOICES: usize = 8;
/// Each pad voice gets its control update every this many control steps (staggered across voices).
pub(crate) const PAD_CTRL_STRIDE: usize = 4;
pub(crate) const GROUP: usize = 4;
pub(crate) const GROUPS: usize = VOICES / GROUP;
const _: () = assert!(VOICES.is_multiple_of(GROUP) && GROUP == 4, "voice i uses shared saw i % 4");
pub(crate) const Q_BUTTERWORTH: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// Largest delay reached: (0.02 + 0.01) / 15 s = 2 ms; margin added.
pub(crate) const MAX_DELAY_S: f32 = 0.004;
pub(crate) const LOG2_5: f32 = 2.321_928;
pub(crate) const LOG2_30: f32 = 4.906_890_6;
/// LFNoise0.kr(1/3) for the pan.
pub(crate) const PAN_RATE: f32 = 1.0 / 3.0;
pub(crate) const PIANO_MAX_DELAY_S: f32 = 0.025;
/// The piano string loop is skipped this long after the last note: 8 s of a 6 s (-60 dB) decay is
/// below -80 dB, so the skipped tail is inaudible. Counted in control steps (block-size independent).
pub(crate) const PIANO_LIVE_S: f32 = 8.0;
