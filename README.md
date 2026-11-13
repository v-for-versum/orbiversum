# Orbiversum

Orbiversum is a round-display ESP32-S3 drone synthesizer platform. The audio and UI code is
`no_std` Rust (with `alloc`) layered into a reusable SDK, example voices and a board port, so the
same voices run on the board and in host tests and a WAV renderer. The first target is the
JC3636K718C board (round display, I2S DAC, touch). The repository directory keeps its old name;
Orbiversum is the project name.

## Layout

```text
rust/
  sdk/core/                      orbiversum-core: DSP blocks, the Voice trait, profiler
  sdk/os/                        orbiversum-os: Applet, Board, AudioOut, Dispatcher, Params,
                                 the generic VoiceApplet / ParamUi adapters
  apps/dronecaster/eno/          dronecaster-eno: the Eno voice
  apps/dronecaster/starlids/     dronecaster-starlids: the Starlids voice
  apps/registry/                 orbiversum-apps: all(), wires the apps together
  boards/jc3636k718c/hal/        orbiversum-board-jc3636k718c: display, input, audio, menu
  boards/jc3636k718c/firmware/   orbiversum-firmware-jc3636k718c: the firmware binary
                                 (its own cargo workspace, esp toolchain)
  tools/render-wav/              render-wav: host renderer, voice -> WAV
components/bsp_jc3636k718c/      C board support package (I2S, display, touch) used by the firmware
docs/audio.md                    audio guide: signal path, real-time rules, CPU budget
docs/superpowers/                design specs and plans (history)
```

Dependencies only point downward: apps -> `sdk/core`; `sdk/os` -> `sdk/core`; registry -> apps and
`sdk/os`; hal -> `sdk/os`; firmware -> hal and registry. Hardware-agnostic abstractions (`Board`,
`AudioOut`, `Display`) live in `sdk/os`; everything specific to one board lives under
`boards/<name>/`. Details of the Rust workspace: [rust/README.md](rust/README.md).

## Quick start

Host tests (from `rust/`):

```sh
cd rust && cargo test --workspace
```

Render a WAV (stereo, 16-bit, 48 kHz; flags `--drone starlids|eno`, `--hz`, `--amp`, `--seconds`,
`--seed`, `--out`, see `rust/tools/render-wav/src/main.rs`):

```sh
cd rust && cargo run --release -p render-wav -- --drone eno --seconds 30 --out eno.wav
```

Build the firmware. It needs the `esp` Rust toolchain and the Xtensa/ESP-IDF environment loaded
into the shell (the file below is the one used on the author's machine; it sets `LIBCLANG_PATH`
and `PATH`, use your own equivalent). The toolchain is selected by
`rust/boards/jc3636k718c/firmware/rust-toolchain.toml` (`channel = "esp"`), the target is
`xtensa-esp32s3-espidf`, and ESP-IDF v6.1 is fetched by the build
(`rust/boards/jc3636k718c/firmware/.cargo/config.toml`):

```sh
cd rust/boards/jc3636k718c/firmware && . /Users/noise/export-esp.sh && cargo build --release
```

Flashing uses `espflash flash --monitor` (the cargo runner in the same config file).

## Board support

| Board | Status |
|---|---|
| JC3636K718C (round display, PCM5100A DAC) | supported: display, touch input, audio on core 1, applet menu with Starlids and Eno |

## Where to start

- A new voice: implement `Voice` from `orbiversum-core` in a crate under `rust/apps/` and add it to
  `all()` in `rust/apps/registry/src/lib.rs`. Steps: [docs/audio.md](docs/audio.md), "Adding a
  voice". The existing voices are described in [rust/apps/dronecaster/README.md](rust/apps/dronecaster/README.md).
- DSP building blocks and their cost: [rust/sdk/core/README.md](rust/sdk/core/README.md).
- A new board: add `rust/boards/<name>/hal` implementing `Board`, `Display` and `AudioOut` from
  `orbiversum-os` (the JC3636K718C hal is the reference; its audio path is documented in the
  module docs of `rust/boards/jc3636k718c/hal/src/audio.rs`) and a `firmware` crate that calls
  `orbiversum_apps::all` and `Dispatcher` (`rust/boards/jc3636k718c/firmware/src/main.rs`).
- Applet and dispatcher internals: the crate docs, `cargo doc --no-deps -p orbiversum-os --open`
  from `rust/`.

## License

MIT, see [LICENSE](LICENSE). Third-party components and their licenses: [THIRD-PARTY.md](THIRD-PARTY.md).
