//! Starlids: port of dronecaster's Starlids.scd.
//!
//! Control-rate parts (slow LFOs, filter cutoffs, pan) update every
//! `CTRL` samples at fixed absolute positions, so the output does not
//! depend on the caller's block size.

use crate::voice::Voice;
use alloc::vec::Vec;
use blocks::{
    biquad::Biquad,
    delay::DelayC,
    lfo::{Noise0, Noise1, VarLagSine},
    moog::MoogLadder,
    osc::{LfTri, Pulse, SawDpw, SinOsc},
    pan::pan2,
    rng::Rng,
};
use libm::{exp2f, powf, tanhf};

const CTRL: usize = 32;
const LANES: usize = 12;
const Q_BUTTERWORTH: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// Largest delay time reached: (0.02 + 0.01) / 15 s = 2 ms; margin added.
const MAX_DELAY_S: f32 = 0.004;

struct Lane {
    tri1: LfTri,
    f_tri1: f32,
    tri2: LfTri,
    f_tri2: f32,
    saw: SawDpw,
    saw_hz: f32,
    lpf_lfo: SinOsc,
    f_lpf: f32,
    lpf: Biquad,
    delay: DelayC,
    dly_noise: Noise1,
    f_dly: f32,
    delay_s: f32,
    pan_noise: Noise0,
    pan_lag: VarLagSine,
    gain: [f32; 2],
}

fn child_rng(rng: &mut Rng) -> Rng {
    let hi = (rng.f32() * 16_777_216.0) as u64;
    let lo = (rng.f32() * 16_777_216.0) as u64;
    Rng::new((hi << 24) | lo)
}

impl Lane {
    fn new(rng: &mut Rng, sr: f32) -> Lane {
        // Draw order mirrors the SC graph; each value is drawn exactly once.
        let f_tri1 = rng.range(1.0 / 100.0, 1.0 / 30.0);
        let ph1 = rng.f32();
        let f_tri2 = rng.range(1.0 / 100.0, 1.0 / 30.0);
        let ph2 = rng.f32();
        let f_lpf = rng.range(1.0 / 30.0, 1.0 / 10.0);
        let ph_lpf = rng.f32();
        let f_dly = rng.range(5.0, 10.0);
        Lane {
            tri1: LfTri::new(sr, ph1),
            f_tri1,
            tri2: LfTri::new(sr, ph2),
            f_tri2,
            saw: SawDpw::new(sr),
            saw_hz: 100.0,
            lpf_lfo: SinOsc::new(sr, ph_lpf),
            f_lpf,
            lpf: Biquad::lowpass(sr, 1000.0, Q_BUTTERWORTH),
            delay: DelayC::new(sr, MAX_DELAY_S),
            dly_noise: Noise1::new(child_rng(rng), sr),
            f_dly,
            delay_s: 0.02 / 15.0,
            pan_noise: Noise0::new(child_rng(rng), sr),
            pan_lag: VarLagSine::new(sr),
            gain: [core::f32::consts::FRAC_1_SQRT_2; 2],
        }
    }

    fn control(&mut self, hz: f32, max_hz: f32) {
        let c = CTRL as f32;
        let o1 = libm::floorf(self.tri1.tick(self.f_tri1 * c) + 1.0);
        let o2 = libm::floorf(self.tri2.tick(self.f_tri2 * c) + 1.0);
        self.saw_hz = (hz * exp2f((4.0 * o1 + 5.0 * o2) / 12.0)).min(max_hz);
        let s = self.lpf_lfo.tick(self.f_lpf * c);
        self.lpf.set_lowpass(20.0 * powf(600.0, (s + 1.0) * 0.5));
        self.delay_s = (0.02 + 0.01 * self.dly_noise.tick(self.f_dly * c)) / 15.0;
        let p = self.pan_noise.tick(PAN_RATE * c);
        let p = self.pan_lag.tick(p, 3.0 / c);
        self.gain = pan2(1.0, p);
    }
}

/// LFNoise0.kr(1/3)
const PAN_RATE: f32 = 1.0 / 3.0;

pub struct Starlids {
    seed: u64,
    sr: f32,
    hz: f32,
    amp: f32,
    pos: usize,
    max_hz: f32,
    sub: Pulse,
    sub_tri: LfTri,
    sub_width: f32,
    lanes: Vec<Lane>,
    moog_lag_noise: Noise0,
    moog_lag: VarLagSine,
    moog_cut: f32,
    moog: [MoogLadder; 2],
    hpf: [Biquad; 2],
}

impl Starlids {
    pub fn new(seed: u64, sr: f32) -> Starlids {
        Starlids::build(seed, sr, 110.0, 0.0)
    }

    fn build(seed: u64, sr: f32, hz: f32, amp: f32) -> Starlids {
        let mut rng = Rng::new(seed);
        let mut lanes = Vec::with_capacity(LANES);
        for _ in 0..LANES {
            lanes.push(Lane::new(&mut rng, sr));
        }
        let mut s = Starlids {
            seed,
            sr,
            hz: 110.0,
            amp: 0.0,
            pos: 0,
            max_hz: sr * 0.45,
            sub: Pulse::new(sr),
            sub_tri: LfTri::new(sr, 0.0),
            sub_width: 0.5,
            lanes,
            moog_lag_noise: Noise0::new(child_rng(&mut rng), sr),
            moog_lag: VarLagSine::new(sr),
            moog_cut: 880.0,
            moog: [MoogLadder::new(sr), MoogLadder::new(sr)],
            hpf: [
                Biquad::highpass(sr, 20.0, Q_BUTTERWORTH),
                Biquad::highpass(sr, 20.0, Q_BUTTERWORTH),
            ],
        };
        s.set_params(hz, amp);
        s
    }

    fn control(&mut self) {
        let c = CTRL as f32;
        let t = self.sub_tri.tick(0.5 * c);
        self.sub_width = 0.5 + 0.3 * t;
        for l in self.lanes.iter_mut() {
            l.control(self.hz, self.max_hz);
        }
        let n = self.moog_lag_noise.tick(c * (1.0 / 6.0));
        let x = self.moog_lag.tick(n, 6.0 / c);
        self.moog_cut = self.hz * 8.0 * powf(7.5, (x + 1.0) * 0.5);
    }
}

impl Voice for Starlids {
    fn set_sample_rate(&mut self, sr: f32) {
        *self = Starlids::build(self.seed, sr, self.hz, self.amp);
    }

    fn set_params(&mut self, hz: f32, amp: f32) {
        let hz = if hz.is_finite() { hz } else { 0.0 };
        self.hz = hz.clamp(8.0, self.max_hz);
        // hz == 0 means silence
        let silent = hz < 1.0;
        let amp = if amp.is_finite() { amp.clamp(0.0, 1.0) } else { 0.0 };
        self.amp = if silent { 0.0 } else { amp };
    }

    fn process(&mut self, out: &mut [[f32; 2]]) {
        let gain = self.amp / 12.0;
        for frame in out.iter_mut() {
            if self.pos == 0 {
                self.control();
            }
            self.pos = (self.pos + 1) % CTRL;

            let sub = self.sub.tick(self.hz * 0.5, self.sub_width) * gain * core::f32::consts::FRAC_1_SQRT_2;
            let (mut l, mut r) = (sub, sub);
            for ln in self.lanes.iter_mut() {
                let x = ln.saw.tick(ln.saw_hz);
                let x = ln.lpf.tick(x);
                let x = ln.delay.tick(x, ln.delay_s) * gain;
                l += x * ln.gain[0];
                r += x * ln.gain[1];
            }
            let cut = self.moog_cut;
            let l = self.hpf[0].tick(self.moog[0].tick(tanhf(l), cut, 0.0));
            let r = self.hpf[1].tick(self.moog[1].tick(tanhf(r), cut, 0.0));
            *frame = [l, r];
        }
    }
}
