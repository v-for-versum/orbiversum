# Orbiversum

Orbiversum is a drone synthesizer platform for ESP32-S3 boards with a round display. The audio
and UI code is `no_std` Rust (with `alloc`), layered into a reusable SDK, example voices and a
board port. The same voices run on the device, in host unit tests and in a command-line WAV
renderer, so you can develop and listen to a sound on a laptop before flashing it.

The first (and so far only) target is the **Guition JC3636K718C**: ESP32-S3R8, 8 MB PSRAM,
16 MB flash, 360x360 round display, touch, I2S DAC (PCM5100A). The repository directory keeps its
old name (`esp32-round-base`); Orbiversum is the project name.

## What it does

On the board you get an applet menu with two drones, ports of SuperCollider drones from the
Dronecaster project:

- **Starlids**: twelve slowly drifting lanes of detuned saws, each with a sweeping lowpass, a
  short wandering delay and random panning, plus a sub pulse, summed through a Moog ladder filter.
- **Eno**: a slow generative "music for airports" piece: an 8-voice pad, sub sines, a Moog
  filter, a sparse piano-string layer, a percussive PolyPerc layer, a Klank resonator and a
  reverb.

Both take two parameters, pitch (`hz`) and level (`amp`). Details of the voices:
[rust/apps/dronecaster/README.md](rust/apps/dronecaster/README.md).

### Controls

The board has no encoder push contact, so everything goes through the knob and the touch screen:

| Gesture | Action |
|---|---|
| Turn the knob | pitch up/down by one semitone (20 Hz to 2000 Hz) |
| Tap the top half of the screen | level up (+0.1) |
| Tap the bottom half of the screen | level down (-0.1) |
| Hold a finger on the screen (600 ms) | open the applet menu |
| In the menu: turn the knob, then tap or hold again | move the cursor, confirm |

Switching applets fades the old one out and the new one in (20 ms), without clicks.

## Hardware you need

- A Guition JC3636K718C board with a USB cable (the USB port is used both for flashing and for
  the serial log).
- A line-level connection from the DAC to your amplifier or headphones.

## Quick start

### 1. Try it on the host (no board needed)

You need only a stable Rust toolchain (`rustup`). From `rust/`:

```sh
cd rust
cargo test --workspace            # unit and integration tests, including the audio regression tests
cargo run --release -p render-wav -- --drone eno --seconds 30 --out eno.wav
```

`render-wav` writes a stereo 16-bit 48 kHz WAV. Flags: `--drone starlids|eno`, `--hz`, `--amp`,
`--seconds`, `--seed`, `--out` (see `rust/tools/render-wav/src/main.rs`). The output is
deterministic for a given seed and does not depend on the block size.

### 2. Build and flash the firmware

The firmware is its own cargo workspace (`rust/boards/jc3636k718c/firmware`) because it needs the
Xtensa `esp` toolchain and ESP-IDF, which a host build cannot use.

**One-time setup**

1. Install Rust via [rustup](https://rustup.rs).
2. Install the Espressif Rust tooling and the `esp` toolchain:

   ```sh
   cargo install espup espflash ldproxy
   espup install                    # installs the esp toolchain, writes ~/export-esp.sh
   ```

   `espup` prints the path of the generated environment script (`export-esp.sh`); it sets
   `LIBCLANG_PATH` and `PATH` for Xtensa builds. Source it in every shell you build from.
3. A C toolchain, CMake, Ninja and Python 3 are needed for ESP-IDF itself; the build downloads
   ESP-IDF v6.1 and its tools on the first run (several GB, 10 to 20 minutes).
4. Supply the board baseline `sdkconfig.jc3636k718c` in the repository root. It is the full
   `sdkconfig` of an ESP-IDF 6.1 build for this board, and the firmware build reads it through
   `ESP_IDF_SDKCONFIG_DEFAULTS` in `rust/boards/jc3636k718c/firmware/.cargo/config.toml`. The file
   is **not tracked in git yet**, so a fresh clone does not build until you provide it.

**Build**

```sh
cd rust/boards/jc3636k718c/firmware
. "$HOME/export-esp.sh"          # the script espup generated
cargo build --release
```

**Flash and watch the log**

Connect the board over USB, then:

```sh
cargo run --release
```

The cargo runner is `espflash flash --monitor`, so this flashes the board and opens the serial
monitor. If several serial devices are present, pick one with `ESPFLASH_PORT=/dev/cu.usbmodem...`.
Exit the monitor with Ctrl+C.

**What the log shows**

At boot you get the firmware version (`rust: firmware <git describe>`). Then, every 750 audio
blocks (about 2 s), the audio task prints its load on core 1 (`audio: core1 load mean ...`), with
per-section profile lines (`prof:`) and memory placement (`mem:`) from the firmware. The audio block is 2666 us at 48 kHz, so a load above 100% means
audible crackle. Measured numbers and how to read them: [docs/audio.md](docs/audio.md).

**Check without a board**

```sh
cargo clippy -- -D warnings      # inside the firmware directory, with the esp environment sourced
```

### Troubleshooting

- `can't find crate for std` or linker errors: the `esp` toolchain is not active. Source
  `export-esp.sh` in the same shell and run cargo from the firmware directory (it holds
  `rust-toolchain.toml`).
- The build fails right at the start about `sdkconfig.jc3636k718c`: step 4 above.
- Crackle on the device: check `core1 load` in the log. Eno runs at about 91% of the budget, so
  anything that adds work on core 1 (extra logging, a redraw storm) can push it over.
- The device is not detected: use a data USB cable (not a charge-only one) and set the port with
  `ESPFLASH_PORT`.

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
main/, CMakeLists.txt,           minimal C/ESP-IDF bring-up app: inits the BSP and idles
partitions.csv
tools/, web/                     helper scripts and a browser flasher inherited from the C app
LICENSE, THIRD-PARTY.md          licence and third-party notices
docs/audio.md                    audio guide: signal path, real-time rules, CPU budget
docs/superpowers/                design specs and plans (history)
```

Dependencies only point downward: apps -> `sdk/core`; `sdk/os` -> `sdk/core`; registry -> apps and
`sdk/os`; hal -> `sdk/os`; firmware -> hal, registry, `sdk/os`, `sdk/core` and
`dronecaster-starlids`. Hardware-agnostic abstractions (`Board`, `AudioOut`, `Display`) live in
`sdk/os`; everything specific to one board lives under `boards/<name>/`. Details of the Rust
workspace: [rust/README.md](rust/README.md).

## How it works, in short

- A **voice** (`Voice` in `orbiversum-core`) renders blocks of 128 stereo frames at 48 kHz from
  DSP building blocks (oscillators, filters, delays, reverb), with no allocation and no locks in
  the audio path.
- An **applet** (`orbiversum-os`) pairs a voice (audio half) with a UI half. `VoiceApplet` and
  `ParamUi` give any voice the standard hz/amp screen.
- The **dispatcher** splits work between the UI thread and the audio thread (pinned to core 1),
  switches applets with a fade, and hands parameters over through a lock-free `Params`.
- The **board port** (`boards/jc3636k718c`) implements the display, touch/knob input and the
  I2S audio output on top of the C board support package.

The full picture, with numbers: [docs/audio.md](docs/audio.md).

## Board support

| Board | Status |
|---|---|
| JC3636K718C (round display, PCM5100A DAC) | supported: display, touch input, audio on core 1, applet menu with Starlids and Eno |

## Where to start

- A new voice: implement `Voice` from `orbiversum-core` in a crate under `rust/apps/` and add it to
  `all()` in `rust/apps/registry/src/lib.rs`. Steps: [docs/audio.md](docs/audio.md), "Adding a
  voice". The existing voices are described in
  [rust/apps/dronecaster/README.md](rust/apps/dronecaster/README.md).
- DSP building blocks and their cost: [rust/sdk/core/README.md](rust/sdk/core/README.md).
- A new board: add `rust/boards/<name>/hal` implementing `Board`, `Display` and `AudioOut` from
  `orbiversum-os` (the JC3636K718C hal is the reference; its audio path is documented in the
  module docs of `rust/boards/jc3636k718c/hal/src/audio.rs`) and a `firmware` crate that calls
  `orbiversum_apps::all` and `Dispatcher` (`rust/boards/jc3636k718c/firmware/src/main.rs`).
- Applet and dispatcher internals: the crate docs, `cargo doc --no-deps -p orbiversum-os --open`
  from `rust/`.

## Known limitations

- Eno uses about 91% of core 1 (target was 86%); switching from Starlids to Eno while the menu
  animates exceeded the block budget in measurements (max 100.5-101%), so a click is possible.
- Only one board is supported; the firmware build needs the untracked `sdkconfig.jc3636k718c`.
- The `main/` C application and the scripts in `tools/` come from the earlier C firmware and are
  not used by the Rust build.

## License

MIT, see [LICENSE](LICENSE). Third-party components and their licenses: [THIRD-PARTY.md](THIRD-PARTY.md).
