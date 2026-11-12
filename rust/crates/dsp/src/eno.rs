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
    comb::DelayL,
    env::EnvPerc,
    fast::{exp2, tanh},
    lfo::{Noise0, Noise1, VarLagSine},
    moog::MoogStereo,
    osc::{SawDpw, SinOsc},
    pan::pan2,
    rng::Rng,
    seq::{Dust, Dxrand, Impulse, TDelay},
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
const VOICES: usize = 8;
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

    fn control(&mut self, hz: f32, max_hz: f32) {
        let c = CTRL as f32;
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
        };
        s.set_params(hz, amp);
        prof::note_addr(0, s.voices[0].delay.buf_ptr());
        prof::note_addr(1, s.voices[VOICES - 1].delay.buf_ptr());
        prof::note_addr(2, s.scratch.as_ptr().cast());
        prof::note_addr(3, s.voices.as_ptr().cast());
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
        for v in self.voices.iter_mut() {
            v.control(hz, self.max_hz);
        }
        for k in 0..2 {
            let s = self.sub_lfo[k].tick(self.sub_f[k] * c);
            let (lo, hi) = if k == 0 { (0.05, 0.15) } else { (0.001, 0.05) };
            self.sub_amp[k] = lo + (hi - lo) * (s + 1.0) * 0.5;
        }
        let n = self.moog_noise.tick(c * (1.0 / 6.0));
        let x = self.moog_lag.tick(n, 6.0 / c);
        self.moog_cut = (hz * 2.0 * exp2(LOG2_30 * (x + 1.0) * 0.5)).min(self.max_hz);
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
        }
        self.amp = if self.silent { 0.0 } else { amp_in };
    }

    // Hot path: keep it in internal instruction RAM so it never waits on the flash/PSRAM cache.
    #[inline(never)]
    #[cfg_attr(target_arch = "xtensa", link_section = ".iram1.eno_process")]
    fn process(&mut self, out: &mut [[f32; 2]]) {
        prof::set_names(&ENO_NAMES);
        let amp = self.amp;
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
                let y = self.moog.tick([tanh(l[j]), tanh(r[j])], cut, 0.0);
                l[j] = y[0];
                r[j] = y[1];
            }
            pb.mark(P_MOOG);
            for (j, o) in out[i..i + n].iter_mut().enumerate() {
                *o = [l[j] * amp, r[j] * amp];
            }
            pb.mark(P_FINAL);
            self.pos = (self.pos + n) % CTRL;
            i += n;
        }
        pb.finish();
    }
}
