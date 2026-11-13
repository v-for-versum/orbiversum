use orbiversum_core::{
    biquad::Biquad,
    comb::DelayL,
    fast::exp2,
    lfo::{Noise0, Noise1, VarLagSine},
    osc::SinOsc,
    pan::pan2,
    rng::Rng,
};

use crate::tuning::{CTRL, LOG2_5, MAX_DELAY_S, PAD_CTRL_STRIDE, PAN_RATE, Q_BUTTERWORTH};

pub(crate) fn child_rng(rng: &mut Rng) -> Rng {
    let hi = (rng.f32() * 16_777_216.0) as u64;
    let lo = (rng.f32() * 16_777_216.0) as u64;
    Rng::new((hi << 24) | lo)
}

pub(crate) struct PadVoice {
    pub(crate) lfo: SinOsc,
    pub(crate) f_lfo: f32,
    pub(crate) lpf: Biquad,
    pub(crate) delay: DelayL,
    pub(crate) dly_noise: Noise1,
    pub(crate) f_dly: f32,
    pub(crate) pan_noise: Noise0,
    pub(crate) pan_lag: VarLagSine,
    pub(crate) gain: [f32; 2],
    /// pan position the cached `gain` belongs to (NaN = none)
    pub(crate) pan_c: f32,
}

impl PadVoice {
    pub(crate) fn new(rng: &mut Rng, sr: f32) -> PadVoice {
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
    pub(crate) fn control(&mut self, hz: f32, max_hz: f32) {
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
