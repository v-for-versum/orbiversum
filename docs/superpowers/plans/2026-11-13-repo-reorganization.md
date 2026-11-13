# Orbiversum repo reorganization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the `rust/` tree into a layered, documented, clippy-clean public repo (sdk / apps / boards) with bit-identical audio output.

**Architecture:** Clean clippy and drop `sine` first on the current layout (small diffs), then move crates layer by layer (`git mv`, rename packages, fix paths), then split the two big files, then write docs. Every task ends green on tests and golden WAVs.

**Tech Stack:** Rust (host: stable via `rust/rust-toolchain.toml`; firmware: `esp` toolchain, target `xtensa-esp32s3-espidf`), cargo workspaces, clippy, `render-wav` for golden audio.

**Spec:** `docs/superpowers/specs/2026-11-13-repo-reorganization-design.md`

## Global Constraints

- Behaviour never changes: Starlids and Eno output stays bit-identical (`cmp` on golden WAVs), UI behaviour unchanged. A bit difference means stop and investigate, no epsilon.
- No wholesale `cargo fmt` (repo is not fmt-clean). Format only code you write or move, with `rustfmt` on those files if they were already fmt-clean; otherwise match surrounding style.
- Clippy command: `cargo clippy --workspace --all-targets -- -D warnings` (host) and the same inside the firmware dir (esp toolchain). Default lint groups only, no pedantic/nursery. A fix that could change float/integer results or hot-path speed is not applied: use `#[allow(clippy::lint, reason = "...")]` instead. Every `#[allow]` carries a `reason`.
- Package names: `orbiversum-core`, `orbiversum-os`, `dronecaster-eno`, `dronecaster-starlids`, `orbiversum-apps`, `orbiversum-board-jc3636k718c`, `orbiversum-firmware-jc3636k718c`; `render-wav` keeps its name. Crate idents use underscores.
- Use `git mv` for every move so blame survives. Do not rename the repo directory or remote.
- Do not stage or commit the owner's pre-existing uncommitted files: `.gitignore` hunk `.pio/`, `dependencies.lock`, `platformio.ini`, `sdkconfig.jc3636k718c`, `docs/superpowers/plans/2026-11-12-eno-applet.md`. Always `git add` explicit paths, never `-A`/`-a`. `.gitignore` edits for the new layout go in their own commit via `git add -p`, taking only the new-layout hunks.
- `sine` is deleted, not moved.
- Branch `refactor/layout` off `scaffold`, in place (no worktree). Commit trailer: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Golden/baseline files live outside the repo in `GOLDEN=/private/tmp/claude-501/-Users-noise-code-esp32-round-base-rust-firmware-jc3636k718c/8e8bdeb2-f135-4660-b2b1-b97141766ea6/scratchpad/golden`.
- All cargo commands for the host run from `/Users/noise/code/esp32-round-base/rust` unless a task says otherwise.

## Review Focus

Failure modes the tasks' own tests would not catch; each is pinned by a step in the owning task.

1. Eno stops processing the piano string loop 8 s after the last note (`PIANO_LIVE_S`): a short golden would miss a move that breaks it. Goldens render 30 s. (Task 1 creates them, every task compares.)
2. Output must not depend on the caller's block size (Eno control rate `CTRL = 32`): `audio_independent_of_block_size` and the Eno block-size test must still exist and pass after moves. (Task 4, 5, 7, 8.)
3. Tests silently lost in a move: the sorted test-name list must equal the baseline (minus the sine tests). (Every task.)
4. `no_alloc` guard must still run against the moved audio path. (Task 5.)
5. `#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]` must move with `prof.rs`; host stable must still build and the firmware must still build. Firmware path changes (`ESP_IDF_SDKCONFIG_DEFAULTS`, `component_dirs`, path deps) are only proven by a firmware build or check. (Task 4, 6.)

---

### Task 1: Branch, baselines and golden check script

Риск: обычный — только подготовка, код не меняется.

**Files:**
- Create (outside repo): `$GOLDEN/check.sh`, `$GOLDEN/*.wav`, `$GOLDEN/tests.txt`, `$GOLDEN/clippy-baseline.txt`, `$GOLDEN/firmware-baseline.txt`

**Interfaces:**
- Produces: `$GOLDEN/check.sh` — renders the golden set with the current tree and exits non-zero if any WAV differs from baseline; `$GOLDEN/tests.txt` — sorted test names. Later tasks run `sh $GOLDEN/check.sh` and compare test lists.

- [ ] **Step 1: Create the branch**

Run: `cd /Users/noise/code/esp32-round-base && git switch -c refactor/layout`
Expected: `Switched to a new branch 'refactor/layout'`; `git status --short` still lists the five pre-existing files.

- [ ] **Step 2: Write `$GOLDEN/check.sh`**

It renders with `cargo run -q --release -p render-wav -- --drone <d> --hz <hz> --amp <a> --seconds 30 --seed <s> --out <file>` for this set: `starlids` hz 110 amp 0.5 seed 1; `starlids` hz 55 amp 0.8 seed 7; `eno` hz 110 amp 0.5 seed 1; `eno` hz 55 amp 0.8 seed 7. With argument `baseline` it writes into `$GOLDEN/base/`; otherwise into `$GOLDEN/new/` and `cmp`s each file with `base/`, printing `OK` or `DIFF <name>` and exiting non-zero on any diff.

- [ ] **Step 3: Record baselines**

Run: `sh $GOLDEN/check.sh baseline`, then
`cd rust && cargo test --workspace -- --list 2>/dev/null | grep ': test$' | sort > $GOLDEN/tests.txt` (record the count), and
`cargo clippy --workspace --all-targets 2>&1 > /dev/null | grep -E '^(warning|error)' > $GOLDEN/clippy-baseline.txt`.
Then in `rust/firmware-jc3636k718c` run `cargo check 2>&1 | tail -20 > $GOLDEN/firmware-baseline.txt`.
Expected: all `cargo test` results ok; clippy baseline shows the known `approx_constant` errors; the firmware baseline records whether the firmware builds at all before any move (later failures are attributed against it).

- [ ] **Step 4: Verify determinism**

Run `sh $GOLDEN/check.sh` twice.
Expected: both runs print `OK` for all four files. If not, stop and report: determinism is the premise of the whole plan.

No commit (nothing in the repo changed).

---

### Task 2: Delete `sine`

Риск: обычный — удаление тестовой озвучки, аудио Starlids/Eno не затрагивается.

**Files:**
- Delete: `rust/crates/dsp/src/sine.rs`, `rust/crates/dsp/tests/sine.rs`
- Modify: `rust/crates/dsp/src/lib.rs`, `rust/crates/applets/src/lib.rs` (`all()` and its test), `rust/tools/render-wav/src/main.rs` (drone match and error text), any test or doc referencing `Sine`/`sine` (find with `grep -rIn -i sine rust --include='*.rs' --include='*.md' --include='*.toml' --exclude-dir=target`)

**Interfaces:**
- Produces: `applets::all(seed: u64, sr: f32, params: Arc<Params>) -> Vec<(Box<dyn Applet + Send>, Box<dyn AppletUi>)>` returning exactly Starlids then Eno.

- [ ] **Step 1: Update the test first.** In `applets::tests::all_returns_matching_pairs` expect names `[("Starlids","Starlids"), ("Eno","Eno")]`. Run `cargo test -p applets` and see it FAIL.
- [ ] **Step 2: Remove sine** from all files above; `render-wav` error text becomes `unknown drone {other} (starlids|eno)`. Check `firmware-jc3636k718c/src/main.rs` and `crates/board-jc3636k718c` for applet-index assumptions (menu length, default selection) and adjust them if they relied on three entries.
- [ ] **Step 3: Verify.** `cargo test --workspace` passes; `sh $GOLDEN/check.sh` prints OK x4; regenerate `$GOLDEN/tests.txt` as in Task 1 and confirm the only removed names are the sine tests (diff against the old list).
- [ ] **Step 4: Commit** `git add` the changed paths explicitly. Message: `refactor: remove the sine test voice`.

---

### Task 3: Workspace lints and clippy-clean (current layout)

Риск: высокий — правки в realtime-аудио коде (`blocks`, `dsp`), где «безобидный» фикс может изменить биты.

**Files:**
- Modify: `rust/Cargo.toml`, every crate `Cargo.toml` (`[lints] workspace = true`), `rust/firmware-jc3636k718c/Cargo.toml` (own `[lints.clippy]` table, same content, with a comment saying it mirrors the host workspace), source files with findings.

**Interfaces:**
- Produces: `[workspace.lints.clippy] all = { level = "warn", priority = -1 }` in `rust/Cargo.toml`; all later tasks keep `cargo clippy --workspace --all-targets -- -D warnings` at zero.

- [ ] **Step 1: Add lint config** as above; add `[lints] workspace = true` to each member crate.
- [ ] **Step 2: Run clippy.** `cargo clippy --workspace --all-targets -- -D warnings`. The baseline stopped at the first crate failing to compile, so more findings will appear as you fix; loop until zero. Known findings: 4 `approx_constant` errors, 4 `excessive_precision`, 9 never-read fields, 2 `should_implement_trait`, 1 constant `chunks`, 1 unnecessary cast/`or`, 1 `neg_cmp_op_on_partial_ord`.
- [ ] **Step 3: Apply fixes by these rules.**
  - `excessive_precision`: shorten the literal only if the shortened literal parses to the same `f32` bits (assert in a scratch test or by comparing `to_bits()`); otherwise allow with reason.
  - `approx_constant`: replace by `core::f32::consts::*` only if `to_bits()` is identical to the literal; otherwise `#[allow(clippy::approx_constant, reason = "literal kept to preserve bit-exact output")]`.
  - `neg_cmp_op_on_partial_ord`: keep NaN semantics exactly (`!(a < b)` is true for NaN); rewrite with `partial_cmp` only in a form that is equivalent for NaN, else allow with reason.
  - never-read fields: remove only if removal cannot affect output (no `Drop`, no state); otherwise `#[allow(dead_code, reason = ...)]` naming what the field is for.
  - `should_implement_trait`: rename the method if it has no external callers outside the repo (all callers are in the repo), else allow with reason.
- [ ] **Step 4: Verify.** Clippy is zero on host; `cargo clippy -- -D warnings` zero inside `rust/firmware-jc3636k718c` (compare with `$GOLDEN/firmware-baseline.txt` if it cannot run); `cargo test --workspace` green; `sh $GOLDEN/check.sh` OK x4; test list equals `$GOLDEN/tests.txt`.
- [ ] **Step 5: Commit.** `chore: workspace lints, clippy clean`. Keep it separate from any later moves.

---

### Task 4: `sdk/core` and `sdk/os`

Риск: высокий — меняются границы крейтов в аудио-пути (диспетчер, `Voice`), это может изменить кодген и порядок инициализации.

**Files:**
- Move: `rust/crates/blocks` → `rust/sdk/core` (package `orbiversum-core`); `crates/dsp/src/voice.rs` → `sdk/core/src/voice.rs`; `crates/dsp/src/prof.rs` → `sdk/core/src/prof.rs`; `crates/os` → `sdk/os` (package `orbiversum-os`); `crates/applets/src/voice_applet.rs` → `sdk/os/src/voice_applet.rs`; `crates/applets/src/ui.rs` → `sdk/os/src/param_ui.rs`.
- Modify: `rust/Cargo.toml` (members, default-members), `sdk/core/src/lib.rs`, `sdk/os/src/lib.rs`, every dependent `Cargo.toml` and `use` path (`blocks::x` → `orbiversum_core::x`, `dsp::voice::Voice` → `orbiversum_core::voice::Voice`, `os::` → `orbiversum_os::`), `crates/applets` and `crates/dsp` keep existing until Task 5, `firmware-jc3636k718c/Cargo.toml` and `src/`.
- Move tests: tests that exercise only blocks/Voice/prof (`crates/blocks/tests/block_size.rs`, and any in `crates/dsp/tests` that touch only core types; check `no_alloc.rs`) go with `sdk/core`.

**Interfaces:**
- Produces: `orbiversum_core::{fast, osc, moog, biquad, resonant, comb, delay, reverb, env, lfo, noise, rng, pan, seq, voice, prof}` as flat public modules (the existing block module names; read `blocks/src/lib.rs` for the exact list); `orbiversum_core::voice::Voice`; `orbiversum_os::{Applet, AppletUi, AppletInfo, Board, AudioOut, Display, Dispatcher, AudioPath, Params, VoiceApplet, ParamUi, AMP_STEP, MIN_HZ, MAX_HZ, ...}` (all current `os` exports plus the moved adapters).
- Consumes: Task 3 lints (`[lints] workspace = true` stays in the moved crates).

- [ ] **Step 1: Move and rename.** `git mv` the paths above; set package names; add `orbiversum-core` as a dependency of `orbiversum-os`; fix workspace members. `sdk/core/src/lib.rs` carries the `#![no_std]` and `#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]` lines from `crates/dsp/src/lib.rs` (check that `blocks/src/lib.rs` already has `no_std`).
- [ ] **Step 2: Fix every `use`** and rebuild: `cargo build --workspace --all-targets`.
- [ ] **Step 3: Verify.** Clippy zero; `cargo test --workspace` green; `sh $GOLDEN/check.sh` OK x4; sorted test list equals `$GOLDEN/tests.txt`; `audio_independent_of_block_size` still listed.
- [ ] **Step 4: Verify xtensa gate.** `grep -n asm_experimental_arch sdk/core/src/lib.rs` shows the gate; in `rust/firmware-jc3636k718c` (path deps updated to the new locations) run `cargo check`; result must equal the baseline in `$GOLDEN/firmware-baseline.txt` (builds if it built before).
- [ ] **Step 5: Commit.** One commit: `refactor: sdk/core and sdk/os layers`.

---

### Task 5: `apps/dronecaster/{eno,starlids}` and `apps/registry`

Риск: высокий — аудио-код (Eno, Starlids) переезжает в отдельные крейты, меняется инлайнинг; тест `no_alloc` охраняет realtime-правило.

**Files:**
- Move: `rust/crates/dsp/src/eno.rs` → `rust/apps/dronecaster/eno/src/lib.rs` (package `dronecaster-eno`); `crates/dsp/src/starlids.rs` → `apps/dronecaster/starlids/src/lib.rs` (package `dronecaster-starlids`); `crates/dsp/tests/{eno.rs, starlids.rs, starlids_ref.rs}` into the matching app's `tests/`; `crates/applets` → `apps/registry` (package `orbiversum-apps`, keeps only `all()` and its test).
- Move or keep: `crates/dsp/tests/no_alloc.rs` goes to the crate whose audio path it tests (read it; if it tests both apps, split it per app so each app crate keeps a no-alloc test).
- Delete: `rust/crates/dsp` (empty), and `rust/crates/` when empty.
- Modify: `rust/Cargo.toml`, `tools/render-wav` (dependencies and imports: `dronecaster_eno::Eno`, `dronecaster_starlids::Starlids`, `orbiversum_core::voice::Voice`), firmware `Cargo.toml`/`src/`.

**Interfaces:**
- Produces: `dronecaster_eno::Eno` with `Eno::new(seed: u64, sr: f32) -> Eno`, `Eno::plane_notes(&self) -> [i8; 4]`, `P_*` section constants; `dronecaster_starlids::Starlids::new(seed: u64, sr: f32)`; `orbiversum_apps::all(seed: u64, sr: f32, params: Arc<Params>) -> Vec<(Box<dyn Applet + Send>, Box<dyn AppletUi>)>`. All keep their current signatures.
- Consumes: Task 4 module paths.

- [ ] **Step 1: Move, rename, delete `crates/dsp`.**
- [ ] **Step 2: Fix imports and rebuild.** Apps depend on `orbiversum-core` only (not `orbiversum-os`).
- [ ] **Step 3: Verify.** Clippy zero; `cargo test --workspace` green; `sh $GOLDEN/check.sh` OK x4; sorted test list equals `$GOLDEN/tests.txt`; `grep -rn "fn no_alloc\|no_alloc" apps sdk` shows the guard still present and its test appears in `cargo test --workspace -- --list`.
- [ ] **Step 4: Firmware check** as in Task 4 Step 4.
- [ ] **Step 5: Commit.** `refactor: dronecaster apps and registry crates`.

---

### Task 6: `boards/jc3636k718c/{hal,firmware}`

Риск: высокий — пути ESP-IDF/sdkconfig ломаются молча, ошибка видна только при сборке прошивки.

**Files:**
- Move: `rust/crates/board-jc3636k718c` → `rust/boards/jc3636k718c/hal` (package `orbiversum-board-jc3636k718c`); `rust/firmware-jc3636k718c` → `rust/boards/jc3636k718c/firmware` (package `orbiversum-firmware-jc3636k718c`).
- Modify: `rust/Cargo.toml` (members: `boards/jc3636k718c/hal`; `exclude = ["boards/jc3636k718c/firmware"]`), firmware `Cargo.toml` (path deps `../hal`, `../../../sdk/os`, `../../../apps/registry`; `component_dirs = ["../../../../components/bsp_jc3636k718c"]`; drop `dsp`/old deps if unused, keep `orbiversum-core` if `main.rs` uses it), firmware `.cargo/config.toml` (`ESP_IDF_SDKCONFIG_DEFAULTS = "../../../../sdkconfig.jc3636k718c;sdkconfig.defaults"`), firmware `build.rs` if it contains relative paths, hal `Cargo.toml` and `use os::` → `use orbiversum_os::`.
- Modify (separate commit): `/Users/noise/code/esp32-round-base/.gitignore` or `rust/.gitignore` if the new `target/` and `.embuild/` locations are not ignored (`git status` must show no build artifacts).

**Interfaces:**
- Consumes: Task 4 and 5 package names. The paths above are relative to `rust/boards/jc3636k718c/firmware`, whose repo root is four levels up.

- [ ] **Step 1: Move and rename** (`git mv` the firmware directory including its lockfiles; build output moves with it, fine).
- [ ] **Step 2: Fix paths** as listed; grep the firmware dir for `../` to find any other relative path.
- [ ] **Step 3: Host verify.** Clippy zero; `cargo test --workspace` green; `sh $GOLDEN/check.sh` OK x4; test list equals `$GOLDEN/tests.txt`.
- [ ] **Step 4: Firmware verify.** In `rust/boards/jc3636k718c/firmware`: `cargo clippy -- -D warnings` zero, and `cargo build --release` (or `cargo check` if the baseline could only check). Compare against `$GOLDEN/firmware-baseline.txt`. If it builds, confirm the ESP-IDF build log shows the BSP component and the `sdkconfig.jc3636k718c` paths resolved. If a full build is impossible in this environment, report the exact command for the owner.
- [ ] **Step 5: Commit** the move, then commit `.gitignore` changes separately (`git add -p`).

---

### Task 7: Split `eno.rs` into modules

Риск: высокий — Eno упирается в бюджет CPU на плате, горячий цикл должен остаться тем же.

**Files:**
- Modify: `rust/apps/dronecaster/eno/src/lib.rs` → keep as thin entry (`mod` declarations, `pub use`).
- Create: `apps/dronecaster/eno/src/` modules chosen by responsibility. Suggested: `tuning.rs` (tunable constants: `OUT_TRIM`, `REVERB_MIX`, `KLANK_*`, `INTRO_*`, `HPF_HZ`, `PIANO_LIVE_S`, ...), `music.rs` (`AIRPORTS`, `PLANES`, `NOTE_RATES`, tuning ratios), `pad.rs` (`PadVoice`), `voice.rs` or `engine.rs` (`Eno`, `build`, `control`, `note_control`, the `Voice` impl). Final split is the implementer's call; each file under about 250 lines.

**Interfaces:**
- Produces: the public API of Task 5 unchanged (`dronecaster_eno::{Eno, P_CONTROL, P_PAD, P_MOOG, P_KLANK, P_PIANO, P_PERC, P_REVERB, P_FINAL, ...}` re-exported at the crate root).
- Consumes: nothing new.

- [ ] **Step 1: Split without changing any function body.** Use `pub(crate)` for items shared between modules. Do not reorder struct fields, change loop structure or add `#[inline]` attributes.
- [ ] **Step 2: Verify.** Clippy zero; `cargo test --workspace` green; `sh $GOLDEN/check.sh` OK x4 (this covers the 8 s idle cut-off); test list equals `$GOLDEN/tests.txt`; firmware check as before.
- [ ] **Step 3: Commit.** `refactor(eno): split into modules`.

---

### Task 8: Split `dispatcher.rs`

Риск: высокий — диспетчер переключает аудио-апплеты между потоками; нужны сохранить `stress_handoff_never_overlaps` и alloc-проверки.

**Files:**
- Modify: `rust/sdk/os/src/dispatcher.rs` → `sdk/os/src/dispatcher/mod.rs` (types, `Dispatcher`, `AudioPath`, constants) and `sdk/os/src/dispatcher/tests.rs` (everything under `#[cfg(test)] mod tests`, about 380 lines). If the production code (about 240 lines) reads better in two files (for example `shared.rs` for `Shared` and the state constants), split it; otherwise stop at mod + tests.

**Interfaces:**
- Produces: `orbiversum_os::{Dispatcher, AudioPath, DEFAULT_SAMPLE_RATE}` unchanged.

- [ ] **Step 1: Split, no body changes.**
- [ ] **Step 2: Verify.** As Task 7 Step 2, plus `cargo test -p orbiversum-os` runs `stress_handoff_never_overlaps`, `switch_frees_old_applet_memory` and `audio_independent_of_block_size`.
- [ ] **Step 3: Commit.** `refactor(os): split dispatcher tests out`.

---

### Task 9: Document `orbiversum-core` blocks

Риск: обычный — только комментарии и `#![warn(missing_docs)]`.

**Files:**
- Modify: every file in `rust/sdk/core/src/` (`fast, osc, moog, biquad, resonant, comb, delay, reverb, env, lfo, noise, rng, pan, seq, voice, prof, lib`).
- Create: `rust/sdk/core/README.md`.

- [ ] **Step 1: Add `#![warn(missing_docs)]` to `sdk/core/src/lib.rs`** and see the missing-docs list.
- [ ] **Step 2: Write docs from the code.** Each module: `//!` overview with what the block does, state it keeps, whether it is audio-rate or control-rate, whether it allocates (only at construction) and its cost class. Each public item: `///` with units (Hz, seconds, samples, linear gain), valid ranges, and what happens outside them (clamped, wrapped, undefined). Filters and oscillators state the algorithm and formula the code implements (read the code, do not invent a reference; cite a paper/book only if a comment or the git log names it). Where code ports SuperCollider UGens (`EnvPerc` follows `Env.perc`), say so. A short doc example (`/// ```) is welcome where a block is easy to drive and the test compiles under `no_std`-safe host tests; otherwise skip.
- [ ] **Step 3: README.** Block catalogue table (module, one line, rate, cost) and a minimal usage example of building a `Voice`.
- [ ] **Step 4: Verify.** `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p orbiversum-core` is clean; clippy zero (missing_docs counts under `-D warnings`); tests and `sh $GOLDEN/check.sh` unchanged.
- [ ] **Step 5: Commit.** `docs(core): document blocks`.

---

### Task 10: Document the Dronecaster apps

Риск: обычный — комментарии.

**Files:**
- Modify: `rust/apps/dronecaster/eno/src/*`, `rust/apps/dronecaster/starlids/src/lib.rs`
- Create: `rust/apps/dronecaster/README.md`

- [ ] **Step 1: `#![warn(missing_docs)]` on both app crates.**
- [ ] **Step 2: Fix the stale Eno header.** It still says "milestone 1: pad, sub sines, tanh, Moog" although Klank, piano string, perc and reverb exist. Rewrite the crate-level `//!` to describe the actual signal flow: sections `P_CONTROL … P_FINAL`, the 8-voice pad in two groups of four sharing a saw, the control rate `CTRL = 32` at fixed absolute sample positions (block-size independence), the piano string loop that is skipped 8 s after the last note (`PIANO_LIVE_S`), `PAD_CTRL_STRIDE`, reverb size. Every tunable constant in `tuning.rs` gets a doc line with unit, meaning and effect on sound or CPU. Document the CPU budget using the measurements in `docs/superpowers/specs/2026-11-12-eno-applet-design.md` ("Eno board measurements"); copy numbers verbatim with their conditions.
- [ ] **Step 3: Starlids** (sparse today): document algorithm, state, parameters `hz`/`amp` meaning, and the reference-implementation test (`starlids_ref`) and what it proves.
- [ ] **Step 4: README** for the group: what Dronecaster is (ports of the SuperCollider drones), where the originals come from (name only what the repo already states, see `THIRD-PARTY.md`), how to render a WAV: `cargo run --release -p render-wav -- --drone eno --seconds 30 --out eno.wav`.
- [ ] **Step 5: Verify.** `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p dronecaster-eno -p dronecaster-starlids` clean; clippy zero; goldens OK.
- [ ] **Step 6: Commit.** `docs(dronecaster): document Eno and Starlids`.

---

### Task 11: Document `orbiversum-os` and the audio path

Риск: обычный — комментарии.

**Files:**
- Modify: `rust/sdk/os/src/*` (`applet, board, params, dispatcher/, voice_applet, param_ui, lib`), `rust/boards/jc3636k718c/hal/src/audio.rs` and the other hal files' `//!` headers.

- [ ] **Step 1: `#![warn(missing_docs)]` on `orbiversum-os`.**
- [ ] **Step 2: Dispatcher docs.** Explain the audio/UI split: `Dispatcher` (UI thread) vs `AudioPath` (audio thread), the `Shared` atomic state machine `RUNNING → PARKED → READY`, the fade on applet switch (`DEFAULT_FADE` = 20 ms at 48 kHz), what is allowed on the audio thread (no allocation, no locks; old applet memory is freed on the UI side via `service`). Document `Params` as the lock-free channel between UI and voice.
- [ ] **Step 3: Audio output docs.** `hal/src/audio.rs`: the I2S/codec path from `AudioOut`, sample format, block size, who calls `AudioPath::audio`, latency, and which thread/core it runs on, read from the code.
- [ ] **Step 4: Verify.** `cargo doc` with `-D warnings` for `orbiversum-os` and the hal clean; clippy zero; tests green.
- [ ] **Step 5: Commit.** `docs(os): document dispatcher and audio path`.

---

### Task 12: Repository docs and final verification

Риск: обычный — документация и проверка.

**Files:**
- Create: `/Users/noise/code/esp32-round-base/README.md`, `/Users/noise/code/esp32-round-base/docs/audio.md`, `rust/README.md`
- Modify: `docs/superpowers/specs/2026-11-12-*.md` and `plans/2026-11-12-*.md` — add one line at the top of each: `> Historical document. Paths predate the sdk/apps/boards layout; see README.md.`

- [ ] **Step 1: Root `README.md`.** What Orbiversum is (a round-display ESP32-S3 drone/synth platform), the layout tree from the spec, quick start: host tests (`cd rust && cargo test --workspace`), render a WAV, build the firmware (the exact command verified in Task 6), board support status (JC3636K718C), where to start for a new app or a new board, license pointer (`LICENSE`, `THIRD-PARTY.md`).
- [ ] **Step 2: `docs/audio.md`.** Sections: sample rate and block size; signal path (voice → applet → dispatcher → I2S) as a small text diagram; control rate vs audio rate (Eno `CTRL = 32`) and why output is block-size independent; UI↔audio parameter flow (`Params`); real-time rules and how `no_alloc` and the dispatcher tests enforce them; CPU budget and measurements (from the Eno board measurements); how to add a voice (implement `Voice`, add to `orbiversum-apps::all`). Every number comes from the code or the measurements doc, with the file named.
- [ ] **Step 3: `rust/README.md`.** One page: the workspace members, the firmware being a separate workspace and why, lints.
- [ ] **Step 4: Final verification.** Run and record: `cargo clippy --workspace --all-targets -- -D warnings` (zero); firmware clippy/build as in Task 6; `cargo test --workspace`; `sh $GOLDEN/check.sh` (OK x4); sorted test list equals `$GOLDEN/tests.txt`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` clean; `git status --short` lists only the owner's pre-existing files; `git log --oneline scaffold..HEAD` shows one commit per task.
- [ ] **Step 5: Commit.** `docs: README, audio guide, historical notes`.
