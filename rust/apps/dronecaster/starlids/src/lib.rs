//! Starlids: port of dronecaster's `Starlids.scd` for a fixed-size, allocation-free real-time voice.
//!
//! # Algorithm
//!
//! Twelve independent *lanes* (`LANES`) of detuned saws are summed into a stereo field:
//!
//! * Each lane has a band-limited saw (`SawDpw`) whose pitch is `hz * 2^((4 * o1 + 5 * o2) / 12)`.
//!   `o1` and `o2` are integer steps (`floor(tri + 1)`) of two very slow triangle LFOs
//!   (periods 30-100 s), so the pitch drifts in steps of 4 and 5 semitones above `hz`.
//! * The saw goes through a lowpass (Butterworth) whose cutoff sweeps exponentially between
//!   20 Hz and 12 kHz (`20 * 600^((lfo + 1) / 2)`, LFO period 10-30 s), then through a short
//!   interpolated delay (about 0.7-2 ms, time wandering with smooth noise) that adds a slow
//!   phasing, then is panned by a smoothed random position (a new target about every 3 s).
//! * A sub pulse at `hz / 2` with a slowly changing pulse width (0.2-0.8) is added to both
//!   channels.
//! * The sum goes through a stereo Moog ladder whose cutoff wanders between `8 * hz` and
//!   `60 * hz` (`hz * 8 * 7.5^((x + 1) / 2)`), then a 20 Hz highpass per channel.
//! * Output gain is `amp / 12` per lane (the sub is scaled by 1/sqrt(2) on top).
//!
//! # State and parameters
//!
//! The state (oscillators, filters, delay lines, random generators) is allocated in
//! [`Starlids::new`]; `process` never allocates. All random values (LFO rates, phases) are drawn
//! once from the seed, so equal seeds render equal audio.
//!
//! * `hz` (Hz): base pitch, clamped to [8, 0.45 * sample rate]. Below 1 Hz, or if not finite,
//!   the voice is silent.
//! * `amp`: linear level 0..1 (clamped; non-finite counts as 0).
//!
//! Control-rate parts (slow LFOs, filter cutoffs, pan) update every `CTRL` = 32 samples at fixed
//! absolute positions, so the output does not depend on the caller's block size. The lanes are
//! processed in groups of four (`GROUP`) so the serial FPU chains of one lane overlap with
//! independent lanes on the in-order core.
//!
//! # Tests
//!
//! `tests/starlids_ref.rs` compares the output (seeds 5 and 9, 110 Hz, amp 0.8, 4 s at 48 kHz)
//! with a golden captured from the pre-optimisation implementation (libm `tanh`, 2x oversampled
//! Moog): the level of 12 probe frequencies (direct DFT correlation, Hann windows) within 1 dB up to 6 kHz,
//! within 10 dB above (the ladder's stopband), the summed power above 6 kHz within 3 dB, and the
//! RMS within 0.5 dB. It checks that the output stays within these tolerances of the golden
//! (up to ~9 dB difference at 18 kHz in the stopband is accepted, from the real-time changes: no
//! outer `tanh`, single-pass Moog); it is a spectral tolerance check, not a bit-exact one and
//! not a listening verdict.

#![warn(missing_docs)]
#![no_std]
extern crate alloc;

use orbiversum_core::prof;
use orbiversum_core::voice::Voice;
use alloc::vec::Vec;
use orbiversum_core::{
    biquad::Biquad,
    delay::DelayC,
    lfo::{Noise0, Noise1, VarLagSine},
    moog::MoogStereo,
    osc::{LfTri, Pulse, SawDpw, SinOsc},
    pan::pan2,
    rng::Rng,
};
use orbiversum_core::fast::exp2 as exp2f;

/// 2^x-based replacement for libm::powf with a constant base (f32 only).
fn pow_k(log2_base: f32, e: f32) -> f32 {
    exp2f(log2_base * e)
}
/// log2(600): the lane lowpass cutoff is `20 * 2^(LOG2_600 * (lfo + 1) / 2)`, i.e. 20 Hz to 12 kHz.
const LOG2_600: f32 = 9.228_819;
/// log2(7.5): the Moog cutoff is `hz * 8 * 2^(LOG2_7_5 * (x + 1) / 2)`, i.e. `8 * hz` to `60 * hz`.
const LOG2_7_5: f32 = 2.906_890_6;

/// Control period in samples: modulation updates once per 32 frames.
const CTRL: usize = 32;
/// Number of detuned saw lanes; the cost of the voice scales linearly with it.
const LANES: usize = 12;
/// Lanes processed together in the hot loops (must divide LANES).
const GROUP: usize = 4;
const _: () = assert!(LANES.is_multiple_of(GROUP), "lane loops process whole groups");
const _: () = assert!(CTRL.is_multiple_of(2) && CTRL > 0, "control segments are CTRL samples");
/// Biquad Q of 1/sqrt(2): flat (Butterworth) response.
const Q_BUTTERWORTH: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// Largest delay time reached: (0.02 + 0.01) / 15 s = 2 ms; margin added.
const MAX_DELAY_S: f32 = 0.004;

/// One saw lane: two step LFOs for the pitch, the saw, a lowpass with its cutoff LFO, a delay
/// with wandering time and a smoothed random pan. Lanes share no state.
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
    /// (o1, o2) the cached `ratio` belongs to (NaN = none)
    oct: (f32, f32),
    ratio: f32,
    /// pan position the cached `gain` belongs to (NaN = none)
    pan_c: f32,
}

/// Derives an independent generator from `rng` (two draws, so the parent advances by a fixed amount).
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
            oct: (f32::NAN, f32::NAN),
            ratio: 1.0,
            pan_c: f32::NAN,
        }
    }

    /// Control step of one lane: pitch steps, cutoff, delay time and pan gains.
    fn control(&mut self, hz: f32, max_hz: f32) {
        let c = CTRL as f32;
        let o1 = orbiversum_core::fast::floor(self.tri1.tick(self.f_tri1 * c) + 1.0);
        let o2 = orbiversum_core::fast::floor(self.tri2.tick(self.f_tri2 * c) + 1.0);
        // The octave/fifth steps change rarely: recompute the ratio only when they do.
        if o1 != self.oct.0 || o2 != self.oct.1 {
            self.oct = (o1, o2);
            self.ratio = exp2f((4.0 * o1 + 5.0 * o2) * (1.0 / 12.0));
        }
        self.saw_hz = (hz * self.ratio).min(max_hz);
        let s = self.lpf_lfo.tick(self.f_lpf * c);
        self.lpf.set_lowpass(20.0 * pow_k(LOG2_600, (s + 1.0) * 0.5));
        self.delay_s = (0.02 + 0.01 * self.dly_noise.tick(self.f_dly * c)) * (1.0 / 15.0);
        self.delay.set_delay(self.delay_s);
        self.saw.set_hz(self.saw_hz);
        let p = self.pan_noise.tick(PAN_RATE * c);
        let p = self.pan_lag.tick(p, 3.0 / c);
        // pan is constant between the noise steps: recompute the gains only on change
        if p != self.pan_c {
            self.pan_c = p;
            self.gain = pan2(1.0, p);
        }
    }
}

/// LFNoise0.kr(1/3)
const PAN_RATE: f32 = 1.0 / 3.0;

/// The Starlids drone voice (twelve detuned saw lanes plus a sub pulse, see the crate docs).
///
/// Create with [`Starlids::new`], set `hz`/`amp` with `Voice::set_params` and render with
/// `Voice::process`. The fields are internal state.
pub struct Starlids {
    seed: u64,
    hz: f32,
    amp: f32,
    pos: usize,
    max_hz: f32,
    sub: Pulse,
    sub_tri: LfTri,
    sub_width: f32,
    lanes: Vec<Lane>,
    /// per-lane scratch for one control segment
    scratch: Vec<[[f32; CTRL]; GROUP]>,
    moog_lag_noise: Noise0,
    moog_lag: VarLagSine,
    moog_cut: f32,
    moog: MoogStereo,
    hpf: [Biquad; 2],
}

impl Starlids {
    /// Creates the voice for sample rate `sr` (Hz) with the given random `seed`.
    ///
    /// Initial parameters: `hz` = 110, `amp` = 0 (silent until `set_params` is called).
    pub fn new(seed: u64, sr: f32) -> Starlids {
        Starlids::build(seed, sr, 110.0, 0.0)
    }

    /// Builds the voice with explicit `hz` and `amp` (also used to re-create it at a new sample rate).
    fn build(seed: u64, sr: f32, hz: f32, amp: f32) -> Starlids {
        let mut rng = Rng::new(seed);
        let mut lanes = Vec::with_capacity(LANES);
        for _ in 0..LANES {
            lanes.push(Lane::new(&mut rng, sr));
        }
        let mut s = Starlids {
            seed,
            hz: 110.0,
            amp: 0.0,
            pos: 0,
            max_hz: sr * 0.45,
            sub: Pulse::new(sr),
            sub_tri: LfTri::new(sr, 0.0),
            sub_width: 0.5,
            lanes,
            scratch: alloc::vec![[[0.0; CTRL]; GROUP]; LANES / GROUP],
            moog_lag_noise: Noise0::new(child_rng(&mut rng), sr),
            moog_lag: VarLagSine::new(sr),
            moog_cut: 880.0,
            moog: MoogStereo::with_oversample(sr, 1),
            hpf: [
                Biquad::highpass(sr, 20.0, Q_BUTTERWORTH),
                Biquad::highpass(sr, 20.0, Q_BUTTERWORTH),
            ],
        };
        s.set_params(hz, amp);
        prof::note_addr(0, s.lanes[0].delay.buf_ptr());
        prof::note_addr(1, s.lanes[LANES - 1].delay.buf_ptr());
        prof::note_addr(2, s.scratch.as_ptr().cast());
        prof::note_addr(3, s.lanes.as_ptr().cast());
        s
    }

    /// Control step of the whole voice: sub pulse width, every lane, Moog cutoff.
    fn control(&mut self) {
        let c = CTRL as f32;
        let t = self.sub_tri.tick(0.5 * c);
        self.sub_width = 0.5 + 0.3 * t;
        for l in self.lanes.iter_mut() {
            l.control(self.hz, self.max_hz);
        }
        let n = self.moog_lag_noise.tick(c * (1.0 / 6.0));
        let x = self.moog_lag.tick(n, 6.0 / c);
        self.moog_cut = self.hz * 8.0 * pow_k(LOG2_7_5, (x + 1.0) * 0.5);
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

    // Hot path: keep it in internal instruction RAM so it never waits on the flash/PSRAM cache.
    #[inline(never)]
    #[cfg_attr(target_arch = "xtensa", link_section = ".iram1.starlids_process")]
    #[allow(clippy::needless_range_loop, reason = "indexed lane loops keep the proven codegen of the realtime path")]
    fn process(&mut self, out: &mut [[f32; 2]]) {
        let gain = self.amp * (1.0 / 12.0);
        let sub_gain = gain * core::f32::consts::FRAC_1_SQRT_2;
        prof::set_names(&prof::NAMES);
        let mut pb = prof::Block::start();
        let mut i = 0;
        while i < out.len() {
            if self.pos == 0 {
                self.control();
                pb.mark(prof::CONTROL);
            }
            // Segment up to the next control tick: every section runs over the
            // whole segment (per-lane state is independent, mix order is fixed),
            // so the output does not depend on how the caller chops blocks.
            // `.min(CTRL)` lets the compiler drop the bounds checks on the CTRL-sized arrays below.
            let n = (CTRL - self.pos).min(out.len() - i).min(CTRL);
            let mut l = [0.0f32; CTRL];
            let mut r = [0.0f32; CTRL];
            for j in 0..n {
                let sub = self.sub.tick(self.hz * 0.5, self.sub_width) * sub_gain;
                l[j] = sub;
                r[j] = sub;
            }
            pb.mark(prof::OTHER);
            // Lanes in groups of GROUP: the FPU chains of one lane are serial (and the
            // core in-order), so interleaving independent lanes hides the latency.
            for (lc, bc) in self.lanes.as_chunks_mut::<GROUP>().0.iter_mut().zip(self.scratch.iter_mut()) {
                for j in 0..n {
                    for q in 0..GROUP {
                        bc[q][j] = lc[q].saw.tick_fixed();
                    }
                }
            }
            pb.mark(prof::SAW);
            for (lc, bc) in self.lanes.as_chunks_mut::<GROUP>().0.iter_mut().zip(self.scratch.iter_mut()) {
                for j in 0..n {
                    for q in 0..GROUP {
                        bc[q][j] = lc[q].lpf.tick(bc[q][j]);
                    }
                }
            }
            pb.mark(prof::LPF);
            for (lc, bc) in self.lanes.as_chunks_mut::<GROUP>().0.iter_mut().zip(self.scratch.iter_mut()) {
                for j in 0..n {
                    for q in 0..GROUP {
                        bc[q][j] = lc[q].delay.tick_fixed(bc[q][j]) * gain;
                    }
                }
            }
            pb.mark(prof::DELAY);
            for (lc, bc) in self.lanes.as_chunks_mut::<GROUP>().0.iter().zip(self.scratch.iter()) {
                for q in 0..GROUP {
                    let [g0, g1] = lc[q].gain;
                    for j in 0..n {
                        l[j] += bc[q][j] * g0;
                        r[j] += bc[q][j] * g1;
                    }
                }
            }
            pb.mark(prof::MIX);
            let cut = self.moog_cut;
            for j in 0..n {
                let y = self.moog.tick([l[j], r[j]], cut, 0.0);
                l[j] = y[0];
                r[j] = y[1];
            }
            pb.mark(prof::MOOG);
            for (j, o) in out[i..i + n].iter_mut().enumerate() {
                *o = [self.hpf[0].tick(l[j]), self.hpf[1].tick(r[j])];
            }
            pb.mark(prof::HPF);
            self.pos = (self.pos + n) % CTRL;
            i += n;
        }
        pb.finish();
    }
}

/// Address of `Starlids::process` (diagnostics: IRAM 0x4037xxxx vs cache-mapped 0x42xxxxxx).
pub fn process_addr() -> usize {
    <Starlids as Voice>::process as fn(&mut Starlids, &mut [[f32; 2]]) as usize
}
