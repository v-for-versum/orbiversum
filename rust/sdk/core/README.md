# orbiversum-core

`#![no_std]` (plus `alloc`) DSP building blocks for the Orbiversum drone voices: oscillators,
filters, delays, a reverb, envelopes, noise and control-rate sequencing helpers. Shared by the
ESP32-S3 firmware and the host tests. The `Voice` trait is the interface every drone implements.

Conventions: sample rates and frequencies in Hz, times in seconds, gains linear. Blocks allocate
only in their constructors (the ones that own a delay buffer); `tick` never allocates. Full
per-item documentation: `cargo doc --no-deps -p orbiversum-core --open`.

## Block catalogue

Cost: **low** = a few multiply-adds per call; **medium** = tens of operations or several buffer
accesses; **high** = many dependent operations with saturators.

| Module | Blocks | Rate | Cost |
|---|---|---|---|
| `fast` | `recip`, `tanh`, `tanh_cheap`, `sin`, `cos`, `sin_cos_0_pi`, `exp`, `exp2`, `floor` (f32 approximations) | audio, stateless | low |
| `osc` | `SawDpw` (band-limited saw), `Pulse`, `SinOsc`, `LfTri` | audio | low |
| `biquad` | `Biquad` (RBJ lowpass/highpass) | audio | low |
| `resonant` | `RLpf`, `Ringz` (SC-style resonant filters) | audio (`set` at control rate) | low |
| `moog` | `MoogLadder`, `MoogStereo` (Huovilainen ladder) | audio | high |
| `comb` | `DelayL` (linear delay), `CombL` (feedback comb) | audio | low |
| `delay` | `DelayC` (cubic delay) | audio | low |
| `reverb` | `ReverbLite` (Freeverb: 4 combs + 2 allpasses per channel) | audio | medium |
| `env` | `Decay2` (audio), `EnvPerc` (control, SC `Env.perc`) | audio / control | low |
| `lfo` | `Noise0`, `Noise1`, `VarLagSine` | slow modulators | low |
| `noise` | `Noise2` (LFNoise2), `PinkNoise` (Kellet) | audio | low |
| `rng` | `Rng` (xorshift64*, deterministic) | any | low |
| `pan` | `pan2` (equal power) | audio | low |
| `seq` | `Dust`, `Impulse`, `Dseq`, `Dxrand`, `TChoose`, `TDelay` | control | low |
| `prof` | per-section cycle timing, optional | per block | low |
| `voice` | `Voice` trait | per block | n/a |

## Building a `Voice`

```rust
use orbiversum_core::{osc::SinOsc, pan::pan2, voice::Voice};

struct Sine {
    osc: SinOsc,
    hz: f32,
    amp: f32,
}

impl Voice for Sine {
    fn set_sample_rate(&mut self, sr: f32) {
        self.osc = SinOsc::new(sr, 0.0); // reconfiguration, not real time (SinOsc itself does not allocate)
    }
    fn set_params(&mut self, hz: f32, amp: f32) {
        self.hz = hz; // real time: no allocation
        self.amp = amp;
    }
    fn process(&mut self, out: &mut [[f32; 2]]) {
        for frame in out.iter_mut() {
            *frame = pan2(self.osc.tick(self.hz) * self.amp, 0.0);
        }
    }
}

let mut v = Sine { osc: SinOsc::new(48_000.0, 0.0), hz: 440.0, amp: 0.5 };
v.set_sample_rate(48_000.0);
v.set_params(440.0, 0.5);
let mut block = [[0.0f32; 2]; 64];
v.process(&mut block);
```
