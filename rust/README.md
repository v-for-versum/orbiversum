# Rust workspace

Everything Rust lives here. Project overview and quick start: [../README.md](../README.md); audio
path, real-time rules and CPU budget: [../docs/audio.md](../docs/audio.md).

## Workspace members

Defined in `Cargo.toml` (`members` and `default-members` are the same list; host toolchain
`stable`, `rust-toolchain.toml`):

| Path | Package | Role |
|---|---|---|
| `sdk/core` | `orbiversum-core` | `no_std` DSP blocks, `Voice` trait, profiler ([README](sdk/core/README.md)) |
| `sdk/os` | `orbiversum-os` | `Applet`, `Board`, `AudioOut`, `Dispatcher`, `Params`, `VoiceApplet`, `ParamUi` |
| `apps/dronecaster/eno` | `dronecaster-eno` | Eno voice ([group README](apps/dronecaster/README.md)) |
| `apps/dronecaster/starlids` | `dronecaster-starlids` | Starlids voice |
| `apps/registry` | `orbiversum-apps` | `all()`: voices wrapped as applets |
| `boards/jc3636k718c/hal` | `orbiversum-board-jc3636k718c` | display, input, audio, menu for the board |
| `tools/render-wav` | `render-wav` | host renderer, voice to WAV |

The hal crate is a member so its host-testable parts (`convert`, `display`, `input`, `menu`) are
tested on the host; `audio`, `jc` and `ffi` are compiled only for `target_os = "espidf"`
(`boards/jc3636k718c/hal/src/lib.rs`).

## The firmware is a separate workspace

`boards/jc3636k718c/firmware` (package `orbiversum-firmware-jc3636k718c`) is listed under
`exclude` in `Cargo.toml` and is its own workspace root. It builds with the `esp` toolchain
(`firmware/rust-toolchain.toml`) for `xtensa-esp32s3-espidf`, with `build-std`, and pulls in
ESP-IDF and the C board support package (`../../components/bsp_jc3636k718c` at the repository
root) and `sdkconfig.jc3636k718c` through paths in `firmware/Cargo.toml` and
`firmware/.cargo/config.toml`. None of this is available to a host `cargo test --workspace`, and
the host toolchain cannot build for the target, so the two must not share a workspace. Build
command: [../README.md](../README.md), "Quick start". The firmware has its own `Cargo.lock`.

## Lints

`[workspace.lints.clippy]` in `Cargo.toml` enables the default `clippy::all` group at warn level;
member crates opt in with `[lints] workspace = true`. The firmware workspace cannot inherit it and
repeats the same table in its own `Cargo.toml`; keep the two in step. The bar is zero warnings:

```sh
cargo clippy --workspace --all-targets -- -D warnings      # in rust/
cargo clippy -- -D warnings                                # in rust/boards/jc3636k718c/firmware, esp toolchain
```

Every `#[allow(clippy::...)]` carries a `reason = "..."`. `sdk/core`, `sdk/os` and the two dronecaster crates also build with
`#![warn(missing_docs)]`; check docs with `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`.
The repository is not `cargo fmt`-clean, so do not run a wholesale `cargo fmt`; format only code
you write.
