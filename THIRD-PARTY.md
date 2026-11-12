# Сторонние компоненты выпуска 1.2.1

Документ описывает всё чужое, что попадает в выпуск (в прошивку, в исходный архив и в
документацию), а также то, что из выпуска удалено. Версии взяты из `dependencies.lock` и из файлов компонентов, лицензии — из
файлов лицензий в дереве. Пути даны от корня репозитория.

Весь код самого прибора (`components/synth_dsp`, `components/synth_app`, `components/synth_ui`,
`components/ui`, `components/midi`, `components/app_settings`, `components/app_net`,
`components/bsp_jc3636k718c`, `main/`, `tools/`) написан в этом проекте.

## Сводная таблица

| Компонент | Версия | Лицензия | Где лежит | Менялся нами |
|---|---|---|---|---|
| ESP-IDF | 6.1.0 | Apache-2.0 | вне репозитория, `C:\esp\v6.1\esp-idf` | нет |
| ESP32Synth — движок голосов | 2.5.0 | MIT | `components/synth_core` | **да**, см. `PATCHES.md` |
| Helix MP3 decoder (внутри synth_core) | по архиву ESP32Synth, отдельного номера нет | RCSL 1.0 / RPSL 1.0 (RealNetworks) | **удалён из выпуска 1.0.0**, был в `components/synth_core/src/helix` | каталог удалён целиком |
| Обвязка Helix (`buffers.c`, `helix_mp3.c`, `helix_mp3.h`) | — | **не указана в файлах** | **удалён из выпуска 1.0.0** | каталог удалён целиком |
| LVGL | 9.6.0~1 | MIT | `managed_components/lvgl__lvgl` | нет |
| LVGL: QR Code (Project Nayuki) | — | MIT | `managed_components/lvgl__lvgl/src/libs/qrcode` | нет |
| LVGL: шрифт Montserrat | — | OFL-1.1 | `managed_components/lvgl__lvgl/.../font_license/Montserrat/OFL.txt` | нет |
| esp_tinyusb | 2.4.0 | Apache-2.0 | `managed_components/espressif__esp_tinyusb` | нет |
| TinyUSB (espressif/tinyusb) | 0.21.0~2 | MIT | `managed_components/espressif__tinyusb` | нет |
| espressif/button | 4.2.1 | Apache-2.0 | `managed_components/espressif__button` | нет |
| espressif/cmake_utilities | 0.5.3 | Apache-2.0 | `managed_components/espressif__cmake_utilities` | нет |
| espressif/esp_lcd_st77916 | 2.0.2 | Apache-2.0 | `managed_components/espressif__esp_lcd_st77916` | нет |
| espressif/esp_lcd_touch | 1.2.1 | Apache-2.0 | `managed_components/espressif__esp_lcd_touch` | нет |
| espressif/esp_lcd_touch_cst816s | 1.1.2 | Apache-2.0 | `managed_components/espressif__esp_lcd_touch_cst816s` | нет |
| espressif/esp_lvgl_port | 2.9.0 | Apache-2.0 | `managed_components/espressif__esp_lvgl_port` | нет |
| espressif/led_strip | 3.1.0~1 | Apache-2.0 | `managed_components/espressif__led_strip` | нет |
| espressif/json_generator | 2.0.0 | Apache-2.0 | `managed_components/espressif__json_generator` | нет |
| espressif/network_provisioning | 1.3.1 | Apache-2.0 | `managed_components/espressif__network_provisioning` | нет |
| Freeverb (Jezar at Dreampoint) | — | public domain | алгоритм описан в `components/synth_dsp/src/synth_fx_reverb.cpp` | реализация наша |
| Плита Dattorro | — | статья JAES, не код; сверка коэффициентов — с портом `el-visio/dattorro-verb`, MIT (проверено 15.10.2026 по файлу LICENSE репозитория) | `components/synth_dsp/src/synth_fx_plate.cpp` | реализация наша |
| Mermaid | 11.17.2 | MIT | `docs/signal-chain/vendor/mermaid.min.js` | нет |
| esp-web-tools | 10.4.0 | Apache-2.0 | не хранится, грузится с unpkg | нет |
| actions/checkout, actions/cache, actions/upload-artifact | v4 | MIT | `.github/workflows/build.yml` | нет |

## Движок голосов — главный пункт

`components/synth_core` — это библиотека **ESP32Synth 2.5.0**, автор Danilo Gabriel
(danilogcrf2-oss), лицензия **MIT**.

Доказательства: `components/synth_core/LICENSE` (строка 1 — `MIT License`, строка 3 —
`Copyright (c) 2025 danilogcrf2-oss`), `components/synth_core/library.properties`
(`version=2.5.0`, `url=https://github.com/danilogcrf2-oss/ESP32Synth`),
`components/synth_core/PATCHES.md` (строки 3–6 — источник и версия).

Лицензия понятна и подтверждена файлом. MIT позволяет закрытый выпуск, требует сохранения
текста лицензии и копирайта.

Нами библиотека **правилась**. Полный список правок — `components/synth_core/PATCHES.md`:
сборочная обвязка (у апстрима нет сборочных файлов), правки под ESP-IDF 6.1 (частный
`esp_private/esp_clk.h`, удалённый тип `i2s_port_t`, новые поля конфигураций I2S),
два исправленных бага апстрима и изменение `ESP32Synth_Config.hpp` (`MAX_VOICES 24`,
`MAX_BUSES 6`, `ENABLE_MP3 0`). Сборка компилирует только `src/ESP32Synth.cpp` —
`components/synth_core/CMakeLists.txt`, блок `idf_component_register`.

## Helix MP3 decoder: удалён из выпуска 1.0.0

В архиве ESP32Synth 2.5.0 внутри `components/synth_core/src/helix` лежали 23 файла
(около 252 КБ) декодера MP3 производства RealNetworks. В 20 из них стоял блок:

```
Version: RCSL 1.0/RPSL 1.0
Portions Copyright (c) 1995-2002 RealNetworks, Inc. All Rights Reserved.
... subject to the current version of the RealNetworks Public Source License
Version 1.0 (the "RPSL") ...
```

Это **лицензия семейства reciprocal/copyleft** (RPSL — OSI-совместимая, но не GPL).
Она не MIT и не Apache: для этих файлов действуют требования о сохранении уведомлений и о
распространении производных на тех же условиях. Точные условия лежат по адресу
`http://www.helixcommunity.org/content/rpsl`; текста лицензии в репозитории не было — в файлах
был только блок уведомления.

Три файла из 23 **не имели никакого уведомления о лицензии**: `buffers.c`, `helix_mp3.c`,
`helix_mp3.h`. Они включают `esp_heap_caps.h`, то есть являются адаптацией под ESP-IDF, и
происхождение их прав было неясно.

**Почему каталог удалён.** В корне лежит `LICENSE` — MIT, `Copyright (c) 2026 Annabiosis`.
Эта лицензия покрывает код проекта и **не может покрывать файлы Helix**: их условия задаёт
RPSL/RCSL, и объявить их MIT нельзя. Поэтому каталог `components/synth_core/src/helix` удалён
из дерева в выпуске 1.0.0. В прошивку декодер и раньше не входил: `ENABLE_MP3` выставлен в 0
(`components/synth_core/src/ESP32Synth_Config.hpp`), `src/helix` не перечислен в `SRCS`
(`components/synth_core/CMakeLists.txt`), и в приборе нет воспроизведения MP3.

**Как вернуть.** Каталог `src/helix` нужно взять из исходного архива ESP32Synth 2.5.0 и
положить обратно. Ссылки на декодер в живом коде сохранены под условием: `ESP32Synth.cpp`
(строки 5–6) и `ESP32Synth_SDStream.hpp` (строки 4–5) — `#if ENABLE_MP3` /
`#include "helix/mp3dec.h"`. Для потокового MP3 нужно вернуть каталог, перевести `ENABLE_MP3`
в 1 и добавить `src/helix` в `SRCS`. Вместе с каталогом возвращаются и требования RCSL/RPSL:
исходный архив тогда нужно сопровождать текстом RPSL и отдельной пометкой о том, что MIT на
эти файлы не распространяется.

## Копилефт в проекте

Прямого совпадения с GPL, LGPL и AGPL в собираемом коде **нет**. Проверены все `*.c`, `*.h`,
`*.cpp`, `*.hpp`, `*.md`, `*.py`, `*.ps1`, `*.html`, `*.json` репозитория.

Найденные упоминания GPL относятся к загруженным компонентам, которые не компилируются:

- `managed_components/lvgl__lvgl/src/libs/vg_lite_driver/VGLiteKernel/*` — драйвер Vivante под
  GNU General Public License (шапка `vg_lite_kernel.h` и соседних файлов). Не собирается:
  `sdkconfig` содержит `# CONFIG_LV_USE_DRAW_VG_LITE is not set`.
- `managed_components/lvgl__lvgl/src/libs/freetype` — `FTL OR GPL-2.0-or-later`, двойная
  лицензия, мы выбрали бы FTL. Не собирается: `# CONFIG_LV_USE_FREETYPE is not set`.
- `managed_components/espressif__tinyusb/test/hil/pymtp.py` — пометка `__LICENSE__ = "GPL-3"`,
  тестовый скрипт, в сборку не входит.

Отдельная оговорка: `managed_components/` перечислен в `.gitignore` и восстанавливается
менеджером компонентов, поэтому эти файлы не входят в исходный архив. Если кто-то положит
`managed_components` в архив целиком, в него попадут и файлы драйвера Vivante под GPL.

Вывод: **копилефта в дереве исходников проекта нет.** Декодер Helix (RPSL/RCSL) удалён из
выпуска 1.0.0 (см. раздел выше); в прошивке его не было.

## Что собирается в прошивку из LVGL

LVGL содержит много встроенных библиотек, и большинство из них выключено. Включены:

- QR Code (Project Nayuki) — `CONFIG_LV_USE_QRCODE=y`, лицензия MIT
  (`managed_components/lvgl__lvgl/src/libs/qrcode/LICENSE.txt`, строка 1:
  `Copyright © 2025 Project Nayuki. (MIT License)`);
- шрифты Montserrat — `CONFIG_LV_FONT_MONTSERRAT_14`, `_20`, `_28` в состоянии `=y`,
  шрифт по умолчанию `CONFIG_LV_FONT_DEFAULT_MONTSERRAT_14=y`, лицензия OFL-1.1
  (`.../font_license/Montserrat/OFL.txt`).

Выключены и в образ не попадают: VG Lite, ThorVG, NanoVG, FreeType, libpng, libjpeg-turbo,
LodePNG, Tiny TTF, GIF, barcode, LZ4, frogfs (все — `is not set` в `sdkconfig`).

## Шрифты

`components/synth_ui/src/fonts/lv_font_anbscreen_14.c` и `lv_font_anbscreen_20.c` —
собственные шрифты проекта, нарисованные для этого прибора: 14 px для подписей и 20 px
для значений и заголовков. Сторонних шрифтов на экране прибора нет, поэтому и
лицензионного текста шрифта в дереве не требуется. Обе таблицы глифов собраны
сторонним экспортёром под LVGL; посадку глифов и кернинг в них проверяет
`tools/font-metrics-check.js`, а пересчёт вертикальных метрик после экспорта делает
`tools/font_metrics_fix.js`.

Шрифты самой документации — другое дело и остаются сторонними: IBM Plex Sans на
страницах сайта под OFL-1.1, Montserrat внутри LVGL под OFL-1.1 (см. выше).

## Алгоритмы эффектов

- **Freeverb** — `components/synth_dsp/src/synth_fx_reverb.cpp`, строка 11 и далее: восемь
  гребёнок с демпфированием и четыре аллпасса на канал, настройки взяты из Freeverb
  (строка 42 — `Freeverb's tunings at 44.1 kHz`). Freeverb (Jezar at Dreampoint) — public
  domain. Код в файле написан в этом проекте.
- **Плита Dattorro** — `components/synth_dsp/src/synth_fx_plate.cpp`, строки 11–24: структура,
  длины линий, позиции и знаки отводов и коэффициенты диффузии взяты из статьи Dattorro,
  сверены построчно с портом `el-visio/dattorro-verb`. Исходников этого порта в репозитории
  нет, копирования файлов не было. Лицензия порта проверена 15.10.2026 по файлу LICENSE
  репозитория: MIT, Copyright (c) 2022 Pauli Pölkki. Ограничений на выпуск это не
  накладывает.
- PolyBLEP-таблицы `AA_Saw`/`AA_Pulse` приходят из ESP32Synth
  (`components/synth_core/src/ESP32Synth_Patches.hpp`) и покрыты её MIT.

## Документация и сборка

- **Mermaid 11.17.2**, лицензия MIT. Файл `docs/signal-chain/vendor/mermaid.min.js`
  (3,5 МБ) положен в репозиторий. Версия видна строкой `version:"11.17.2"` внутри файла,
  лицензия названа в `docs/signal-chain/obod-signal-chain.html` (строка 15) и в
  `docs/signal-chain/gen_html.py` (строка 33). Библиотека нужна для отрисовки Mermaid-схемы
  сигнального тракта в браузере. **В прошивку она не входит.**
- **esp-web-tools 10.4.0**, лицензия Apache-2.0. Не хранится в репозитории: страница
  `tools/webflash/index.html` (строка 229) подключает её с `unpkg.com`. Нужна для записи
  прошивки из браузера, в прошивку не входит.
- **GitHub Actions** (`actions/checkout`, `actions/cache`, `actions/upload-artifact`, все v4,
  лицензия MIT) — `.github/workflows/build.yml`. Только сборка, в выпуск не входит.
- `docs/lfo-shapes.png` — схема формы LFO; сторонних материалов в ней нет.
- `tools/webflash/firmware/OBOD.bin` — собранный образ прибора, не сторонний файл.

## Как восстанавливаются зависимости

Версии зафиксированы в `dependencies.lock` (строка 191 — `manifest_hash`, строка 192 —
`target: esp32s3`, строка 193 — версия формата 3.0.0). Каталоги `managed_components/` и
`build/` перечислены в `.gitignore`, поэтому в исходный архив не попадают. Каждый компонент
несёт файл `.component_hash` с хешем содержимого, по которому менеджер компонентов проверяет
целостность. Наши правки в загруженные компоненты не вносились.

---

## Помощь в подготовке

Раздел не про лицензии: всё выше — про чужой код и его условия, а здесь про то, как
готовился выпуск.

Подготовка кода, документации и выпуска велась вместе с ассистентом DeepSeek Harness
(модель deepseek-v4.1-flash-20260910) под руководством автора проекта. Исключительные
права на код и документы проекта остаются у автора; лицензия проекта — MIT,
`LICENSE`.
