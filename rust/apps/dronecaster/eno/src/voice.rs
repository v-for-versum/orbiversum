use orbiversum_core::prof;
use orbiversum_core::voice::Voice;

use crate::engine::Eno;
use crate::profile::{ENO_NAMES, P_CONTROL, P_FINAL, P_KLANK, P_MOOG, P_PAD, P_PERC, P_PIANO, P_REVERB};
use crate::tuning::{CTRL, GROUP, INTRO_DELAY_S, INTRO_RAMP_S, KLANK_ENABLED, KLANK_EXCITE, KLANK_GAIN, OUT_TRIM, REVERB_MIX, REVERB_SINGLE_CHANNEL};

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
    #[allow(clippy::needless_range_loop, reason = "indexed lane loops keep the proven codegen of the realtime path")]
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
