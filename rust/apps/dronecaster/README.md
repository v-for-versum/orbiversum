# Dronecaster

Dronecaster is a pair of generative drone voices for the Orbiversum synth, written as `no_std`
Rust ports of SuperCollider drone patches. The code states the originals by name: Eno is a port
of dronecaster's `Eno.scd`, Starlids a port of dronecaster's `Starlids.scd` (see the crate-level
docs). The repository's `THIRD-PARTY.md` does not list the originals, so nothing more about their
origin or licence is claimed here.

Each voice is an audio-rate generator built from the blocks of `orbiversum-core`
(`rust/sdk/core`) and implements its `Voice` trait: `set_params(hz, amp)` sets the base pitch and
level, `process` fills stereo `f32` frames. They allocate only at construction (and in `set_sample_rate`), so they run in the
real-time audio task. The output is a function of the seed, sample rate, `hz` and `amp` only, not of
the block size the caller uses.

| Crate | Voice | What it is |
|---|---|---|
| `dronecaster-starlids` (`starlids/`) | `Starlids` | Twelve detuned saw lanes drifting in 4- and 5-semitone steps, slow lowpass, phasing delay and pan per lane, a sub pulse, a Moog ladder and a highpass. |
| `dronecaster-eno` (`eno/`) | `Eno` | An evolving pad of eight voices on changing chord planes with sub sines, a Moog ladder, Klank resonators, a sparse piano string and PolyPerc note layer, and a reverb. |

On the board both voices show the dronecaster `classic` scene (antenna, blinking lights, birds, wind, rare UAP) as line art, ported from the original `classic.lua` (`apps/registry/src/scene.rs`), and sit in the `dronecaster` group of the two-level menu (hold to open, knob to move, tap right to enter or select, tap left to go back).

Details of the signal flow, the control rate and the CPU budget are in the crate docs; build them
with `cargo doc --no-deps -p dronecaster-eno -p dronecaster-starlids --open` from `rust/`.

## Render a WAV on the host

From the `rust/` directory (stereo, 16-bit, 48 kHz):

```sh
cargo run --release -p render-wav -- --drone eno --seconds 30 --out eno.wav
```

Options: `--drone starlids|eno`, `--hz` (default 110), `--amp` (default 0.5), `--seed` (default 1),
`--seconds`, `--out`.
