# Audio guide

How sound gets from a voice to the DAC, what may and may not run on the audio thread, and what it
costs on the board. Reference docs live next to the code and are linked rather than repeated here.

Numbers below come from the code (file named) or from the board measurements in
`docs/superpowers/specs/2026-11-12-layered-rust-platform-design.md`, section "Результаты замеров на
плате (2026-11-12)" ("measurements doc" below).

## Sample rate and block size

- Sample rate 48 000 Hz: `SAMPLE_RATE` in `rust/boards/jc3636k718c/hal/src/audio.rs`; the same
  value is `DEFAULT_SAMPLE_RATE` in `rust/sdk/os/src/dispatcher/mod.rs`, which the firmware passes
  to both the applets and `AudioOut::start`. The JC3636K718C `AudioOut` rejects any other rate.
- Block size 128 frames: `BLOCK_FRAMES` in the same hal file, which is 2.67 ms per block. The
  measurements doc gives the budget as 2666 us per block.
- Samples are stereo `f32` frames (`[left, right]`); the hal converts them to interleaved 16-bit PCM
  just before the I2S write (`rust/boards/jc3636k718c/hal/src/convert.rs`).
- Everything in the SDK takes the sample rate as a constructor or `set_sample_rate` argument, so
  the host renderer runs the same voices at 48 kHz (`rust/tools/render-wav/src/main.rs`).

## Signal path

```text
 Voice::process          Applet::audio            AudioPath::audio        AudioOut render callback
 (orbiversum-core)  ->   (VoiceApplet,       ->   (Dispatcher: fades,  -> (hal audio task, core 1)
 stereo f32 frames       reads Params)             applet switching)        frames_to_i16 -> I2S DMA -> DAC
```

- `Voice` (`rust/sdk/core/src/voice.rs`) renders frames into a caller buffer.
- `VoiceApplet` (`rust/sdk/os/src/voice_applet.rs`) wraps a voice as an `Applet`: on every block
  it pushes the current `Params` into the voice with `set_params`, then calls `process`.
- `Dispatcher` / `AudioPath` (`rust/sdk/os/src/dispatcher/mod.rs`) own the applets, run the active
  one and crossfade between applets on a switch. Its module docs have the state machine.
- The hal's audio task calls `AudioPath::audio` once per block; the render callback, the
  sample conversion, the DMA queue and the resulting output latency are documented at the top of
  `rust/boards/jc3636k718c/hal/src/audio.rs`. Pacing comes from the blocking I2S write, not from a
  timer.

## Control rate and audio rate

Slow modulation does not need to be computed per sample. Eno runs its control step once every
`CTRL = 32` samples (`rust/apps/dronecaster/eno/src/tuning.rs`), which is 1500 times a second at
48 kHz, and the per-sample code runs in between. Control steps happen at fixed absolute sample
positions, not at block starts: a `process` call is cut into segments that end at control-step
boundaries (`rust/apps/dronecaster/eno/src/lib.rs`). So the output depends on the seed, sample
rate, `hz` and `amp`, not on how the caller splits the stream into blocks. Both voices are
tested for this: `eno_block_size_independence` (`rust/apps/dronecaster/eno/tests/eno.rs`),
`starlids_block_size_independence` (`rust/apps/dronecaster/starlids/tests/starlids.rs`) and
`audio_independent_of_block_size` for the dispatcher (`rust/sdk/os/src/dispatcher/tests.rs`).

Block catalogue with the audio/control rate of each block: `rust/sdk/core/README.md`.

## Parameter flow between UI and audio

`Params` (`rust/sdk/os/src/params.rs`) is the only channel between an applet's UI half
(`ParamUi`, UI thread) and its audio half (`VoiceApplet`, audio thread). It holds `hz` and `amp`
as `f32` bits in atomics with relaxed ordering: the UI stores, the audio thread loads, neither
waits. One `Params` is shared by all applets, so switching applet keeps pitch and level. The
default is 110 Hz at amplitude 0.5; the UI clamps `hz` to 20..=2000 Hz and steps `amp` by 0.1
(`rust/sdk/os/src/param_ui.rs`). Because the two values are independent, a block may see a new
`hz` with the old `amp`; nothing else is published through them.

## Real-time rules

On the audio thread (`Voice::set_params`, `Voice::process`, `Applet::audio`, `AudioPath::audio`):

- no allocation or freeing, no locks, no blocking;
- shared state only through atomics (`Params`, the dispatcher's phase and target);
- applet memory is allocated in `Applet::start` and freed in `Applet::stop`, both run on the UI
  thread by `Dispatcher::service`; `Voice::set_sample_rate` is reconfiguration and may allocate.

How the rules are enforced by tests:

- `eno_no_alloc_in_process` (`rust/apps/dronecaster/eno/tests/no_alloc.rs`) and the Starlids
  counterpart count heap allocations on the test thread with a counting global allocator and
  require zero across thousands of `process` calls.
- `switch_frees_old_applet_memory` (`rust/sdk/os/src/dispatcher/tests.rs`) requires that `audio()`
  neither allocates nor frees during an applet switch, that the old applet's memory is freed in
  `service()`, and that nothing leaks.
- `stress_handoff_never_overlaps` (same file) hammers the ownership hand-over between the two
  threads; `switch_does_not_click` checks the crossfade.

One lifetime requirement outside the tests: the audio path holds a refcount on the dispatcher's
shared state, and dropping the last reference would free applet memory on the audio thread, so the
`Dispatcher` must outlive the `AudioPath` (the firmware keeps it for the whole run; see the
"What is allowed on the audio thread" section of `rust/sdk/os/src/dispatcher/mod.rs`).

## CPU budget and measurements

Measured on the board, core 1 at 240 MHz, 128-frame blocks at 48 kHz, budget 2666 us per block
(measurements doc).

| Voice | Core 1 load | Worst block (profiler) | Notes |
|---|---|---|---|
| Starlids | mean 86.9 % (about 2317 us) | 2254-2260 us | profile in us per block: control 297, saw 342, lpf 271, delay 617, mix 180, moog 451, hpf 41, other 45 |
| Eno | mean about 91.0-91.2 %, max 91-93 % (2430-2470 us) | 2380-2405 us | control about 97, pad 790, moog 462, klank 262, piano 160, perc 151-281, reverb 225, final 81; about 260 us headroom |

- Eno's target of about 86 % was not met (about 5 points higher); the owner accepted this and
  confirmed the sound by ear (measurements doc).
- Switching Starlids to Eno during menu animation peaked at 100.5-101 % (2680-2693 us, over the
  2666 us budget) with zero write errors; a click is possible there and was not checked by ear.
- The numbers were taken with the UI nearly idle, not during continuous animation.
- What made the budget reachable (measurements doc): libm `sinf/cosf/expf/powf/exp2f` replaced by
  f32 polynomials (`orbiversum-core` `fast`), f32 division removed from hot paths, `tanhf`
  replaced by a Pade approximation, code and rodata kept out of PSRAM (`SPIRAM_FETCH_INSTRUCTIONS`
  and `SPIRAM_RODATA` off in `rust/boards/jc3636k718c/firmware/sdkconfig.defaults`), and hot loops
  processed in groups of four to overlap dependent FPU chains.
- Per-section timing comes from `prof` (`rust/sdk/core/src/prof.rs`), off until the firmware
  installs a clock. Eno's tuning switches that trade sound for CPU are in
  `rust/apps/dronecaster/eno/src/tuning.rs` (`KLANK_ENABLED`, `PAD_CTRL_STRIDE`, `VOICES`).

## Adding a voice

1. Create a crate under `rust/apps/<group>/<name>/` depending on `orbiversum-core`, add it to the
   workspace members in `rust/Cargo.toml` (and to `default-members`), and use
   `[lints] workspace = true`.
2. Implement `Voice`: allocate everything in the constructor and in `set_sample_rate`; keep
   `set_params` and `process` allocation-free; make `process` independent of block size (run
   control-rate work at fixed absolute sample positions, as Eno does). The trait docs in
   `rust/sdk/core/src/voice.rs` have a complete example, `rust/sdk/core/README.md` lists the blocks.
3. Add the voice to `all()` in `rust/apps/registry/src/lib.rs` next to Starlids and Eno, wrapped
   in `VoiceApplet` and paired with a `ParamUi`. Add the dependency to
   `rust/apps/registry/Cargo.toml`. The `all_returns_matching_pairs` test there lists the names.
4. Add a `no_alloc` test and a block-size-independence test modelled on the two existing voices,
   and the voice to `render-wav` (`rust/tools/render-wav/src/main.rs`) to listen on the host.
5. Measure on the board before relying on it: the profiler sections and the budget above are the
   yardstick.
