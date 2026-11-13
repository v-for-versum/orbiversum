use alloc::vec::Vec;
use orbiversum_core::prof;
use orbiversum_core::{
    biquad::Biquad,
    comb::CombL,
    env::{Decay2, EnvPerc},
    lfo::{Noise0, VarLagSine},
    moog::{MoogLadder, MoogStereo},
    noise::{Noise2, PinkNoise},
    osc::{Pulse, SawDpw, SinOsc},
    resonant::{RLpf, Ringz},
    reverb::ReverbLite,
    rng::Rng,
    seq::{Dseq, Dust, Dxrand, Impulse, TChoose, TDelay},
    voice::Voice,
};

use crate::music::{AIRPORTS, PLANES};
use crate::pad::{child_rng, PadVoice};
use crate::tuning::{CTRL, GROUP, GROUPS, HPF_HZ, PIANO_MAX_DELAY_S, Q_BUTTERWORTH, REVERB_MIX, VOICES};

/// The Eno drone voice: an evolving pad of chord planes with a sparse piano/PolyPerc note layer.
///
/// Construct with [`Eno::new`], set the pitch and level through
/// `orbiversum_core::voice::Voice::set_params`, and render with `Voice::process`. All memory
/// is allocated at construction (and again in `set_sample_rate`); `process` never allocates.
/// The fields are internal state: random generators, oscillators, filters, envelopes, delay
/// lines and the reverb.
pub struct Eno {
    pub(crate) seed: u64,
    pub(crate) sr: f32,
    /// control step in seconds (CTRL / sr), computed once
    pub(crate) dt: f32,
    pub(crate) hz: f32,
    pub(crate) amp: f32,
    pub(crate) silent: bool,
    pub(crate) sub_f_hz: [f32; 2],
    /// raw `hz` input bits the derived values belong to
    pub(crate) hz_key: u32,
    pub(crate) pos: usize,
    pub(crate) max_hz: f32,
    // plane sequencing
    pub(crate) dust: Dust,
    pub(crate) impulse: Impulse,
    pub(crate) plane_env: EnvPerc,
    pub(crate) plane_delay: TDelay,
    pub(crate) plane_pick: Dxrand,
    pub(crate) plane_notes: [i8; 4],
    pub(crate) plane_gain: f32,
    // pad
    pub(crate) saws: [SawDpw; 4],
    pub(crate) voices: Vec<PadVoice>,
    /// which residue class of voices gets its control update on the next step
    pub(crate) pad_rot: usize,
    pub(crate) saw_buf: [[f32; CTRL]; 4],
    pub(crate) scratch: Vec<[[f32; CTRL]; GROUP]>,
    // sub sines
    pub(crate) sub: [SinOsc; 2],
    pub(crate) sub_f: [f32; 2],
    pub(crate) sub_lfo: [SinOsc; 2],
    pub(crate) sub_amp: [f32; 2],
    // moog
    pub(crate) moog_noise: Noise0,
    pub(crate) moog_lag: VarLagSine,
    pub(crate) moog_cut: f32,
    pub(crate) moog: MoogStereo,
    // notes
    pub(crate) note_dust: Dust,
    pub(crate) note_imp0: Impulse,
    pub(crate) note_pick: TChoose,
    pub(crate) note_mul: f32,
    pub(crate) noterate: f32,
    pub(crate) notepulse: Impulse,
    pub(crate) seq_dust: Dust,
    pub(crate) seq_imp0: Impulse,
    pub(crate) seq_pick: Dxrand,
    pub(crate) seq_idx: usize,
    pub(crate) dseq: Dseq,
    pub(crate) seqnote: i8,
    /// a note pulse fired at the start of the current control step; consumed by the first segment
    pub(crate) note_trig: bool,
    // piano
    pub(crate) piano_noise_hz_n: Noise0,
    pub(crate) piano_noise_hz_lag: VarLagSine,
    pub(crate) noise_hz: f32,
    pub(crate) noise2: Noise2,
    pub(crate) decay2: Decay2,
    pub(crate) combs: [CombL; 2],
    pub(crate) piano_hz: f32,
    /// Control steps left until the piano string loop can be skipped.
    pub(crate) piano_live: u32,
    pub(crate) piano_lpf: RLpf,
    pub(crate) piano_hpf: Biquad,
    // PolyPerc
    pub(crate) pulse: Pulse,
    pub(crate) perc_moog: MoogLadder,
    pub(crate) perc_env: EnvPerc,
    pub(crate) perc_hz: f32,
    pub(crate) perc_cut: f32,
    pub(crate) env_prev: f32,
    pub(crate) env_cur: f32,
    // DynKlank: per channel 4 resonators on the current plane
    pub(crate) klank: [[Ringz; 4]; 2],
    pub(crate) klank_pink: [PinkNoise; 2],
    pub(crate) klank_notes: [i8; 4],
    pub(crate) klank_hz: f32,
    // final chain
    pub(crate) out_lpf: [Biquad; 2],
    pub(crate) out_hpf: [Biquad; 2],
    pub(crate) reverb: ReverbLite,
    pub(crate) intro_pos: u32,
    // layer mix
    pub(crate) mix_noise: Noise0,
    pub(crate) mix_lfo: SinOsc,
    pub(crate) piano_g: f32,
    pub(crate) perc_g: f32,
}

impl Eno {
    /// Creates the voice for sample rate `sr` (Hz) with the given random `seed`.
    ///
    /// The seed fixes all random choices (LFO rates and phases, plane order, note rates and
    /// sequences), so equal seeds render equal audio. The initial parameters are `hz` = 330 and
    /// `amp` = 0 (silent until `set_params` is called).
    pub fn new(seed: u64, sr: f32) -> Eno {
        Eno::build(seed, sr, 330.0, 0.0)
    }

    /// Builds the voice with explicit `hz` and `amp` (also used to re-create it at a new sample rate).
    pub(crate) fn build(seed: u64, sr: f32, hz: f32, amp: f32) -> Eno {
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
}
