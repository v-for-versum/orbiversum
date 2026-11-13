//! The control step: everything that changes slower than the audio rate.
//!
//! Runs once per `CTRL` samples (at fixed absolute sample positions). Generators advance by
//! `CTRL` samples per call, so their rates are in Hz of real time.

use orbiversum_core::{fast::{exp2, recip}, resonant::Ringz};

use crate::engine::Eno;
use crate::music::{AIRPORTS, NOTE_RATES, PLANES, STRING_DECAY, TUNE_DOWN, TUNE_UP};
use crate::tuning::{CTRL, KLANK_DECAY, KLANK_ENABLED, LOG2_30, PAD_CTRL_STRIDE, PIANO_LIVE_S};

impl Eno {
    /// One control step: plane trigger and pick, saw pitches, round-robin pad voice updates,
    /// sub-sine amplitudes, Moog cutoff, Klank retuning, then the note layer.
    pub(crate) fn control(&mut self) {
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
            self.plane_notes = PLANES[self.plane_pick.pick()];
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
            self.seq_idx = self.seq_pick.pick();
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
