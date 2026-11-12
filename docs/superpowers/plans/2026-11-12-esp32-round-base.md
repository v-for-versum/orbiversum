# esp32-round-base Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Собрать в `~/code/esp32-round-base` каркас на ESP-IDF для платы Guition JC3636K718C: слой железа из дрон-синтезатора и пустой модуль `synth`, который играет тест-тон 440 Гц.

**Architecture:** Копия `~/code/obod-firmware-main` с вычисткой дрона. Звуковая часть (`synth`: очереди событий, генератор, `synth_render`) написана на чистом C без ESP-IDF и проверяется на хосте. ESP-IDF-обвязка (аудио-задача, MIDI, LVGL) тонкая. Исходный репозиторий не меняется.

**Tech Stack:** ESP-IDF 6.1.0 (esp32s3), FreeRTOS, LVGL 9.x, TinyUSB MIDI, C11, host-тесты на `cc` + `assert.h`.

**Spec:** `docs/superpowers/specs/2026-11-12-esp32-round-base-design.md`

## Global Constraints

- Источник копирования: `/Users/noise/code/obod-firmware-main` (только чтение).
- Target `esp32s3`, ESP-IDF 6.1.0. Проект `esp32_round_base`, версия 0.1.0.
- Аудио: 48 кГц, 16 бит, стерео, блок 128 кадров. Другая частота недопустима.
- В `synth_render` нет malloc, мьютексов, логов, вызовов LVGL и блокирующих вызовов.
- Очереди событий не блокируют вызывающего: при переполнении событие отбрасывается.
- У каждой очереди ровно один производитель и один потребитель (SPSC).
- Остаются `partitions.csv` (ota_0/ota_1 по 4 МБ) и стандартный загрузчик ESP-IDF без правок.
- Лицензия MIT, авторство Annabiosis 2026 сохраняется в `LICENSE`.
- Микрофон (PDM) по умолчанию выключен (`audio_in = false`).
- Ничего не меняется в `/Users/noise/code/obod-firmware-main`.

## Review Focus

1. Поток MIDI-такта (`CLOCK`, 24 раза за четверть) переполняет очередь: вызывающий не блокируется, старые события не портятся, счётчик `dropped` растёт (Task 2, Task 3).
2. NOTE ON с velocity 0 означает NOTE OFF; байты данных больше 127 и чужие сообщения (CC, active sensing, reset) не создают событий (Task 4).
3. `synth_render` с `frames = 0`, нечётными размерами блока и вызов до `synth_init` не падают, фаза не рвётся между блоками (Task 3).
4. Плата без аудио или с ошибкой `bsp_audio_out_init`: загрузка идёт дальше, аудио-задача не стартует, паники нет (Task 7).
5. Первая загрузка с пустым NVS: настройки получают значения по умолчанию, `MIDI_IN` из NVS управляет запуском MIDI (Task 5, Task 7).

---

### Task 1: Каркас репозитория

Риск: обычный — копирование и чистка файлов, кода синтеза нет.

**Files:**
- Create: `/Users/noise/code/esp32-round-base/{CMakeLists.txt,sdkconfig.defaults,partitions.csv,dependencies.lock,LICENSE,THIRD-PARTY.md,.editorconfig,.gitignore}`, `components/bsp_jc3636k718c/`, `components/app_net/`, `main/main.c`, `main/CMakeLists.txt`, `tools/`, `web/`
- Modify: `CMakeLists.txt`

**Interfaces:**
- Produces: проект, который собирается с `main` + `bsp_jc3636k718c` + `app_net`; корень репозитория для остальных задач.

- [ ] **Step 1: Скопировать из источника** `components/bsp_jc3636k718c`, `components/app_net`, `partitions.csv`, `sdkconfig.defaults`, `dependencies.lock`, `LICENSE`, `THIRD-PARTY.md`, `.editorconfig`, `CMakeLists.txt`, а также `tools/{idf.sh,idf.ps1,_idf_env.ps1,serial_log.py,midi-send.py,upload_fs.sh,upload_fs.ps1,webflash}` и `web/flash.html` с тем, на что она ссылается. Команда: `cp -R`, без `tools/__pycache__`.
- [ ] **Step 2: Правка корневого `CMakeLists.txt`:** `project(esp32_round_base VERSION 0.1.0)`, комментарий о версии оставить по смыслу, шапку переписать под каркас.
- [ ] **Step 3: Минимальный `main/main.c`:** `app_main` вызывает `bsp_init(&features)` с `BSP_FEATURES_ALL()`, `bsp_print_info()`, затем `vTaskDelay(portMAX_DELAY)` в цикле. `main/CMakeLists.txt`: `SRCS main.c`, `REQUIRES bsp_jc3636k718c`.
- [ ] **Step 4: `.gitignore`** (`build/`, `sdkconfig`, `managed_components/`, `__pycache__/`), `git init`, первый коммит вместе с `docs/`.
- [ ] **Step 5: Собрать.** Run: `. $IDF_PATH/export.sh && idf.py build`. Expected: `Project build complete`, предупреждений о неизвестных компонентах нет.
- [ ] **Step 6: Commit** `chore: scaffold from obod-firmware (bsp, app_net)`.

---

### Task 2: Очередь событий (SPSC)

Риск: высокий — конкурентный доступ из разных задач, realtime-аудио.

**Files:**
- Create: `components/synth/CMakeLists.txt` (пока без регистрации в `main`), `components/synth/include/synth.h` (только типы), `components/synth/src/synth_queue.h`, `components/synth/src/synth_queue.c`, `test/host/Makefile`, `test/host/test_synth_queue.c`

**Interfaces:**
- Produces в `synth.h`:
  - `typedef enum { SYNTH_MIDI_NOTE_ON, SYNTH_MIDI_NOTE_OFF, SYNTH_MIDI_CLOCK, SYNTH_MIDI_START, SYNTH_MIDI_CONTINUE, SYNTH_MIDI_STOP } synth_midi_kind_t;`
  - `typedef struct { synth_midi_kind_t kind; uint8_t channel; uint8_t note; float velocity; } synth_midi_event_t;` (`channel` 0..15, `velocity` 0..1)
  - `typedef enum { SYNTH_INPUT_ENCODER, SYNTH_INPUT_TOUCH } synth_input_kind_t;`
  - `typedef struct { synth_input_kind_t kind; int32_t a; int32_t b; } synth_input_event_t;` (энкодер: `a` = знаковая дельта; тач: `a`,`b` = x,y)
- Produces в `synth_queue.h`:
  - `#define SYNTH_QUEUE_CAPACITY 256` (степень двойки)
  - `typedef struct synth_queue synth_queue_t;` на C11-атомиках, элементы `synth_event_t { enum { SYNTH_EV_MIDI, SYNTH_EV_INPUT } type; union { synth_midi_event_t midi; synth_input_event_t input; } u; }`
  - `void synth_queue_init(synth_queue_t *q);`
  - `bool synth_queue_push(synth_queue_t *q, const synth_event_t *ev);` (производитель; `false` и счётчик `dropped` при полной очереди)
  - `bool synth_queue_pop(synth_queue_t *q, synth_event_t *out);` (потребитель)
  - `uint32_t synth_queue_dropped(const synth_queue_t *q);`

- [ ] **Step 1: `test/host/Makefile`:** цель `test` собирает и запускает все `test_*.c` через `cc -std=c11 -Wall -Wextra -Werror -pthread -I../../components/synth/include -I../../components/synth/src`, цель `tsan` то же с `-fsanitize=thread`. Каждый тест печатает `ok <name>`, сборка завершается кодом 0.
- [ ] **Step 2: Написать тесты в `test_synth_queue.c`:** `test_pop_empty_returns_false`; `test_fifo_order` (3 события, порядок сохранён); `test_wraparound` (10 × `SYNTH_QUEUE_CAPACITY` циклов push/pop, порядок сохранён); `test_full_drops_new_keeps_old` (256 успешных push, 257-й возвращает `false`, `dropped == 1`, все 256 прочитаны без искажений); `test_spsc_threads` (производитель шлёт 100000 последовательных номеров с повтором при полной очереди, потребитель проверяет порядок и отсутствие потерь).
- [ ] **Step 3: Run `make -C test/host test`.** Expected: FAIL (сборка падает, `synth_queue.h` не найден).
- [ ] **Step 4: Реализовать `synth_queue.c`:** кольцо из `SYNTH_QUEUE_CAPACITY` элементов, индексы `head`/`tail` в `_Atomic uint32_t`, `release` при публикации и `acquire` при чтении другой стороной, без блокировок. Типы записать в `synth.h` с `#include <stdint.h>` и без ESP-IDF.
- [ ] **Step 5: Run `make -C test/host test && make -C test/host tsan`.** Expected: все `ok ...`, ThreadSanitizer без отчётов.
- [ ] **Step 6: Commit** `feat(synth): SPSC event queue with host tests`.

---

### Task 3: `synth_render` и API модуля

Риск: высокий — realtime-аудио, состояние читается и пишется из разных задач через очереди.

**Files:**
- Create: `components/synth/src/synth.c`, `test/host/test_synth_render.c`
- Modify: `components/synth/include/synth.h`, `components/synth/CMakeLists.txt`

**Interfaces:**
- Consumes: типы и `synth_queue_*` из Task 2.
- Produces в `synth.h`:
  - `void synth_init(uint32_t sample_rate);` (сбрасывает очереди, фазу и счётчики)
  - `void synth_render(int16_t *buf, size_t frames);` (стерео, чередование L,R, сначала вычитывает обе очереди, затем пишет синус 440 Гц с амплитудой 0.5 полной шкалы)
  - `bool synth_on_midi(const synth_midi_event_t *ev);` и `bool synth_on_input(const synth_input_event_t *ev);` (`false`, если очередь полна; не блокируют)
  - `typedef struct { uint32_t midi_events; uint32_t input_events; uint32_t dropped; } synth_stats_t; synth_stats_t synth_get_stats(void);`
- Две отдельные очереди: MIDI-очередь с единственным производителем в задаче MIDI, очередь ввода с единственным производителем в задаче LVGL.

- [ ] **Step 1: Написать тесты в `test_synth_render.c`** (после `synth_init(48000)`):
  - `test_stereo_sine_amplitude`: блок 128 кадров, `L == R` в каждом кадре, пик в диапазоне 16384 ± 400.
  - `test_phase_continuous_across_blocks`: 1024 кадра одним вызовом побайтно равны 8 вызовам по 128 и вызовам размерами 1, 7, 127, 889.
  - `test_no_click`: модуль разности соседних отсчётов, включая границы блоков, не больше 1000.
  - `test_zero_frames_is_noop`: `synth_render(buf, 0)` не пишет в буфер (канарейка цела).
  - `test_render_before_init_is_silent`: без `synth_init` на статическом состоянии буфер заполнен нулями, падения нет.
  - `test_events_drained_and_counted`: 3 вызова `synth_on_midi` и 2 `synth_on_input`, затем `synth_render` на 128 кадров: `midi_events == 3`, `input_events == 2`.
  - `test_clock_flood_never_blocks`: 1000 вызовов `synth_on_midi(CLOCK)` без `synth_render` возвращаются без задержки, затем `synth_render`: `midi_events == 256`, `dropped == 744`.
- [ ] **Step 2: Run `make -C test/host test`.** Expected: FAIL (`synth_init` не определён).
- [ ] **Step 3: Реализовать `synth.c`:** статическое состояние (две `synth_queue_t`, фаза `double`, `sample_rate`, счётчики). Переход от фазы к отсчёту без `sinf`-накопления ошибки: фаза в радианах, свёртка по `2π`. Без malloc.
- [ ] **Step 4: Зарегистрировать компонент в `CMakeLists.txt`** (`SRCS synth.c synth_queue.c`, `INCLUDE_DIRS include`, `PRIV_INCLUDE_DIRS src`, без зависимостей от IDF).
- [ ] **Step 5: Run `make -C test/host test && make -C test/host tsan && idf.py build`.** Expected: все `ok ...`, сборка проходит.
- [ ] **Step 6: Commit** `feat(synth): 440 Hz stub render, event API`.

---

### Task 4: Развязка `midi` от дрона

Риск: высокий — разбор недоверенного ввода из USB, порядок и источник вызовов.

**Files:**
- Create: `components/midi/src/midi_parse.c`, `components/midi/src/midi_parse.h`, `test/host/test_midi_parse.c`
- Copy then modify: `components/midi/` из источника (`CMakeLists.txt`, `idf_component.yml`, `include/midi_in.h`, `src/midi_in.c`)

**Interfaces:**
- Consumes: `synth_midi_event_t`, `synth_on_midi` из Task 3.
- Produces в `midi_parse.h`:
  - `bool midi_parse_channel_message(uint8_t status, uint8_t d1, uint8_t d2, synth_midi_event_t *out);`
  - `bool midi_parse_realtime(uint8_t byte, synth_midi_event_t *out);`
  - Публичный `midi_in.h` (`midi_in_start`, `midi_in_set_enabled`, `midi_in_is_enabled`) не меняется.

- [ ] **Step 1: Написать тесты в `test_midi_parse.c`:**
  - `test_note_on_velocity`: `0x93, 60, 127` даёт `NOTE_ON`, `channel == 3`, `note == 60`, `velocity == 1.0f`.
  - `test_note_on_velocity_zero_is_off`: `0x90, 60, 0` даёт `NOTE_OFF`.
  - `test_note_off`: `0x80, 60, 64` даёт `NOTE_OFF`.
  - `test_all_channels_pass`: все 16 каналов дают событие со своим номером канала.
  - `test_realtime`: `0xF8` `CLOCK`, `0xFA` `START`, `0xFB` `CONTINUE`, `0xFC` `STOP`.
  - `test_ignored_realtime`: `0xFE` и `0xFF` возвращают `false`.
  - `test_other_messages_ignored`: `0xB0` (CC) и `0xE0` (pitch bend) возвращают `false`.
  - `test_data_bytes_over_127_rejected`: `d1 = 128` или `d2 = 200` возвращают `false`.
- [ ] **Step 2: Run `make -C test/host test`.** Expected: FAIL (`midi_parse.h` не найден).
- [ ] **Step 3: Реализовать `midi_parse.c`**, перенеся логику из `midi_message()` (`midi_in.c` ~ строки 305–345 источника) и разбор realtime из ~ строк 285–300; привязку к каналам 1 и 2 и тип `synth_midi_part_t` не переносить.
- [ ] **Step 4: Подключить в `midi_in.c`:** заменить все вызовы `synth_app_midi_*` на `midi_parse_*` и `synth_on_midi`, убрать `#include "synth_app.h"`. В `midi/CMakeLists.txt`: `SRCS` + `midi_parse.c`, `REQUIRES synth` вместо `synth_app`, комментарий переписать под новую границу.
- [ ] **Step 5: Проверить единственного производителя.** Прочитать `midi_in.c`: все вызовы `synth_on_midi` идут из одной задачи или колбэка TinyUSB. Записать это в комментарии `CMakeLists.txt`. Если вызовов из двух контекстов несколько, остановиться и сообщить.
- [ ] **Step 6: Run `make -C test/host test && grep -rn "synth_app" components/midi` и `idf.py build`.** Expected: все `ok ...`, `grep` пуст, сборка проходит.
- [ ] **Step 7: Commit** `feat(midi): decouple from drone, pure parser with host tests`.

---

### Task 5: Урезать `app_settings`

Риск: обычный — удаление зашитых в дрон настроек, внешний контракт `main` сохраняется.

**Files:**
- Copy then modify: `components/app_settings/` из источника (`CMakeLists.txt`, `include/app_settings.h`, `src/app_settings.c`); `src/app_presets.c` не копировать

**Interfaces:**
- Produces: `typedef enum { APP_SET_SCREEN = 0, APP_SET_RING, APP_SET_RING_MODE, APP_SET_MIDI_IN, APP_SET_COUNT } app_setting_t;`, `app_ring_mode_t` как в источнике; функции `uint32_t app_settings_init(void)`, `int app_settings_get(app_setting_t)`, `void app_settings_set(app_setting_t, int)`, `void app_settings_commit(void)`. Значения по умолчанию и диапазоны этих четырёх настроек берутся из источника без изменений.

- [ ] **Step 1: Скопировать компонент без `app_presets.c`.** Из `app_settings.h` и `app_settings.c` удалить всё остальное: настройки дрона, `app_seq_reset_*`, `app_patch_mode_t`, `app_settings_tempo_*`, `app_settings_patch_*`, `app_settings_preset_*`, `app_presets_init`, ключи NVS и штамп образа для пресетов. Комментарии про дрон и `synth_*` удалить.
- [ ] **Step 2: Добавить** `_Static_assert(APP_SET_COUNT == 4, "device settings only");` в `app_settings.c`.
- [ ] **Step 3: `CMakeLists.txt`:** убрать `app_presets.c`; `esp_app_format` оставить только если он ещё нужен.
- [ ] **Step 4: Run `grep -rniE "preset|patch|tempo|seq_reset|wave_morph|weak_vel|synth_|slide|rnd_delay|drive_pos" components/app_settings`.** Expected: пусто.
- [ ] **Step 5: Run `idf.py build`.** Expected: `Project build complete` (`main` ещё не использует компонент, поэтому добавить `app_settings` в `REQUIRES` и вызвать `app_settings_init()` в `main.c`, чтобы он собрался).
- [ ] **Step 6: Commit** `feat(settings): device-level settings only`.

---

### Task 6: Хост LVGL-экрана и заглушка

Риск: обычный — UI-заглушка и проводка событий.

**Files:**
- Copy then modify: `components/ui/include/ui_app.h`, `components/ui/src/ui_app.c`, `components/ui/CMakeLists.txt`, `components/ui/idf_component.yml`
- Create: `main/screen.c`, `main/screen.h`
- Modify: `main/main.c`, `main/CMakeLists.txt`

**Interfaces:**
- Consumes: `synth_on_input`, `synth_input_event_t` (Task 3); `ui_app_init_with_screen(const ui_app_config_t *, ui_app_screen_fn)`, `ui_app_add_focusable`, `ui_app_load_screen` (источник).
- Produces: `void screen_build(void);` строит экран с надписью «esp32-round-base», фокусируемый полноэкранный объект, обработчик `LV_EVENT_KEY` (`LV_KEY_RIGHT` и `LV_KEY_LEFT` дают `SYNTH_INPUT_ENCODER` с `a = +1/-1`) и обработчик `LV_EVENT_PRESSED` (`SYNTH_INPUT_TOUCH`, `a`,`b` = координаты точки).

- [ ] **Step 1: Скопировать `ui` без `generated/`.** В `ui_app.c` убрать `#include "ui.h"` и вызов `ui_init()`/SquareLine-кода; `ui_app_init` без экрана больше не нужен, оставить `ui_app_init_with_screen`. Если для сборки без `ui.h` нужны правки сверх этого, сделать минимальные.
- [ ] **Step 2: `components/ui/CMakeLists.txt`:** убрать glob, `generated` из `INCLUDE_DIRS`, оставить `SRCS src/ui_app.c`, `REQUIRES lvgl bsp_jc3636k718c`.
- [ ] **Step 3: Реализовать `main/screen.c`** по интерфейсу выше.
- [ ] **Step 4: `main.c`:** после `app_settings_init()` вызвать `ESP_ERROR_CHECK(ui_app_init_with_screen(NULL, screen_build))` и `bsp_display_backlight_show()`; `main/CMakeLists.txt`: добавить `ui`, `synth`.
- [ ] **Step 5: Run `grep -rniE "squareline|ui\.h|generated" components/ui main` и `idf.py build`.** Expected: `grep` пуст, сборка проходит.
- [ ] **Step 6: Commit** `feat(ui): LVGL host, placeholder screen, input events`.

---

### Task 7: Аудио-задача и интеграция

Риск: высокий — realtime-аудио, приоритеты и ядра, ошибки инициализации железа.

**Files:**
- Create: `components/synth/src/synth_audio.c`
- Modify: `components/synth/include/synth.h`, `components/synth/CMakeLists.txt`, `main/main.c`

**Interfaces:**
- Consumes: `synth_init`, `synth_render`, `synth_get_stats` (Task 3); `bsp_audio_out_init`, `bsp_audio_out_write`, `bsp_audio_out_is_ready`, `BSP_AUDIO_OUT_CONFIG_DEFAULT` (или аналог в `bsp_audio.h`); `midi_in_start` (Task 4); `app_settings_get` (Task 5).
- Produces: `esp_err_t synth_start(void);` инициализирует `synth_init(48000)` и `bsp_audio_out_init`, создаёт аудио-задачу на ядре 1 (приоритет `configMAX_PRIORITIES - 2`, стек 4096 байт). Задача в цикле вызывает `synth_render` на буфере 128 кадров и пишет блок в `bsp_audio_out_write`. Время одного рендера меряется `esp_timer_get_time`, максимум раз в 5 секунд логируется вне аудио-задачи. Предупреждение, если максимум больше половины бюджета блока (1333 мкс из 2666 мкс).

- [ ] **Step 1: Реализовать `synth_audio.c`:** если `bsp_audio_out_init` вернул ошибку, `synth_start` логирует её и возвращает ошибку без создания задачи. Логирование статистики через периодический `esp_timer` (не из аудио-задачи). `synth_audio.c` в `CMakeLists.txt` отдельно от IDF-независимых файлов, `PRIV_REQUIRES bsp_jc3636k718c esp_timer`.
- [ ] **Step 2: Собрать `main.c`:** `bsp_init` → `bsp_print_info` → `app_settings_init` → `ui_app_init_with_screen` → `bsp_display_backlight_show` → `synth_start` (при ошибке только предупреждение) → `midi_in_start` только если `app_settings_get(APP_SET_MIDI_IN)` не ноль → `app_ota_mark_valid()` (сохранить как в источнике, иначе откат OTA) → бесконечный `vTaskDelay`. `app_net_init` не вызывать, как в источнике.
- [ ] **Step 3: Run `idf.py build`.** Expected: `Project build complete`.
- [ ] **Step 4: Прошить и проверить на плате** (нужна плата Guition JC3636K718C): `tools/idf.sh flash monitor`. Expected в логе: строка запуска аудио-задачи с блоком 128 кадров, периодическая строка `render max=<N> us` с `N` меньше 1333, на выходе слышен ровный тон 440 Гц без щелчков. Дисплей показывает надпись, поворот энкодера и касания увеличивают счётчики `input_events` в логе, отправка ноты (`tools/midi-send.py`) увеличивает `midi_events`.
- [ ] **Step 5: Проверить сбой железа.** Временно собрать с `features.audio_out = false` в `bsp_init`: загрузка идёт до конца, в логе предупреждение об аудио, паники и перезагрузок нет. Вернуть исходное значение.
- [ ] **Step 6: Проверить пустой NVS.** `idf.py erase-flash flash monitor`: прибор стартует с настройками по умолчанию, при `MIDI_IN = 0` в логе нет запуска USB MIDI.
- [ ] **Step 7: Commit** `feat: audio task, main integration`.

Если платы под рукой нет, шаги 4–6 помечаются как непроверенные, выполняются владельцем платы, а сборка (шаг 3) остаётся минимальным критерием.

---

### Task 8: Документы и финальная чистка

Риск: обычный — документация и чистка ссылок.

**Files:**
- Create: `README.md`
- Modify: `THIRD-PARTY.md`, `docs/HARDWARE.md` (если нужно)
- Delete: `CHANGELOG.md`, `FEATURES*.md`, `README.en.md`, SquareLine- и релиз-документы в `docs/`

**Interfaces:**
- Consumes: готовый проект после Task 1–7.

- [ ] **Step 1: Написать `README.md`:** что это (каркас для синтезатора на Guition JC3636K718C), как собрать и прошить (`tools/idf.sh`), как устроены `bsp_jc3636k718c`, `midi`, `app_settings`, `app_net`, `ui`, `synth`, где писать DSP (`synth_render`), откуда код (ссылка на источник и MIT). Из `docs/` оставить `HARDWARE.md`.
- [ ] **Step 2: `THIRD-PARTY.md`:** убрать записи про удалённое (Helix, шрифты, SquareLine, ESP32Synth), остальное оставить; добавить строку об происхождении кода.
- [ ] **Step 3: Run `grep -rniE "drone|дрон|squareline|synth_app|synth_dsp|synth_core|synth_ui|obod|guition_knob" . --exclude-dir=build --exclude-dir=managed_components --exclude-dir=.git --exclude-dir=docs/superpowers`.** Expected: остаются только осознанные упоминания источника в `README.md`, `LICENSE` и `THIRD-PARTY.md`.
- [ ] **Step 4: Run `make -C test/host test && idf.py build`.** Expected: все тесты `ok`, `Project build complete`.
- [ ] **Step 5: Commit** `docs: README, third-party notes`.
