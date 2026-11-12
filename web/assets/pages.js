/*
 * Список страниц документации OBOD.
 *
 * Единственное место, где перечислены страницы, их порядок, заголовки и вкладки. Из него
 * собирается всё навигационное: дерево слева, хлебные крошки, переходы «Назад / Далее» и
 * оглавление «На этой странице». Поэтому разойтись между собой они не могут — достаточно
 * поправить этот файл. Подробнее — web/README.md.
 *
 * Разделы документации совпадают с разделами прибора: «Тело», «Голос», «Время»,
 * «Перкуссия», «Рэк», «Микшеры», «Пресеты», «Настройки». Поэтому каждый узел прибора
 * описан ровно в одном месте — на странице своего раздела, — и отдельного раздела «Звук»
 * поверх них нет. Описание раздела стоит первым, за ним идут его экраны вкладками.
 *
 * Поля записи:
 *
 *   file    имя файла страницы: он же адрес, он же ключ записи;
 *   title   заголовок в дереве, крошках и переходах;
 *   family  имя раздела из REF (web/assets/ref.js): по нему страница раздела находит
 *           свои данные и раскладывает их по вкладкам экранов;
 *   tabs    вкладки страницы: id — то, что попадает в адрес после #, title — подпись;
 *   parent  file родительской страницы, если страницу нужно вложить в дерево; сейчас не
 *           используется: все разделы прибора стоят верхним уровнем.
 *
 * Порядок записей — порядок чтения: он же порядок переходов «Назад / Далее».
 * Файл подключается обычным <script src>: страницы открываются по file://, а модули
 * браузер из соседних файлов в этом режиме не отдаёт.
 */
var DOCS_PAGES = [
  { file: 'index.html', title: 'Обзор' },

  { file: 'start.html', title: 'С чего начать' },

  { file: 'ref.html', title: 'Разделы и термины' },

  { file: 'ref-drone.html', title: 'Тело', family: 'Тело', tabs: [
    { id: 'DRONE', title: 'DRONE' },
    { id: 'FLTR',  title: 'DRONE FLTR' },
    { id: 'MOD',   title: 'DRONE MOD' }
  ] },

  { file: 'ref-lead.html', title: 'Голос', family: 'Голос', tabs: [
    { id: 'LEAD', title: 'LEAD' },
    { id: 'TONE', title: 'LEAD TONE' },
    { id: 'ENV',  title: 'LEAD ENV' }
  ] },

  { file: 'ref-time.html', title: 'Время', family: 'Время', tabs: [
    { id: 'SCL',  title: 'SCALE' },
    { id: 'EUCL', title: 'LEAD EUCLID' },
    { id: 'ARP',  title: 'LEAD ARP' }
  ] },

  { file: 'ref-perc.html', title: 'Перкуссия', family: 'Перкуссия', tabs: [
    { id: 'PERC', title: 'PERC' },
    { id: 'PSEQ', title: 'PERC SEQ' }
  ] },

  { file: 'ref-fx.html', title: 'Рэк', family: 'Рэк', tabs: [
    { id: 'CHOR', title: 'CHORUS' },
    { id: 'DLY',  title: 'DELAY' },
    { id: 'REV',  title: 'REVERB' }
  ] },

  { file: 'ref-mix.html', title: 'Микшеры', family: 'Микшеры', tabs: [
    { id: 'MSTR', title: 'MASTER' },
    { id: 'LVLS', title: 'LEVELS' },
    { id: 'REVS', title: 'REV SENDS' },
    { id: 'CHRS', title: 'CHR SENDS' },
    { id: 'DELS', title: 'DEL SENDS' }
  ] },

  { file: 'ref-preset.html', title: 'Пресеты', family: 'Пресеты', tabs: [
    { id: 'PRST', title: 'PRESET' }
  ] },

  { file: 'ref-system.html', title: 'Настройки', family: 'Настройки', tabs: [
    { id: 'SYS', title: 'SYSTEM' },
    { id: 'OPT', title: 'OPTIONS' }
  ] },

  { file: 'ui.html', title: 'Управление' },

  { file: 'flash.html', title: 'Прошивка', tabs: [
    { id: 'browser', title: 'Из браузера' },
    { id: 'command', title: 'Командой' },
    { id: 'lost',    title: 'Что теряется' }
  ] },

  { file: 'changelog.html', title: 'История изменений' },

  { file: 'hw.html', title: 'Железо' },

  { file: 'under.html', title: 'Под капотом' }
];
