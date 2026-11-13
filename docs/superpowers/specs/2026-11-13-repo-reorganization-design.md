# Repo reorganization: sdk / apps / boards, audio docs, clippy-clean

Date: 2026-11-13. Status: draft for approval.

## Intent and why

The repository is going public. It must be understandable and useful to a
stranger: someone should be able to find the reusable DSP building blocks,
see how an example synth app is made from them, and see what a board port
needs. Today `crates/dsp` mixes reusable parts (`voice.rs`, `prof.rs`) with
concrete apps (`eno`, `starlids`, `sine`), the board crate and the firmware
binary live in different places, and the audio code has uneven docs.

## Goals

1. Layered layout: reusable SDK, example apps, board ports, tools.
2. Audio/DSP code documented well enough to learn from: units, ranges,
   formulas, signal flow, real-time rules, CPU budget.
3. `cargo clippy` clean (zero warnings, zero errors) on the host workspace and
   on the firmware crate, enforced by shared lint config, not by blanket allows.
4. Large files split where it improves cohesion (`dsp/src/eno.rs` 606 lines,
   `os/src/dispatcher.rs` 617 lines).

## Non-goals / anti-goals

- No behaviour change. Audio output stays bit-identical, UI behaviour stays
  identical. A refactor that "improves" the sound is a failure.
- No new features, no new applets, no API redesign beyond what moves require.
- No performance regression on the board (Eno is CPU-tight). Hot loops are
  moved, not rewritten.
- Not a wholesale `cargo fmt` of the repo unless it is already fmt-clean
  (it would bury the real diff).

## Target layout (under `rust/`)

```
sdk/core/                 was crates/blocks + dsp/{voice,prof}: blocks, Voice, profiler
sdk/os/                   was crates/os: Applet, Board, AudioOut, Dispatcher, Params
                          (+ generic VoiceApplet / ParamUi adapters from crates/applets)
apps/dronecaster/eno/       group "dronecaster": Eno and Starlids, one crate each,
apps/dronecaster/starlids/  depend on sdk/core
apps/registry/            was crates/applets::all(): wires the apps together
boards/jc3636k718c/hal/       was crates/board-jc3636k718c (display, input, audio, menu)
boards/jc3636k718c/firmware/  was firmware-jc3636k718c (own workspace, xtensa only)
tools/render-wav/
```

Dependency direction only goes downward: apps -> sdk/core; sdk/os -> sdk/core;
registry -> apps + sdk/os; hal -> sdk/os; firmware -> hal + registry.
Hardware-agnostic abstractions (`Board`, `AudioOut`, `Display`) stay in
`sdk/os`; everything specific to one board lives under `boards/<name>/`.

## Constraints

- The firmware crate is its own cargo workspace on the `esp` toolchain,
  target `xtensa-esp32s3-espidf`. It references the C BSP and sdkconfig at the
  repo root (`../../components/bsp_jc3636k718c`, `../../sdkconfig.jc3636k718c`,
  `.cargo/config.toml` `ESP_IDF_SDKCONFIG_DEFAULTS`). Those relative paths
  change when it moves two levels deeper and must be fixed and verified.
- Firmware cannot be built in every environment. If it cannot be built here,
  at least `cargo check`/clippy for the target runs, and the exact build
  command for the owner is reported.
- Existing untracked/modified root files (`platformio.ini`,
  `sdkconfig.jc3636k718c`, `dependencies.lock`, `.gitignore`,
  `docs/superpowers/plans/2026-11-12-eno-applet.md`) are not part of this
  work and are not committed by it; only paths that point at them are updated.
- `core` is not a usable crate name; package names are prefixed (see calls).

## Approaches considered

- A. Rename/regroup crates only. Low risk, but leaves SDK and apps mixed.
- B. Layered sdk / apps / boards (chosen by the owner). Clear roles, new apps
  and boards are additive. Cost: biggest path churn, firmware move.
- C. One `sdk` crate with feature-gated modules. Fewer manifests, but apps and
  boards cannot be separated and firmware stays a special case.

## Verification (the safety net)

1. Before any move: record golden output with `render-wav` for every voice
   with fixed seed and sample rate, and a clean baseline of `cargo test`.
2. After every step: tests green; golden WAVs bit-identical.
3. Clippy: `cargo clippy --workspace --all-targets -- -D warnings` on host;
   firmware with the esp toolchain.
4. Work happens on a new branch off `scaffold`, one commit per step, each
   commit green, so any step reverts alone.

## Documentation deliverables

- Root `README.md`: what this is, layout, quick start (host tests, render a
  WAV, build firmware), pointers to the layers.
- `rust/sdk/core/README.md`: block catalogue (what each does, cost).
- `docs/audio.md`: sample rate, block size, signal path through the
  dispatcher, control vs audio rate, parameter flow between UI and audio,
  real-time rules (no allocation or locking in the audio path, enforced by
  the `no_alloc` test), CPU budget and measurements.
- Rustdoc: every public item in `sdk/core` and `apps/*` has `///` with units
  and ranges; each block states its algorithm/formula and reference where
  applicable; each module has a `//!` overview. `#![warn(missing_docs)]` on
  the SDK crates once complete.

## My calls (owner may change)

- Project name is Orbiversum (owner's decision); Eno and Starlids belong to
  the `dronecaster` group (owner's decision). Package names: `orbiversum-core`,
  `orbiversum-os`, `dronecaster-eno`, `dronecaster-starlids`,
  `orbiversum-apps` (registry), `orbiversum-board-jc3636k718c` (hal),
  `orbiversum-firmware-jc3636k718c`. Crate idents: `orbiversum_core`, ...
- `sine` was only a test voice; the owner said to drop it. It is deleted
  (`dsp/src/sine.rs`, `dsp/tests/sine.rs`, its entry in `applets::all()` and
  `render-wav`), as its own commit, before the moves. Golden baselines then
  cover Starlids and Eno only.
- The repo directory and git remote are not renamed; the root README and
  workspace metadata use the name Orbiversum.
- Generic adapters `VoiceApplet`/`ParamUi` move to `sdk/os`; `all()` moves
  to `apps/registry`.
- `eno.rs` and `dispatcher.rs` become module directories split by
  responsibility (e.g. eno: pad, moog, klank, piano, perc, reverb sections),
  only where it does not touch hot-loop structure.
- Lints defined once in `[workspace.lints]`; the firmware workspace repeats
  the same table (it cannot inherit). Any `#[allow]` must carry a reason.
- Old docs in `docs/superpowers/` stay as history; paths in them are not
  rewritten except a one-line pointer to the new layout.

## Left to the builder

Exact split points inside `eno.rs`/`dispatcher.rs`, which hal files stay in
the hal crate vs `sdk/os`, step order, fixing each clippy finding.

## Resolved after review

Facts checked in the repo:
- The C/PlatformIO side (`CMakeLists.txt`, `platformio.ini`, `main/`) does not
  reference the Rust crates, so moving the firmware only touches its own
  `.cargo/config.toml` and `Cargo.toml` paths.
- `render-wav` is deterministic: `--seed` (default 1) drives Starlids and Eno.
- The `esp` toolchain is installed, so firmware clippy/check can run here.
- `no_alloc` exists (`crates/dsp/tests/no_alloc.rs`); tests move with the code
  they test. Apps depend on `orbiversum-core` only; `VoiceApplet` in `orbiversum-os` adapts
  the `Voice` trait.
- No CI, no root README, no external consumers of the old package names, so
  no re-export shims. Moves use `git mv` to keep blame.
- `cargo fmt --check` is not clean, which confirms: no repo-wide fmt.
- The firmware keeps its own workspace and lockfile via `path` dependencies
  (`../../../sdk/core`, ...); host and firmware lockfiles stay separate.

My calls:
- Goldens are throwaway baselines kept outside the repo (scratch dir), for
  Starlids and Eno, with Eno rendered past the 8 s idle cut-off. Any
  bit difference after a step: stop and investigate, no epsilon.
- `sdk/os` keeps its `embedded-graphics` dependency (already has it);
  `sdk/core` stays free of UI and std dependencies.
- Clippy: default lint groups, `--workspace --all-targets -D warnings`. No
  pedantic/nursery. A fix that would change float/integer results or hot-path
  speed is not applied; use a targeted `#[allow(..., reason = "...")]`.
- `#![warn(missing_docs)]` on `orbiversum-core` and apps, with full coverage reached
  in the same pass.
- `.gitignore`: layout-related edits go in their own commit; the existing
  uncommitted `.pio/` line stays uncommitted unless the owner says otherwise.
- Branch `refactor/layout` off `scaffold`, in place (not a worktree), because
  of the untracked root files.
