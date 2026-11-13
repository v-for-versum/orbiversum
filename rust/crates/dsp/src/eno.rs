//! Eno: port of dronecaster's Eno.scd (milestone 1: pad, sub sines, tanh, Moog).
//!
//! Control-rate parts (plane changes, LFOs, cutoffs, pan) update every `CTRL`
//! samples at fixed absolute positions, so the output does not depend on the
//! caller's block size.

use crate::prof;
use crate::voice::Voice;
use alloc::vec::Vec;
use blocks::{
    biquad::Biquad,
    comb::{CombL, DelayL},
    env::{Decay2, EnvPerc},
    fast::{exp2, recip},
    lfo::{Noise0, Noise1, VarLagSine},
    moog::{MoogLadder, MoogStereo},
    noise::{Noise2, PinkNoise},
    osc::{Pulse, SawDpw, SinOsc},
    pan::pan2,
    resonant::{RLpf, Ringz},
    reverb::ReverbLite,
    rng::Rng,
    seq::{Dseq, Dust, Dxrand, Impulse, TChoose, TDelay},
};

pub const P_CONTROL: usize = 0;
pub const P_PAD: usize = 1;
pub const P_MOOG: usize = 2;
pub const P_KLANK: usize = 3;
pub const P_PIANO: usize = 4;
pub const P_PERC: usize = 5;
pub const P_REVERB: usize = 6;
pub const P_FINAL: usize = 7;
/// Profile section names, same order as the `P_*` consts.
pub static ENO_NAMES: [&str; prof::SECTIONS] = ["control", "pad", "moog", "klank", "piano", "perc", "reverb", "final"];

const CTRL: usize = 32;
/// Output trim: the layers peak at ~1.56 at amp 1, which the i16 conversion would hard-clip.
const OUT_TRIM: f32 = 0.6;
/// Sacrifice switches for the CPU budget (flipped by hand after a measurement).
/// Reverb on the mono sum with the same wet on both channels. Note: the current
/// `ReverbLite` has no mono tick, so this saves no CPU until it gets one.
const REVERB_SINGLE_CHANNEL: bool = false;
const KLANK_ENABLED: bool = true;
const REVERB_MIX: f32 = 0.45;
const KLANK_DECAY: f32 = 1.0;
const KLANK_EXCITE: f32 = 0.004;
const KLANK_GAIN: f32 = 0.55;
const INTRO_DELAY_S: f32 = 0.5;
const INTRO_RAMP_S: f32 = 3.0;
const HPF_HZ: f32 = 120.0;
const VOICES: usize = 8;
/// Each pad voice gets its control update every this many control steps (staggered across voices).
const PAD_CTRL_STRIDE: usize = 4;
const GROUP: usize = 4;
const GROUPS: usize = VOICES / GROUP;
const _: () = assert!(VOICES % GROUP == 0 && GROUP == 4, "voice i uses shared saw i % 4");
const Q_BUTTERWORTH: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// Largest delay reached: (0.02 + 0.01) / 15 s = 2 ms; margin added.
const MAX_DELAY_S: f32 = 0.004;
const LOG2_5: f32 = 2.321_928_1;
const LOG2_30: f32 = 4.906_890_6;
/// LFNoise0.kr(1/3) for the pan.
const PAN_RATE: f32 = 1.0 / 3.0;

/// Note-rate choices ([0.02,0.05,1,2,0.5,0.25,2] / 2).
const NOTE_RATES: [f32; 7] = [0.01, 0.025, 0.5, 1.0, 0.25, 0.125, 1.0];
const AIRPORTS: &[&[i8]] = &[
    &[5, 7, 4, 2, 0, 12, 7, 5, 7, 4, 2, 0],
    &[5, 7, 4, 2, 0, 12, 4, 7, 5, 0],
    &[-5, 2, 0, 4, 7, 12, 5, 2, 7, 4, 0, 7, 2, 5, 5, 2, 4, 0],
    &[7, 7, 2, 4, 4, 4, 2, 0, 7, 0, 0],
];
const TUNE_UP: f32 = 1.0005;
const TUNE_DOWN: f32 = 0.9996;
const STRING_DECAY: f32 = 6.0;
const PIANO_MAX_DELAY_S: f32 = 0.025;
/// The piano string loop is skipped this long after the last note: 8 s of a 6 s (-60 dB) decay is
/// below -80 dB, so the skipped tail is inaudible. Counted in control steps (block-size independent).
const PIANO_LIVE_S: f32 = 8.0;

const PLANES: [[i8; 4]; 4] = [[0, 4, 7, 12], [4, 7, 11, 16], [-3, 0, 4, 7], [-3, 0, 5, 9]];

fn child_rng(rng: &mut Rng) -> Rng {
    let hi = (rng.f32() * 16_777_216.0) as u64;
    let lo = (rng.f32() * 16_777_216.0) as u64;
    Rng::new((hi << 24) | lo)
}

struct PadVoice {
    lfo: SinOsc,
    f_lfo: f32,
    lpf: Biquad,
    delay: DelayL,
    dly_noise: Noise1,
    f_dly: f32,
    pan_noise: Noise0,
    pan_lag: VarLagSine,
    gain: [f32; 2],
    /// pan position the cached `gain` belongs to (NaN = none)
    pan_c: f32,
}

impl PadVoice {
    fn new(rng: &mut Rng, sr: f32) -> PadVoice {
        let f_lfo = rng.range(1.0 / 30.0, 1.0 / 10.0);
        let ph = rng.f32();
        let f_dly = rng.range(5.0, 10.0);
        PadVoice {
            lfo: SinOsc::new(sr, ph),
            f_lfo,
            lpf: Biquad::lowpass(sr, 1000.0, Q_BUTTERWORTH),
            delay: DelayL::new(sr, MAX_DELAY_S),
            dly_noise: Noise1::new(child_rng(rng), sr),
            f_dly,
            pan_noise: Noise0::new(child_rng(rng), sr),
            pan_lag: VarLagSine::new(sr),
            gain: [core::f32::consts::FRAC_1_SQRT_2; 2],
            pan_c: f32::NAN,
        }
    }

    /// Runs once every `PAD_CTRL_STRIDE` control steps, so one call spans that many steps of
    /// time: every generator below advances by `c` samples per call.
    fn control(&mut self, hz: f32, max_hz: f32) {
        let c = (CTRL * PAD_CTRL_STRIDE) as f32;
        let s = self.lfo.tick(self.f_lfo * c);
        self.lpf.set_lowpass((hz * exp2(LOG2_5 * (s + 1.0) * 0.5)).min(max_hz));
        let d = (0.02 + 0.01 * self.dly_noise.tick(self.f_dly * c)) * (1.0 / 15.0);
        self.delay.set_delay(d);
        let p = self.pan_noise.tick(PAN_RATE * c);
        let p = self.pan_lag.tick(p, 3.0 / c);
        if p != self.pan_c {
            self.pan_c = p;
            self.gain = pan2(1.0, p);
        }
    }
}

pub struct Eno {
    seed: u64,
    sr: f32,
    /// control step in seconds (CTRL / sr), computed once
    dt: f32,
    hz: f32,
    amp: f32,
    silent: bool,
    sub_f_hz: [f32; 2],
    /// raw `hz` input bits the derived values belong to
    hz_key: u32,
    pos: usize,
    max_hz: f32,
    // plane sequencing
    dust: Dust,
    impulse: Impulse,
    plane_env: EnvPerc,
    plane_delay: TDelay,
    plane_pick: Dxrand,
    plane_notes: [i8; 4],
    plane_gain: f32,
    // pad
    saws: [SawDpw; 4],
    voices: Vec<PadVoice>,
    /// which residue class of voices gets its control update on the next step
    pad_rot: usize,
    saw_buf: [[f32; CTRL]; 4],
    scratch: Vec<[[f32; CTRL]; GROUP]>,
    // sub sines
    sub: [SinOsc; 2],
    sub_f: [f32; 2],
    sub_lfo: [SinOsc; 2],
    sub_amp: [f32; 2],
    // moog
    moog_noise: Noise0,
    moog_lag: VarLagSine,
    moog_cut: f32,
    moog: MoogStereo,
    // notes
    note_dust: Dust,
    note_imp0: Impulse,
    note_pick: TChoose,
    note_mul: f32,
    noterate: f32,
    notepulse: Impulse,
    seq_dust: Dust,
    seq_imp0: Impulse,
    seq_pick: Dxrand,
    seq_idx: usize,
    dseq: Dseq,
    seqnote: i8,
    /// a note pulse fired at the start of the current control step; consumed by the first segment
    note_trig: bool,
    // piano
    piano_noise_hz_n: Noise0,
    piano_noise_hz_lag: VarLagSine,
    noise_hz: f32,
    noise2: Noise2,
    decay2: Decay2,
    combs: [CombL; 2],
    piano_hz: f32,
    /// Control steps left until the piano string loop can be skipped.
    piano_live: u32,
    piano_lpf: RLpf,
    piano_hpf: Biquad,
    // PolyPerc
    pulse: Pulse,
    perc_moog: MoogLadder,
    perc_env: EnvPerc,
    perc_hz: f32,
    perc_cut: f32,
    env_prev: f32,
    env_cur: f32,
    // DynKlank: per channel 4 resonators on the current plane
    klank: [[Ringz; 4]; 2],
    klank_pink: [PinkNoise; 2],
    klank_notes: [i8; 4],
    klank_hz: f32,
    // final chain
    out_lpf: [Biquad; 2],
    out_hpf: [Biquad; 2],
    reverb: ReverbLite,
    intro_pos: u32,
    // layer mix
    mix_noise: Noise0,
    mix_lfo: SinOsc,
    piano_g: f32,
    perc_g: f32,
}

impl Eno {
    pub fn new(seed: u64, sr: f32) -> Eno {
        Eno::build(seed, sr, 330.0, 0.0)
    }

    fn build(seed: u64, sr: f32, hz: f32, amp: f32) -> Eno {
        let mut rng = Rng::new(seed);
        let dust = Dust::new(child_rng(&mut rng));
        let plane_pick = Dxrand::new(child_rng(&mut rng), PLANES.len());
        let f_sub = [rng.range(0.001, 0.01), rng.range(0.001, 0.01)];
        let mut voices = Vec::with_capacity(VOICES);
        for _ in 0..VOICES {
            voices.push(PadVoice::new(&mut rng, sr));
        }
        let moog_noise = Noise0::new(child_rng(&mut rng), sr);
        // note layer RNGs are drawn after the pad's, so the pad is unchanged
        let note_dust = Dust::new(child_rng(&mut rng));
        let note_pick = TChoose::new(child_rng(&mut rng));
        let note_mul = rng.range(0.78, 1.32);
        let seq_dust = Dust::new(child_rng(&mut rng));
        let seq_pick = Dxrand::new(child_rng(&mut rng), AIRPORTS.len());
        let piano_noise_hz_n = Noise0::new(child_rng(&mut rng), sr);
        let noise2 = Noise2::new(child_rng(&mut rng), sr);
        let mix_noise = Noise0::new(child_rng(&mut rng), sr);
        let klank_pink = [PinkNoise::new(child_rng(&mut rng)), PinkNoise::new(child_rng(&mut rng))];
        let mut s = Eno {
            seed,
            sr,
            dt: CTRL as f32 / sr,
            hz: 330.0,
            amp: 0.0,
            silent: false,
            sub_f_hz: [82.5, 165.0],
            hz_key: f32::NAN.to_bits(),
            pos: 0,
            max_hz: sr * 0.45,
            dust,
            impulse: Impulse::new(),
            plane_env: EnvPerc::new(3.0, 10.0, 0.9, -4.0),
            plane_delay: TDelay::new(3.0),
            plane_pick,
            plane_notes: [0; 4],
            plane_gain: 0.0,
            saws: [SawDpw::new(sr), SawDpw::new(sr), SawDpw::new(sr), SawDpw::new(sr)],
            voices,
            pad_rot: 0,
            saw_buf: [[0.0; CTRL]; 4],
            scratch: alloc::vec![[[0.0; CTRL]; GROUP]; GROUPS],
            sub: [SinOsc::new(sr, 0.0), SinOsc::new(sr, 0.0)],
            sub_f: f_sub,
            sub_lfo: [SinOsc::new(sr, 0.0), SinOsc::new(sr, 0.0)],
            sub_amp: [0.1, 0.025],
            moog_noise,
            moog_lag: VarLagSine::new(sr),
            moog_cut: 660.0,
            moog: MoogStereo::with_oversample(sr, 1),
            note_dust,
            note_imp0: Impulse::new(),
            note_pick,
            note_mul,
            noterate: 0.5,
            notepulse: Impulse::new(),
            seq_dust,
            seq_imp0: Impulse::new(),
            seq_pick,
            seq_idx: 0,
            dseq: Dseq::new(),
            seqnote: 0,
            note_trig: false,
            piano_noise_hz_n,
            piano_noise_hz_lag: VarLagSine::new(sr),
            noise_hz: 3500.0,
            noise2,
            decay2: Decay2::new(sr, 0.01, 1.0),
            combs: [CombL::new(sr, PIANO_MAX_DELAY_S), CombL::new(sr, PIANO_MAX_DELAY_S)],
            piano_hz: f32::NAN,
            piano_live: 0,
            piano_lpf: RLpf::new(sr),
            piano_hpf: Biquad::highpass(sr, 40.0, Q_BUTTERWORTH),
            pulse: Pulse::new(sr),
            perc_moog: MoogLadder::with_oversample(sr, 1),
            perc_env: EnvPerc::new(0.01, 4.0, 1.0, -4.0),
            perc_hz: 330.0,
            perc_cut: 495.0,
            env_prev: 0.0,
            env_cur: 0.0,
            klank: [[Ringz::new(sr), Ringz::new(sr), Ringz::new(sr), Ringz::new(sr)],
                [Ringz::new(sr), Ringz::new(sr), Ringz::new(sr), Ringz::new(sr)]],
            klank_pink,
            klank_notes: [i8::MAX; 4],
            klank_hz: f32::NAN,
            out_lpf: [Biquad::lowpass(sr, 1000.0, Q_BUTTERWORTH), Biquad::lowpass(sr, 1000.0, Q_BUTTERWORTH)],
            out_hpf: [Biquad::highpass(sr, HPF_HZ, Q_BUTTERWORTH), Biquad::highpass(sr, HPF_HZ, Q_BUTTERWORTH)],
            reverb: ReverbLite::new(sr, REVERB_MIX, 2.0, 0.5),
            intro_pos: 0,
            mix_noise,
            mix_lfo: SinOsc::new(sr, 0.0),
            piano_g: 0.0,
            perc_g: 0.0,
        };
        s.set_params(hz, amp);
        prof::note_addr(0, s.voices[0].delay.buf_ptr());
        prof::note_addr(1, s.reverb.buf_ptr());
        prof::note_addr(2, s.scratch.as_ptr().cast());
        prof::note_addr(3, s.combs[0].buf_ptr());
        s
    }

    /// Current plane semitone offsets (diagnostics and tests).
    #[doc(hidden)]
    pub fn plane_notes(&self) -> [i8; 4] {
        self.plane_notes
    }

    fn control(&mut self) {
        let c = CTRL as f32;
        let dt = self.dt;
        let hz = self.hz;
        // plane trigger: Dust.kr(1/30) + Impulse.kr(0); both always advance
        let trig = self.dust.tick(1.0 / 30.0, dt) | self.impulse.tick(0.0, dt);
        if trig {
            self.plane_env.trigger();
        }
        self.plane_gain = 1.0 - self.plane_env.tick(dt);
        if self.plane_delay.tick(trig, dt) {
            self.plane_notes = PLANES[self.plane_pick.next()];
        }
        for (i, saw) in self.saws.iter_mut().enumerate() {
            let base = if i == 0 { -36.0 } else { -24.0 };
            let semis = self.plane_notes[i] as f32 + base;
            saw.set_hz((hz * exp2(semis * (1.0 / 12.0))).min(self.max_hz));
        }
        // Pad voices modulate slowly (LFO 0.03-0.1 Hz, noise 5-10 Hz): update VOICES / STRIDE of
        // them per step, round-robin, so each is refreshed every STRIDE steps instead of every one.
        let rot = self.pad_rot;
        self.pad_rot = (rot + 1) % PAD_CTRL_STRIDE;
        for (i, v) in self.voices.iter_mut().enumerate() {
            if i % PAD_CTRL_STRIDE == rot {
                v.control(hz, self.max_hz);
            }
        }
        for k in 0..2 {
            let s = self.sub_lfo[k].tick(self.sub_f[k] * c);
            let (lo, hi) = if k == 0 { (0.05, 0.15) } else { (0.001, 0.05) };
            self.sub_amp[k] = lo + (hi - lo) * (s + 1.0) * 0.5;
        }
        let n = self.moog_noise.tick(c * (1.0 / 6.0));
        let x = self.moog_lag.tick(n, 6.0 / c);
        self.moog_cut = (hz * 2.0 * exp2(LOG2_30 * (x + 1.0) * 0.5)).min(self.max_hz);
        if KLANK_ENABLED && (self.plane_notes != self.klank_notes || hz != self.klank_hz) {
            self.klank_notes = self.plane_notes;
            // a base-pitch jump leaves the old resonance state far off the new coefficients: clear it
            let reset = hz != self.klank_hz && !self.klank_hz.is_nan();
            self.klank_hz = hz;
            for ch in self.klank.iter_mut() {
                for (k, rz) in ch.iter_mut().enumerate() {
                    if reset {
                        *rz = Ringz::new(self.sr);
                    }
                    rz.set((hz * exp2(self.plane_notes[k] as f32 * (1.0 / 12.0))).min(self.max_hz), KLANK_DECAY);
                }
            }
        }
        self.note_control();
    }

    /// Note layer control step: note rate, sequence, pitches, layer mix.
    fn note_control(&mut self) {
        let c = CTRL as f32;
        let dt = self.dt;
        let hz = self.hz;
        // noterate: TChoose on Dust(1) + Impulse(0)
        if self.note_dust.tick(1.0, dt) | self.note_imp0.tick(0.0, dt) {
            self.noterate = self.note_pick.pick(&NOTE_RATES) * self.note_mul;
        }
        // current sequence: Dxrand on Dust(0.1) + Impulse(0)
        if self.seq_dust.tick(0.1, dt) | self.seq_imp0.tick(0.0, dt) {
            self.seq_idx = self.seq_pick.next();
            self.dseq.reset();
        }
        self.note_trig = self.notepulse.tick(self.noterate, dt);
        self.piano_live = self.piano_live.saturating_sub(1);
        if self.note_trig {
            self.piano_live = (PIANO_LIVE_S * self.sr / CTRL as f32) as u32;
            self.seqnote = self.dseq.next(AIRPORTS[self.seq_idx]);
            self.perc_env.trigger();
        }
        let sn = self.seqnote as f32;
        // piano
        let n = self.piano_noise_hz_n.tick(c * 0.1);
        let x = self.piano_noise_hz_lag.tick(n, 10.0 / c);
        self.noise_hz = 3500.0 + 1500.0 * x;
        let ph = (hz * exp2((sn - 12.0) * (1.0 / 12.0))).clamp(40.0, 2000.0);
        if ph != self.piano_hz {
            self.piano_hz = ph;
            let inv = recip(ph);
            self.combs[0].set(inv * recip(TUNE_UP), STRING_DECAY);
            self.combs[1].set(inv * recip(TUNE_DOWN), STRING_DECAY);
            self.piano_lpf.set(2.0 * ph, 4.0);
        }
        // PolyPerc
        self.perc_hz = (hz * exp2(sn * (1.0 / 12.0))).min(self.max_hz);
        self.perc_cut = (hz * 1.5).min(self.max_hz);
        self.env_prev = self.env_cur;
        self.env_cur = self.perc_env.tick(dt);
        // layer crossfade
        let rate = 0.055 + 0.045 * self.mix_noise.tick(c * 0.1);
        let w = 0.5 + 0.4 * self.mix_lfo.tick(rate * c);
        self.piano_g = 0.3 * (1.0 - w);
        self.perc_g = w;
    }
}

impl Voice for Eno {
    fn set_sample_rate(&mut self, sr: f32) {
        *self = Eno::build(self.seed, sr, self.hz, self.amp);
    }

    fn set_params(&mut self, hz: f32, amp: f32) {
        let amp_in = if amp.is_finite() { amp.clamp(0.0, 1.0) } else { 0.0 };
        let key = hz.to_bits();
        if key != self.hz_key {
            self.hz_key = key;
            let h = if hz.is_finite() { hz } else { 0.0 };
            self.hz = h.clamp(8.0, self.max_hz);
            // hz < 1 (or non-finite) means silence
            self.silent = h < 1.0;
            self.sub_f_hz = [self.hz * 0.25, self.hz * 0.5];
            let lp = (self.hz * 8.0).min(self.max_hz);
            for f in self.out_lpf.iter_mut() {
                f.set_lowpass(lp);
            }
        }
        self.amp = if self.silent { 0.0 } else { amp_in };
    }

    // Hot path: keep it in internal instruction RAM so it never waits on the flash/PSRAM cache.
    #[inline(never)]
    #[cfg_attr(target_arch = "xtensa", link_section = ".iram1.eno_process")]
    fn process(&mut self, out: &mut [[f32; 2]]) {
        prof::set_names(&ENO_NAMES);
        let amp = self.amp * OUT_TRIM;
        let mut pb = prof::Block::start();
        let mut i = 0;
        while i < out.len() {
            if self.pos == 0 {
                self.control();
                pb.mark(P_CONTROL);
            }
            let n = (CTRL - self.pos).min(out.len() - i).min(CTRL);
            let [h0, h1] = self.sub_f_hz;
            let [a0, a1] = self.sub_amp;
            let mut l = [0.0f32; CTRL];
            let mut r = [0.0f32; CTRL];
            for j in 0..n {
                let s = self.sub[0].tick(h0) * a0 + self.sub[1].tick(h1) * a1;
                l[j] = s;
                r[j] = s;
            }
            for j in 0..n {
                for q in 0..4 {
                    self.saw_buf[q][j] = self.saws[q].tick_fixed();
                }
            }
            for (vc, bc) in self.voices.as_chunks_mut::<GROUP>().0.iter_mut().zip(self.scratch.iter_mut()) {
                for j in 0..n {
                    for q in 0..GROUP {
                        bc[q][j] = vc[q].lpf.tick(self.saw_buf[q][j]);
                    }
                }
                for j in 0..n {
                    for q in 0..GROUP {
                        bc[q][j] = vc[q].delay.tick_fixed(bc[q][j]);
                    }
                }
            }
            // 0.9 saw amplitude, 1/7 voice gain, plane envelope
            let pg = 0.9 * (1.0 / 7.0) * self.plane_gain;
            for (vc, bc) in self.voices.as_chunks_mut::<GROUP>().0.iter().zip(self.scratch.iter()) {
                for q in 0..GROUP {
                    let [g0, g1] = vc[q].gain;
                    let (g0, g1) = (g0 * pg, g1 * pg);
                    for j in 0..n {
                        l[j] += bc[q][j] * g0;
                        r[j] += bc[q][j] * g1;
                    }
                }
            }
            pb.mark(P_PAD);
            let cut = self.moog_cut;
            for j in 0..n {
                // No outer saturator: the ladder saturates its own input.
                let y = self.moog.tick([l[j], r[j]], cut, 0.0);
                l[j] = y[0];
                r[j] = y[1];
            }
            pb.mark(P_MOOG);
            if KLANK_ENABLED && !self.silent {
                for j in 0..n {
                    let (mut kl, mut kr) = (0.0f32, 0.0f32);
                    let (xl, xr) = (self.klank_pink[0].tick() * KLANK_EXCITE, self.klank_pink[1].tick() * KLANK_EXCITE);
                    for k in 0..4 {
                        kl += self.klank[0][k].tick(xl);
                        kr += self.klank[1][k].tick(xr);
                    }
                    l[j] += kl * KLANK_GAIN;
                    r[j] += kr * KLANK_GAIN;
                }
            }
            pb.mark(P_KLANK);
            // piano: noise burst -> two tuned combs -> RLPF -> HPF
            let mut nm = [0.0f32; CTRL];
            let pg = amp * self.piano_g;
            let mut trig = 0.0f32;
            if self.pos == 0 && self.note_trig {
                trig = 1.0;
            }
            let nhz = self.noise_hz;
            for j in 0..if self.piano_live > 0 { n } else { 0 } {
                let e = self.decay2.tick(trig);
                trig = 0.0;
                let x = self.noise2.tick(nhz) * e;
                let s = self.combs[0].tick(x) + self.combs[1].tick(x);
                nm[j] = self.piano_hpf.tick(self.piano_lpf.tick(s)) * pg;
            }
            pb.mark(P_PIANO);
            // PolyPerc: pulse -> Moog -> control-rate envelope ramped over the step
            let (e0, e1) = (self.env_prev, self.env_cur);
            if e0 != 0.0 || e1 != 0.0 {
                let (hzp, cut, pw) = (self.perc_hz, self.perc_cut, self.perc_g);
                let inc = (e1 - e0) * (1.0 / CTRL as f32);
                let mut e = e0 + inc * (self.pos + 1) as f32;
                for v in nm[..n].iter_mut() {
                    let p = self.pulse.tick(hzp, 0.5);
                    *v += self.perc_moog.tick(p, cut, 2.0) * e * pw;
                    e += inc;
                }
            }
            pb.mark(P_PERC);
            // final chain: LPF + HPF, intro fade, reverb, amp
            let intro_end = ((INTRO_DELAY_S + INTRO_RAMP_S) * self.sr) as u32;
            let intro_off = INTRO_DELAY_S * self.sr;
            let intro_inv = 1.0 / (INTRO_RAMP_S * self.sr);
            // silent (hz/amp zero): feed the chain zeros so the 8 Hz-clamped layers never reach the reverb
            let (in_l, in_r) = if self.silent { (0.0, 0.0) } else { (1.0, 1.0) };
            let mut j = 0;
            while j < n && self.intro_pos < intro_end {
                let g = ((self.intro_pos as f32 - intro_off) * intro_inv).clamp(0.0, 1.0);
                self.intro_pos += 1;
                l[j] = self.out_hpf[0].tick(self.out_lpf[0].tick((l[j] + nm[j]) * in_l)) * g;
                r[j] = self.out_hpf[1].tick(self.out_lpf[1].tick((r[j] + nm[j]) * in_r)) * g;
                j += 1;
            }
            while j < n {
                l[j] = self.out_hpf[0].tick(self.out_lpf[0].tick((l[j] + nm[j]) * in_l));
                r[j] = self.out_hpf[1].tick(self.out_lpf[1].tick((r[j] + nm[j]) * in_r));
                j += 1;
            }
            pb.mark(P_FINAL);
            for (j, o) in out[i..i + n].iter_mut().enumerate() {
                if REVERB_SINGLE_CHANNEL {
                    let m = (l[j] + r[j]) * 0.5;
                    let dry = m * (1.0 - REVERB_MIX);
                    let wet = self.reverb.tick(m, m)[0] - dry;
                    *o = [(l[j] * (1.0 - REVERB_MIX) + wet) * amp, (r[j] * (1.0 - REVERB_MIX) + wet) * amp];
                } else {
                    let y = self.reverb.tick(l[j], r[j]);
                    *o = [y[0] * amp, y[1] * amp];
                }
            }
            pb.mark(P_REVERB);
            self.pos = (self.pos + n) % CTRL;
            i += n;
        }
        pb.finish();
    }
}
