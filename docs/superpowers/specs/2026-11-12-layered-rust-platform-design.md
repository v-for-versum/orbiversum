> Historical document. Paths predate the sdk/apps/boards layout; see README.md.

# Слоёная Rust-платформа с апплетами (Dronecaster как первое применение)

Дата: 2026-11-12
Статус: черновик на проверку. Продолжает `2026-11-12-esp32-round-base-design.md` (C-каркас с BSP).

## Цель

Прошивка на Rust для платы Guition JC3636K718C (ESP32-S3, PSRAM, 16 МБ flash, ЦАП PCM5100A,
круглый дисплей ST77916 360×360, тач CST816S, энкодер, SD, PDM-микрофон).
Платформа открыта для расширения: новый апплет добавляется отдельным крейтом, новая плата
(другой девкит) подключается отдельным крейтом платы.

Первое применение: порт дронов из `northern-information/dronecaster` (SuperCollider/Lua на norns).
Первый апплет — Starlids.

## Слои

Зависимости идут только вниз.

```
firmware-<board>        бинарник: выбирает плату и список апплетов
 ├─ applets-*           Starlids и другие дроны
 ├─ os                  запуск и переключение апплетов, меню, ввод, маршрутизация аудио
 ├─ dsp                 звуковые графы и движок (no_std)
 ├─ blocks              осцилляторы, фильтры, задержки, LFO, секвенсоры (no_std)
 └─ board-jc3636k718c   hw: драйверы платы
```

Правила:
- `blocks` и `dsp` не знают про `os`, плату и графику; собираются и звучат на Mac.
- Апплет не знает про конкретную плату; `os` видит плату только через абстракцию `Board`.
- Расширение происходит на этапе сборки: Cargo workspace, апплеты как крейты, прошивка собирает выбранный набор.
  Динамическая загрузка с SD и скрипты (Lua/Wasm) вне объёма.

## Принятые решения

1. Одна прошивка, апплеты внутри, выбор на плате (меню). Не отдельная прошивка на апплет.
2. Слой hw на `esp-idf-svc` (std Rust поверх ESP-IDF), существующий C BSP
   (`components/bsp_jc3636k718c`) остаётся и вызывается из крейта платы.
   Переход на `esp-hal` (no_std) возможен позже: граница — абстракция `Board`.
3. Апплет рисует сам через `embedded-graphics` и сообщает изменённые области (частичная отрисовка).
   LVGL остаётся в BSP только как временная заглушка. Приоритет — минимум лагов на плате.
4. Аудио на ядре 1 с высоким приоритетом, 48 кГц, блоками, без аллокаций и блокировок в аудио-пути.
   UI, ввод и отправка кадра на ядре 0. Дисплей — DMA из внутренней RAM полосами.
5. Частота дискретизации 48 кГц сквозная (как у оригинального движка).

## Контракты (реальные интерфейсы после первого среза)

Сигнатуры взяты из кода на ветке scaffold (`rust/crates/*`), не из первоначальных набросков.

**`os::Applet`** — аудио-половина апплета. `Send`, владеет диспетчер, живёт на аудио-потоке.
```rust
pub trait Applet {
    fn info(&self) -> AppletInfo;                  // AppletInfo { name: &'static str }
    fn start(&mut self);                           // может аллоцировать; вызывает Dispatcher::service вне аудио-потока
    fn audio(&mut self, out: &mut [[f32; 2]]);     // realtime: перезаписывает стерео-кадры, без аллокаций и блокировок
    fn stop(&mut self);                            // может освобождать память; вызывает service
}
```

**`os::AppletUi`** — UI-половина. Живёт на UI-потоке, `Send` не нужен.
```rust
pub trait AppletUi {
    fn info(&self) -> AppletInfo;
    fn input(&mut self, ev: InputEvent);
    fn invalidate(&mut self);                      // фреймбуфер больше не содержит картинку апплета: следующий draw перерисует всё
    fn draw(&mut self, d: &mut Canvas<'_>, dirty: &mut Dirty);
}
pub enum InputEvent { Knob(i32), Tap { x: i16, y: i16 }, Press }
```
`Dirty` — до `MAX_DIRTY = 8` прямоугольников (`add`, `set_full`, `rects`, `is_full`, `is_empty`, `clear`); девятый превращает его в «весь экран».

**`os::Params`** — единственная связь между половинами: `hz` и `amp` как `f32` в `AtomicU32` (`hz()/set_hz()`, `amp()/set_amp()`, Relaxed). Раздаётся как `Arc<Params>`.

**`os::Dispatcher` / `os::AudioPath`**
```rust
Dispatcher::new(Vec<Box<dyn Applet + Send>>) -> Dispatcher
Dispatcher::with_params(Vec<Box<dyn Applet + Send>>, Arc<Params>) -> Dispatcher   // стартует апплет 0
fn take_audio(&mut self) -> AudioPath          // один раз; AudioPath: Send, уходит в render-замыкание
fn select(&mut self, idx: usize)               // запрос переключения (вне диапазона игнорируется)
fn service(&mut self)                          // UI-поток, ОБЯЗАТЕЛЬНО каждый UI-тик
fn len(&self) / info(&self, idx) -> Option<AppletInfo> / selected(&self) -> usize / params(&self) -> Arc<Params>
AudioPath::audio(&mut self, out: &mut [[f32; 2]])   // realtime, в IRAM (.iram1.audio_path)
```
Переключение: `audio()` гасит старый апплет (fade ~20 мс, `DEFAULT_SAMPLE_RATE = 48000`), паркуется; `service()` делает `stop()` старого и `start()` нового
вне аудио-потока и отпускает аудио. Пока `service()` не вызван, выход молчит. `selected()` возвращает цель (последний запрошенный апплет), а не звучащий сейчас.

**`os::Board`**
```rust
pub trait Board {
    fn poll_input(&mut self) -> Option<InputEvent>;
    fn audio_out(&mut self) -> &mut dyn AudioOut;
    fn display(&mut self) -> &mut dyn Display;
}
pub trait AudioOut { fn start(&mut self, sr: u32, render: RenderFn) -> Result<(), AudioError>; }  // RenderFn = Box<dyn FnMut(&mut [[f32;2]]) + Send>
pub trait Display { fn size(&self) -> (u32, u32); fn surface(&mut self) -> &mut dyn Surface; fn flush(&mut self, dirty: &Dirty); }
pub trait Surface { fn size; fn draw_pixels; fn fill_contiguous; fn fill_solid }   // dyn-совместимая поверхность
pub struct Canvas<'a>(pub &'a mut dyn Surface);   // реализует embedded-graphics DrawTarget<Color = Rgb565, Error = Infallible>
```
`DrawTarget` не dyn-совместим (обобщённые методы), поэтому апплет рисует в `Canvas` поверх `&mut dyn Surface`. Ошибки аудио — `AudioError`
(`Backend(i32)`, `AlreadyStarted`, `UnsupportedSampleRate(u32)`, `Spawn`).

**`board-jc3636k718c`**: `JcBoard::new() -> Result<JcBoard, BoardError>` (после `bsp_init`) реализует `Board`. `JcAudio` — `AudioOut` (48 кГц, 16 бит, стерео, блок `BLOCK_FRAMES = 128`,
задача FreeRTOS приоритет 20, закреплена за ядром 1, буферы на стеке задачи; `frames_to_i16` в IRAM). `JcDisplay = FbDisplay<BspPanel>`: фреймбуфер RGB565 360×360
в PSRAM, отправка грязных областей полосами по 16 строк (`STRIP_PIXELS = 360*16`) через внутреннюю DMA-RAM и `bsp_display_draw_rect`. `JcInput` — энкодер, тач, фильтр жестов
(`LONG_PRESS_MS = 600`). Чистая логика (разбиение на полосы, клиппинг dirty, f32→i16) вынесена так, чтобы тестироваться на хосте.

**`applets::all(seed, sr, params) -> Vec<(Box<dyn Applet + Send>, Box<dyn AppletUi>)>`** — пары (аудио, UI): Starlids и Sine. Аудио-половина — `VoiceApplet` поверх `dsp::Voice`
(`set_sample_rate`, `set_params(hz, amp)`, `process(&mut [[f32; 2]])`), UI-половина — `ParamUi` поверх `Params`.

### Отклонения от первоначального плана

- Апплет разделён на `os::Applet` (аудио, `Send`, у диспетчера) и `os::AppletUi` (UI); связь между ними только `Params`. В плане был один трейт `Applet`.
- `Dispatcher::service()` нужно звать на каждом UI-тике: переключение завершается только там, до тех пор звук молчит.
- `selected()` возвращает цель переключения, а не звучащий апплет (аксессора звучащего нет).
- `take_audio() -> AudioPath`: аудио-половина диспетчера отдаётся один раз и переезжает в аудио-поток.
- Отрисовка идёт через `Canvas` (обёртка над `&mut dyn Surface`), а не напрямую через `DrawTarget`; добавлен `AppletUi::invalidate`.
- У платы нет кнопки энкодера: «нажатие» (`InputEvent::Press`) формируется удержанием пальца на тачскрине 600 мс; удержание открывает меню.
- Основная задача (UI, ядро 0) переведена на ядро 0 через `sdkconfig.defaults`; крейт платы стал `std`.
- Обмен звук↔UI: `Params` (атомики) вместо lock-free очереди; очередь событий в аудио не нужна на первом срезе.

## Порядок портирования дронов

По разбору 38 файлов `engine/drones/*.scd` (оценка по regex, числа приблизительные):
сэмплов, FFT и свёртки нет ни в одном дроне; все буферы — внутренние линии задержки.

1. Starlids (SawDPW, биквады LPF/HPF, MoogLadder, DelayC, медленные LFO).
2. Sine, Square, Triangle, Supersaw, harms way.
3. Hecker, Thx, UNRELACC, Mt. Lion, Mt. Zion.
4. Apparatus, Gristle, Rehberg, Nautilus, Toshiya.
5. Остальные средней сложности.
6. Belong, Eno, UNMEMQUA (DynKlank, буферы).
7. UNWEALNE, UNREANTH, Ruins (PitchShift, GVerb) — по желанию.

Заранее проектируются два общих механизма: Demand-секвенсоры (Dseq/Drand/Dxrand/Dbrown, ≥9 дронов)
и фидбэк через LocalIn/LocalOut (10 дронов).
Звучание MoogLadder, DynKlank и PitchShift воспроизводится приближённо, не побитово.

## Первый срез и критерий успеха

Объём: каркас workspace, `blocks` и `dsp` для Starlids и Sine, крейт платы поверх текущего BSP,
`os` с меню на два апплета.

Готово, когда (статус после среза, 2026-11-12):
1. Starlids звучит на Mac (запись в WAV) и узнаваемо похож на оригинал на norns. **Выполнено** по слову пользователя: после оптимизации на плате и WAV «все ок».
   Строгого A/B с norns и количественного сравнения с оригиналом нет (есть только golden-тест против собственной версии до оптимизации).
2. Тот же код играет на плате без щелчков, ядро аудио не перегружено (замер загрузки). **Частично**: загрузка измерена (ядро 1, mean 86.9%, ниже 100%), пиков нет, слух пользователя «все ок».
   Измеренного порога щелчков нет; переключение апплетов проверено на плате в общем виде. Формально «без щелчков» не доказано измерением.
3. Меню на круглом экране переключает два апплета без заметных лагов (замер времени кадра). **Частично**: пользователь подтвердил «работает»; время кадра измерено (см. «Риски»),
   задержка отклика энкодера не измерена, числа лагов меню нет.
4. По итогам среза зафиксированы реальные интерфейсы `Applet` и `Board`. **Выполнено**: раздел «Контракты».

## Риски и неизвестное

- Сборка Rust под `xtensa-esp32s3` и FFI к C BSP (спайк, `rust/firmware-jc3636k718c`):
  - Собрано и слинковано: бинарник `std` Rust под `xtensa-esp32s3-espidf`; C BSP подключён как `extra_components`,
    в ELF присутствуют `bsp_init`, `bsp_audio_out_init`, `bsp_audio_out_write`; образ 998 КБ, `espflash save-image` проходит.
  - Версии: espup 0.19.0, Xtensa Rust 1.97.0.0 (toolchain `esp`), espflash 4.7.0, ldproxy 0.3.5,
    esp-idf-sys 0.38.1, esp-idf-svc 0.53.0, esp-idf-hal 0.47.0, embuild 0.33.7.
  - ESP-IDF 6.1.0 (`ESP_IDF_VERSION = "v6.1"`, `CONFIG_IDF_INIT_VERSION="6.1.0"`) собирается с `esp-idf-sys` 0.38.1,
    хотя в его CHANGELOG заявлена лишь «basic compatibility» с 6.0; сборка IDF и компонентов BSP (LVGL 9.6.0, esp_lvgl_port 2.9.0)
    прошла, предупреждения CMake про include-каталоги wpa_supplicant/esp_wifi не фатальны.
  - Ограничения: кастомная `partitions.csv` не находится (путь считается от каталога сборки `esp-idf-sys`), в спайке
    используется `PARTITION_TABLE_SINGLE_APP_LARGE`; раскладку OTA/storage надо решать отдельно.
    `sdkconfig.jc3636k718c` берётся из корня репозитория (сейчас не закоммичен) как база sdkconfig.
  - Прошивка и монитор (плата ESP32-S3 rev v0.2, 16 МБ, `/dev/cu.usbmodem2101`, лог через USB-Serial-JTAG):
    - Первая прошивка: загрузчик и PSRAM в порядке (`esp_psram: Found 8MB PSRAM device`, `Speed: 80MHz`,
      `Adding pool of 7232K of PSRAM memory to heap allocator`), но дальше перезагрузочный цикл до `app_main`:
      `E (574) i2c: CONFLICT! driver_ng is not allowed to be used with this old driver` -> `abort() was called` -> `Rebooting...`.
      Строки `rust: bsp ok` нет. Причина: в образ попал старый I2C-драйвер IDF (`check_i2c_driver_conflict`), а BSP использует новый;
      Причина по map-файлу (`libespidf.map`): `driver/libdriver.a(i2c.c.obj)` затянут символом `i2c_driver_delete` из
      `libesp_idf_hal-*.rlib` (cgu esp-idf-hal, зависимость esp-idf-svc), т.е. старый драйвер приходит из Rust-стороны, а не из BSP.
    - Обход: `CONFIG_I2C_SKIP_LEGACY_CONFLICT_CHECK=y` в `sdkconfig.defaults`. Проверено на плате после перепрошивки
      (захват 22 с после сброса, один `rst:` за весь лог, ни `abort`, ни `Rebooting`):
      `I (385) esp_psram: Found 8MB PSRAM device`, `I (1152) bsp.display: ST77916 360x360 ready: QSPI 50 MHz, 16-line double buffer in internal RAM`,
      `I (1585) firmware_jc3636k718c: rust: bsp ok`, `I (1588) firmware_jc3636k718c: rust: psram total = 7460780 bytes`,
      затем `rust: alive` каждые 5 с (6595–21598 мс) и LVGL sysmon ~90 FPS. Работа более 20 с без перезагрузок.
    - Итог: шаг 4 проверен: Rust-прошивка линкует C BSP, `bsp_init` из Rust проходит (дисплей поднят; touch/encoder по логу не проверялись), PSRAM определён.
      Не проверено: энкодер/тач вручную, аудио (`bsp_audio_out_*` только слинкованы), длительная стабильность (>22 с).
      Обход i2c — костыль; чистое решение (не тянуть старый драйвер через esp-idf-hal) не пробовалось.
- Нагрузка на CPU других (тяжёлых) дронов не измерена; оценки только по составу UGen.
- Скорость и поведение `fundsp` на Xtensa не проверены; Starlids написан вручную.

### Результаты замеров на плате (2026-11-12)

Условия: ядро 1 @ 240 МГц, блок 128 кадров при 48 кГц, бюджет 2666 мкс на блок. Числа сняты при работающем UI (экран, обновление раз в ~5 с), не при непрерывной анимации.

**Аудио, Starlids.** Загрузка ядра 1: mean 86.9% (≈2317 мкс), максимум за окно 87.0–87.9% (до 2343 мкс); худший блок по профайлеру `prof: worst block` 2254–2260 мкс (другое окно), пиков нет.
Профиль по секциям (мкс на блок): control 297, saw 342, lpf 271, delay 617, mix 180, moog 451, hpf 41, other 45; сумма ~2244. Sine занимает около 30% ядра 1.

Путь оптимизации (загрузка ядра 1): исходно ~410% по mean (до 11060 мкс на блок по максимуму) → 231% → 135% → 110% → 87%. Главные причины:
- libm `sinf/cosf/expf/powf/exp2f` на ESP32-S3 считают через программный f64; заменены на f32-полиномы (`blocks::fast`);
- f32-деление программное (~100 тактов); убрано из горячих путей (кэш 1/sr, `fast::recip`, tanh без деления);
- `tanhf` стоил 1574 нс, в сумме ~70% исходной нагрузки; заменён на Паде-аппроксимацию и rational `tanh_cheap` в ступенях лестницы;
- код и rodata выполнялись из PSRAM (XIP) через общий кэш; отключено в `sdkconfig.defaults` (`SPIRAM_FETCH_INSTRUCTIONS`, `SPIRAM_RODATA` выключены), аудио-путь целиком в IRAM;
- цепочки FPU зависимы по задержке (~5 тактов на операцию): обработка группами по 4 линии для перекрытия цепочек.

**Аудио, Eno** (2026-11-13, fw `aace293-dirty`: реверб 4+2, Klank включён, атака `EnvPerc` по кривой −4, `PAD_CTRL_STRIDE` 4). Установившийся режим при почти неподвижном UI: ядро 1 mean ~91.0–91.2%, max 91–93% (2430–2470 мкс), write errors 0, short writes 0; худший блок по профайлеру 2380–2405 мкс (запас ~260 мкс до 2666).
Профиль по секциям (мкс на блок): control ~97, pad 790, moog 462, klank 262, piano 160, perc 151–281 (PolyPerc пропускается при нулевой огибающей; закладываться на 281), reverb 225, final 81; сумма ~2360 при perc 281.
Целевые ≤ ~86% не достигнуты (на ~5 п.п. выше; Starlids 87%): отклонение принято пользователем, звук подтверждён на слух. Правки после замера: цикл пианино пропускается через 8 с после последней ноты (пик не меняется, выигрыш только на медленных нотах).
Переключение Starlids → Eno при активной анимации меню (30 flushes за 5 с): max 100.5–101% (2680–2693 мкс > 2666), mean до 91.4%; write errors 0, но блок превысил бюджет: щелчок возможен, на слух не проверен.

**Дисплей.** Полная перерисовка 129600 px за ~43.1-43.4 мс; частичная 7200 px за ~2.3-2.8 мс.

**Память.** Свободная внутренняя куча ~194 КБ, PSRAM 8 МБ.

**Не измерено / не подтверждено:**
- Задержка отклика энкодера: НЕ измерена.
- Ориентация тача и направление энкодера по отдельности не подтверждены (пользователь сказал только, что «работает»).
- Загрузка аудио при непрерывной анимации UI (полные перерисовки) не измерялась.
- Причина прежних пиков ~20-26 мс не доказана (подозрение: общий кэш flash/PSRAM); после переноса аудио-пути в IRAM пиков нет, но корень не установлен.
- Путь тайм-аута/позднего завершения отправки полосы дисплея на плате не проверялся.

## Что менять перед этапами 2-3 портирования

- Запас по CPU: ~13% (~340 мкс) на ядре 1 при одном Starlids. Тяжёлые дроны (Moog, много DelayC) придётся портировать теми же приёмами:
  f32 без деления и без f64/libm, таблицы и полиномы, группировка по 4 линии, код в IRAM, буферы во внутренней RAM.
  Два тяжёлых дрона одновременно не влезут.
- Компромиссы звука Starlids относительно оригинала: Moog в один проход вместо 2x oversampling, один входной tanh (вместо tanh(tanh(x))),
  rational tanh в ступенях лестницы (ошибка ~2%); golden-тест разрешает расхождение до 10 дБ в полосе >6 кГц.
- Второе ядро: ядро 0 занято UI и LVGL-задачей (~1 кГц, LVGL остановлен только для отрисовки, задача крутится вхолостую); перенос части DSP на ядро 0 возможен, но не исследован.
- Отложено: перенос цепочки `VoiceApplet::audio` / доступа к `Params` в IRAM (пиков нет); влияние логов на пики не замечено; проверка на слух одного входного tanh и Moog в 1 проход против norns.
- Неподтверждённые мелочи среза: нет остановки/Drop у аудио-задачи, нет дребезга/порога перетаскивания у тача, нет отмены в меню.

## Открытые вопросы

- `sdkconfig.jc3636k718c` не отслеживается git; крейт `firmware-jc3636k718c` берёт его из корня репозитория, поэтому чистая сборка из свежего клона невозможна.
- Кастомная `partitions.csv` не подхватывается (путь считается от каталога сборки `esp-idf-sys`), используется `PARTITION_TABLE_SINGLE_APP_LARGE`; раскладка OTA/storage не решена.
- `CONFIG_I2C_SKIP_LEGACY_CONFLICT_CHECK=y` остаётся обходом: старый I2C-драйвер затянут `esp-idf-hal`. Возможен конфликт со старым драйвером при реальном использовании I2C из Rust; чистое решение не пробовалось.
- Версия приложения ESP-IDF (`app_version`) кешируется CMake и не обновляется; версия прошивки печатается из `build.rs` (`git describe`).
