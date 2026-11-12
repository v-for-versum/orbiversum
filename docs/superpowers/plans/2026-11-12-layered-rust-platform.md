# Слоёная Rust-платформа: первый срез. План реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Starlids и Sine звучат из Rust-кода на Mac (WAV) и на плате JC3636K718C, апплеты переключаются из меню на круглом экране.

**Architecture:** Cargo workspace в `rust/`. Хост-крейты (`blocks`, `dsp`, `os`, `applets`) собираются и тестируются на Mac. Крейт `board-jc3636k718c` и бинарник `firmware-jc3636k718c` собираются под `xtensa-esp32s3` через `esp-idf-svc` и вызывают существующий C BSP (`components/bsp_jc3636k718c`) по FFI.

**Tech Stack:** Rust (stable для хоста, `esp` toolchain через `espup` для платы), `esp-idf-svc`, `embedded-graphics`, `hound` (WAV на хосте), ESP-IDF 6.1.0, существующий C BSP.

**Spec:** `docs/superpowers/specs/2026-11-12-layered-rust-platform-design.md`

## Global Constraints

- Частота дискретизации 48000 Гц сквозная; блок аудио — стерео `[f32; 2]`.
- `blocks` и `dsp`: `#![no_std]` (допустим `alloc`), без зависимости от `os`, платы и графики.
- В аудио-пути (`Voice::process`, `Applet::audio`) нет аллокаций, блокировок и ввода-вывода; вся память выделяется при создании.
- Зависимости только вниз: firmware → applets → os/dsp → blocks; `board-*` реализует трейты из `os`.
- Аудио на ядре 1 с высоким приоритетом; UI, ввод и отправка кадра на ядре 0.
- Апплет рисует через `embedded-graphics` (`Rgb565`, 360×360) и сообщает изменённые области.
- Расширение только на этапе сборки (крейты в workspace); динамическая загрузка и скрипты вне объёма.
- Тест-тон и дроны не должны давать щелчков при старте, остановке и переключении (плавная огибающая).

## Review Focus

- Нулевые и крайние значения `hz` (0, ниже 20 Гц, около Найквиста): нет NaN, Inf и аварийного выхода — тишина или ограниченный сигнал. Тест в Task 4.
- Выход дрона после долгой работы (минуты) не растёт и не уходит в NaN (дрейф фильтров, накопление фазы). Тест в Task 4.
- Одинаковый seed даёт побитово одинаковый выход, разные seed — разный. Тест в Task 4.
- Переключение апплета посреди воспроизведения: нет щелчка (скачок > порога между соседними сэмплами) и нет утечки памяти. Тест в Task 8.
- Размер блока аудио не равен 128 (1, 7, 1024): результат не зависит от разбиения на блоки. Тест в Task 3 и Task 4.

---

### Task 1: Workspace и каркас хост-крейтов

Риск: обычный — конфиги и пустые крейты.

**Files:**
- Create: `rust/Cargo.toml` (workspace, members: `crates/blocks`, `crates/dsp`, `crates/os`, `crates/applets`, `tools/render-wav`; `board-*` и `firmware-*` не входят в `default-members`)
- Create: `rust/crates/{blocks,dsp,os,applets}/Cargo.toml`, `src/lib.rs`
- Create: `rust/rust-toolchain.toml` (stable для хоста; для платы переопределяется в firmware-крейте)
- Modify: `.gitignore` (добавить `rust/target/`)

**Interfaces:**
- Produces: workspace, в котором `cargo test --workspace` на Mac проходит; `blocks` и `dsp` помечены `#![no_std]`.

- [ ] **Step 1:** Создать workspace и пустые крейты; в `blocks` и `dsp` `#![no_std]` с `extern crate alloc`.
- [ ] **Step 2:** Тест-заглушка в каждом крейте (`it_builds`), чтобы `cargo test` реально запускал тесты.
- [ ] **Step 3:** Run: `cd rust && cargo test --workspace` — Expected: PASS, все крейты собраны.
- [ ] **Step 4:** Commit `chore: rust workspace skeleton`.

### Task 2: blocks — осцилляторы, шумы и медленные LFO

Риск: обычный — чистая математика с тестами; проверка реального времени в Task 4.

**Files:**
- Create: `rust/crates/blocks/src/{osc.rs,lfo.rs,rng.rs}`; тесты в тех же файлах (`#[cfg(test)]`)

**Interfaces:**
- Produces:
  - `rng::Rng::new(seed: u64) -> Rng`, `Rng::f32(&mut self) -> f32` (0..1), `Rng::range(&mut self, lo: f32, hi: f32) -> f32` (детерминированный, без std)
  - `osc::SawDpw::new(sr: f32) -> SawDpw`, `SawDpw::tick(&mut self, hz: f32) -> f32` (−1..1)
  - `osc::Pulse::new(sr: f32)`, `Pulse::tick(&mut self, hz: f32, width: f32) -> f32`
  - `osc::SinOsc::new(sr: f32, phase: f32)`, `SinOsc::tick(&mut self, hz: f32) -> f32`
  - `osc::LfTri::new(sr: f32, phase: f32)`, `LfTri::tick(&mut self, hz: f32) -> f32`
  - `lfo::Noise0::new(rng: Rng, sr: f32)`, `Noise0::tick(&mut self, hz: f32) -> f32` (ступенчатый шум, как `LFNoise0`)
  - `lfo::Noise1::new(rng: Rng, sr: f32)`, `Noise1::tick(&mut self, hz: f32) -> f32` (интерполированный, как `LFNoise1`)
  - `lfo::VarLagSine::new(sr: f32)`, `VarLagSine::tick(&mut self, target: f32, time_s: f32) -> f32` (сглаживание с синусной формой, как `VarLag(warp: \sine)`)

- [ ] **Step 1:** Тесты: `saw_dpw_stays_in_range` (|x| ≤ 1 за 1 с на 55, 440, 5000 Гц), `saw_dpw_period` (440 Гц — 109 сэмплов ±1 между переходами через нуль), `pulse_width_changes_duty`, `rng_is_deterministic_per_seed`, `noise0_holds_between_steps` (значение меняется ~hz раз в секунду), `noise1_is_continuous` (соседние сэмплы отличаются меньше порога), `varlag_reaches_target` (после `time_s` отличие < 1%).
- [ ] **Step 2:** Run: `cargo test -p blocks` — Expected: FAIL (типы не определены).
- [ ] **Step 3:** Реализовать типы по сигнатурам; `SawDpw` — полиномиальный (DPW) вариант пилы.
- [ ] **Step 4:** Run: `cargo test -p blocks` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(blocks): oscillators, noise and slow LFOs`.

### Task 3: blocks — фильтры, задержка, панорама

Риск: обычный — математика с тестами.

**Files:**
- Create: `rust/crates/blocks/src/{biquad.rs,moog.rs,delay.rs,pan.rs}`

**Interfaces:**
- Consumes: ничего из Task 2.
- Produces:
  - `biquad::Biquad::lowpass(sr: f32, hz: f32, q: f32) -> Biquad`, `Biquad::highpass(...)`, `Biquad::set_lowpass(&mut self, hz: f32)` (пересчёт коэффициентов без аллокаций), `Biquad::tick(&mut self, x: f32) -> f32`
  - `moog::MoogLadder::new(sr: f32)`, `MoogLadder::tick(&mut self, x: f32, cutoff_hz: f32, res: f32) -> f32`
  - `delay::DelayC::new(sr: f32, max_s: f32) -> DelayC`, `DelayC::tick(&mut self, x: f32, delay_s: f32) -> f32` (кубическая интерполяция; буфер создаётся в `new`)
  - `pan::pan2(x: f32, pos: f32) -> [f32; 2]` (равномощная панорама, `pos` −1..1)

- [ ] **Step 1:** Тесты: `lowpass_attenuates_above_cutoff` (синус на 10× среза ослаблен > 20 дБ), `highpass_blocks_dc`, `ladder_stable_at_high_resonance` (на импульс отклик затухает, нет NaN), `delay_returns_input_after_n_samples` (целочисленная задержка, точное совпадение), `delay_fractional_is_smooth`, `pan2_equal_power` (L²+R² ≈ 1 при любом `pos`), `block_size_independence` (обработка по 1/7/1024 сэмплов даёт тот же результат, что одним куском).
- [ ] **Step 2:** Run: `cargo test -p blocks` — Expected: FAIL.
- [ ] **Step 3:** Реализовать по сигнатурам; биквады по формулам RBJ cookbook; ладдер — модель Хювиляйнена (приближение `MoogLadder` из SuperCollider, не побитовая копия).
- [ ] **Step 4:** Run: `cargo test -p blocks` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(blocks): filters, delay line, pan`.

### Task 4: dsp — дроны Starlids и Sine

Риск: высокий — аудио-путь реального времени: инвариант «без аллокаций», численная устойчивость при долгой работе.

**Files:**
- Create: `rust/crates/dsp/src/{lib.rs,voice.rs,starlids.rs,sine.rs}`
- Reference: `~/code/github/northern-information/dronecaster/engine/drones/Starlids.scd` (граф 1:1: подставка Pulse на октаву ниже с шириной 0.2–0.8 от LFTri 0.5 Гц; 12 голосов SawDPW с интервалами {0,4,5,9} полутонов, LPF 20–12000 Гц, DelayC ≈ 1.3 мс ± 0.7 мс, свой Pan2; затем `tanh`, MoogLadder на 8×hz…60×hz от медленного шума, HPF 20 Гц)

**Interfaces:**
- Consumes: типы из Task 2 и Task 3.
- Produces:
  - `voice::Voice` (trait): `fn set_sample_rate(&mut self, sr: f32)`, `fn set_params(&mut self, hz: f32, amp: f32)`, `fn process(&mut self, out: &mut [[f32; 2]])`
  - `starlids::Starlids::new(seed: u64, sr: f32) -> Starlids` (`impl Voice`; все буферы выделены в `new`)
  - `sine::Sine::new(sr: f32) -> Sine` (`impl Voice`)

- [ ] **Step 1:** Тесты: `starlids_is_deterministic_per_seed`; `starlids_seeds_differ`; `starlids_output_is_finite_and_bounded` (10 минут при 55, 110, 440 Гц: |x| ≤ 1, без NaN); `starlids_handles_extreme_hz` (hz = 0, 1, 20000, 24000: конечный ограниченный выход); `starlids_block_size_independence` (блоки 1/7/128/1024 дают совпадающий результат); `starlids_no_alloc_in_process` (счётчик аллокаций через тестовый глобальный аллокатор: 0 за `process`); `sine_matches_reference_frequency` (FFT-пик ±1 бин на 440 Гц).
- [ ] **Step 2:** Run: `cargo test -p dsp` — Expected: FAIL.
- [ ] **Step 3:** Реализовать `Starlids` и `Sine` по сигнатурам; `rrand` из оригинала заменяется `Rng`, значения берутся один раз в `new`; медленные LFO считаются раз в блок.
- [ ] **Step 4:** Run: `cargo test -p dsp --release` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(dsp): Starlids and Sine voices`.

### Task 5: Хостовый рендер в WAV и сравнение с norns

Риск: обычный — утилита и ручная проверка.

**Files:**
- Create: `rust/tools/render-wav/{Cargo.toml,src/main.rs}`

**Interfaces:**
- Consumes: `dsp::Starlids`, `dsp::Sine`, `dsp::Voice`.
- Produces: CLI `render-wav --drone <starlids|sine> --hz <f32> --amp <f32> --seconds <f32> --seed <u64> --out <path.wav>` (WAV 48 кГц, стерео, 16 бит).

- [ ] **Step 1:** Тест: `cli_writes_valid_wav` (запуск на 1 с, чтение `hound`, проверка 48000 Гц, 2 канала, ~48000 кадров, не тишина).
- [ ] **Step 2:** Run: `cargo test -p render-wav` — Expected: FAIL.
- [ ] **Step 3:** Реализовать CLI; рендер блоками по 128 сэмплов.
- [ ] **Step 4:** Run: `cargo run -p render-wav -- --drone starlids --hz 110 --amp 0.5 --seconds 30 --seed 1 --out /tmp/starlids.wav` — Expected: файл создан.
- [ ] **Step 5:** Ручная проверка: послушать файл рядом с оригиналом на norns (или с записью оригинала); зафиксировать в сообщении коммита, что расходится на слух (ожидается: ладдер, детали хоруса). Если звук узнаваемо не похож, остановиться и разобрать расхождение до Task 6.
- [ ] **Step 6:** Commit `feat(tools): render-wav for host-side listening`.

### Task 6: Спайк — Rust под Xtensa и FFI к C BSP

Риск: высокий — проверка сборки и линковки не измерена; цель — снять главный риск спеки.

**Files:**
- Create: `rust/firmware-jc3636k718c/{Cargo.toml,rust-toolchain.toml,build.rs,sdkconfig.defaults,src/main.rs}`
- Modify: `docs/superpowers/specs/2026-11-12-layered-rust-platform-design.md` (раздел «Риски»: результат спайка)

**Interfaces:**
- Produces: бинарник, который собирается под `xtensa-esp32s3-espidf`, подключает компоненты `bsp_jc3636k718c` (через `extra_components` в `[package.metadata.esp-idf-sys]`), вызывает `bsp_init` и пишет в лог строку `rust: bsp ok`; `unsafe extern "C"` объявления для `bsp_init`, `bsp_audio_out_init`, `bsp_audio_out_write`.

- [ ] **Step 1:** Установить тулчейн: `cargo install espup espflash ldproxy && espup install`. Записать версии.
- [ ] **Step 2:** Собрать пустой `esp-idf-svc` проект под `esp32s3` с ESP-IDF 6.1.0 (если `esp-idf-sys` пока не поддерживает 6.1.0, зафиксировать, какая версия IDF нужна, и решить с владельцем платформы: обновить `esp-idf-sys` или собирать на поддерживаемой версии).
- [ ] **Step 3:** Подключить C BSP как внешний компонент, использовать `sdkconfig.jc3636k718c` из корня репозитория; собрать.
- [ ] **Step 4:** Прошить и проверить по монитору: `rust: bsp ok`, PSRAM определён, нет перезагрузок.
- [ ] **Step 5:** Записать результат спайка в спеку (что собралось, какие версии, что не получилось). Если путь A непроходим, остановиться и вернуться к выбору слоя hw в спеке.
- [ ] **Step 6:** Commit `spike: rust firmware links the C BSP`.

### Task 7: board-jc3636k718c — аудио-выход

Риск: высокий — realtime-аудио и потоки на двух ядрах.

**Files:**
- Create: `rust/crates/board-jc3636k718c/{Cargo.toml,src/{lib.rs,audio.rs,ffi.rs}}`
- Modify: `rust/firmware-jc3636k718c/src/main.rs` (тест-тон 440 Гц)

**Interfaces:**
- Consumes: `os::Board`/`os::AudioOut` из Task 8, поэтому до Task 8 здесь свой минимальный `AudioOut` (`fn start(&mut self, sr: u32, render: Box<dyn FnMut(&mut [[f32; 2]]) + Send>)`), который Task 8 заменит на трейт из `os`.
- Produces: `audio::JcAudio::new() -> Result<JcAudio, BoardError>`; задача аудио закреплена на ядре 1 с высоким приоритетом, вызывает `render` блоками по 128 кадров и пишет через `bsp_audio_out_write` (f32 → i16 с клипом, без аллокаций в цикле).

- [ ] **Step 1:** Тест на хосте для чистой части: `f32_to_i16_clips_and_scales` (±1.0 → ±32767, >1 клипуется, NaN → 0).
- [ ] **Step 2:** Run: `cargo test -p board-jc3636k718c --lib` (преобразования вынесены в модуль без FFI) — Expected: FAIL.
- [ ] **Step 3:** Реализовать преобразование и `JcAudio`; подставить `Sine` из `dsp`.
- [ ] **Step 4:** Прошить: слушать 440 Гц минуту; Expected: чистый тон, без щелчков. Замерить загрузку ядра 1 (`vTaskGetRunTimeStats` или счётчик циклов) и записать.
- [ ] **Step 5:** Commit `feat(board): audio out on core 1`.

### Task 8: os — Applet, Board, диспетчер

Риск: высокий — конкурентность между аудио и UI, переключение апплетов.

**Files:**
- Create: `rust/crates/os/src/{lib.rs,applet.rs,board.rs,dispatcher.rs,params.rs}`

**Interfaces:**
- Consumes: `dsp::Voice`.
- Produces (первая версия; уточняется по итогам среза, Task 11):
  - `Applet` (trait): `info(&self) -> AppletInfo`, `start(&mut self)`, `audio(&mut self, out: &mut [[f32; 2]])`, `input(&mut self, ev: InputEvent)`, `draw(&mut self, d: &mut dyn DrawTarget<Color = Rgb565, Error = core::convert::Infallible>, dirty: &mut Dirty)`, `stop(&mut self)`
  - `Board` (trait): `poll_input(&mut self) -> Option<InputEvent>`, `audio_out(&mut self) -> &mut dyn AudioOut`, `display(&mut self) -> &mut dyn Display`
  - `InputEvent` (enum: `Knob(i32)`, `Tap{x:i16,y:i16}`, `Press`), `Dirty` (список до 8 прямоугольников, переполнение превращается в «весь экран»)
  - `Dispatcher::new(applets: Vec<Box<dyn Applet + Send>>) -> Dispatcher`, `select(&mut self, idx: usize)`, `audio(&mut self, out)`, `params` — общий атомарный `Params { hz: AtomicU32, amp: AtomicU32 }`
  - Переключение: `select` запускает плавное затухание (~20 мс) старого апплета, затем `stop` и `start` нового с нарастанием.

- [ ] **Step 1:** Тесты с мок-апплетом: `switch_does_not_click` (максимальный скачок между соседними сэмплами при переключении ниже порога), `switch_frees_old_applet_memory` (счётчик аллокаций возвращается к исходному), `audio_independent_of_block_size`, `dirty_overflow_becomes_full_screen`, `select_out_of_range_is_ignored`.
- [ ] **Step 2:** Run: `cargo test -p os` — Expected: FAIL.
- [ ] **Step 3:** Реализовать диспетчер; параметры между аудио и UI — атомики, без мьютексов в аудио-пути.
- [ ] **Step 4:** Run: `cargo test -p os` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(os): applet and board traits, dispatcher`.

### Task 9: applets — обёртки Starlids и Sine

Риск: обычный — тонкие обёртки над `Voice` с тестами.

**Files:**
- Create: `rust/crates/applets/src/{lib.rs,voice_applet.rs,starlids.rs,sine.rs}`

**Interfaces:**
- Consumes: `os::Applet`, `dsp::Voice`, `os::Params`.
- Produces: `applets::all(seed: u64, sr: f32) -> Vec<Box<dyn os::Applet + Send>>` (Starlids, Sine); энкодер меняет `hz` (по шагам в полутонах), тап по экрану меняет `amp`; `draw` рисует название и значения `hz`/`amp` и добавляет в `dirty` только изменённые строки.

- [ ] **Step 1:** Тесты: `knob_moves_hz_by_semitone`, `amp_is_clamped_0_to_1`, `draw_marks_only_changed_regions` (после одного поворота энкодера `dirty` покрывает строку `hz`, а не весь экран).
- [ ] **Step 2:** Run: `cargo test -p applets` — Expected: FAIL.
- [ ] **Step 3:** Реализовать `VoiceApplet<V: Voice>` и две конкретные конструкции.
- [ ] **Step 4:** Run: `cargo test -p applets` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(applets): Starlids and Sine applets`.

### Task 10: board — дисплей, ввод, меню и сборка прошивки

Риск: высокий — потоки на двух ядрах, DMA, общий дисплей.

**Files:**
- Create: `rust/crates/board-jc3636k718c/src/{display.rs,input.rs}`
- Modify: `components/bsp_jc3636k718c/include/bsp/bsp_display.h` и `src/bsp.c` — **только если** BSP не отдаёт сырую запись региона: сейчас публичный API только LVGL (`bsp_display_get_lvgl_display`), поэтому нужна C-функция `esp_err_t bsp_display_draw_rect(int x, int y, int w, int h, const uint16_t *rgb565)` поверх `esp_lcd` (проверить, есть ли панель-хендл в `bsp.c`)
- Modify: `rust/firmware-jc3636k718c/src/main.rs` (меню, запуск двух задач по ядрам)

**Interfaces:**
- Consumes: `os::{Board, Dispatcher}`, `applets::all`, `bsp_encoder_get_steps`, `bsp_touch_contact`.
- Produces: `display::JcDisplay` (`DrawTarget<Color = Rgb565>`, 360×360, отправка только `dirty` областей полосами из внутренней RAM), `input::JcInput` (энкодер и тач → `InputEvent`), меню: короткое нажатие энкодера открывает список апплетов, поворот выбирает, нажатие подтверждает.

- [ ] **Step 1:** Тест на хосте: `dirty_rects_are_clipped_to_screen` и `strip_splitting_covers_rect_exactly` (разбиение области на полосы без пропусков и перекрытий).
- [ ] **Step 2:** Run: `cargo test -p board-jc3636k718c --lib` — Expected: FAIL.
- [ ] **Step 3:** Реализовать разбиение на полосы, `JcDisplay`, `JcInput`; при необходимости добавить `bsp_display_draw_rect` в C BSP и отдельный коммит.
- [ ] **Step 4:** Собрать прошивку, прошить, проверить вручную: меню появляется, оба апплета переключаются, звук не обрывается, энкодер и тап работают.
- [ ] **Step 5:** Commit `feat(firmware): display, input and applet menu`.

### Task 11: Замеры и фиксация интерфейсов

Риск: обычный — измерения и документация.

**Files:**
- Modify: `docs/superpowers/specs/2026-11-12-layered-rust-platform-design.md` (раздел «Контракты» — реальные сигнатуры `Applet`/`Board`; раздел «Риски» — результаты замеров)

- [ ] **Step 1:** Замерить на плате: загрузку ядра 1 при Starlids (доля времени на блок 128 при 48 кГц), время кадра для частичной и полной отрисовки, максимальную задержку отклика энкодера. Записать числа.
- [ ] **Step 2:** Сверить критерии спеки (Starlids узнаваем; нет щелчков; меню без заметных лагов) и отметить выполненные.
- [ ] **Step 3:** Обновить спеку реальными сигнатурами и числами; перечислить, что менять перед этапами 2–3 портирования (запас по CPU определяет, сколько тяжёлых дронов реально).
- [ ] **Step 4:** Commit `docs: first slice results and real interfaces`.
