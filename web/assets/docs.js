/*
 * Оболочка документации OBOD.
 *
 * Разметка у всех страниц одна и та же, отличается только содержимое, поэтому вся
 * навигация собирается здесь, из списка страниц web/assets/pages.js:
 *
 *   — дерево слева: разделы прибора верхним уровнем, под каждым его экраны; раскрытие по
 *     клику, текущий пункт подсвечен, раскрытые ветки запоминаются в localStorage и
 *     переживают переходы;
 *   — хлебные крошки сверху;
 *   — переходы «Назад / Далее» снизу;
 *   — оглавление «На этой странице» справа;
 *   — вкладки внутри страницы: и те, что перечислены в списке страниц, и вложенные
 *     в экран раздела («Ручки», «Списочные значения», «Действия»);
 *   — дерево на узком экране (меньше 1000 пикселей): выдвижная панель по кнопке.
 *
 * Разделы документации совпадают с разделами прибора, поэтому вложенности в дереве всего
 * две ступени: раздел и его экраны. Поле parent в списке страниц поддерживается, но сейчас
 * не используется.
 *
 * Адрес страницы — имя файла; вкладка — то, что стоит после #. У страницы раздела адрес
 * вкладки — метка экрана, у вложенной вкладки — «метка.подвкладка», например
 * ref-preset.html#PRST.lists. Поэтому ссылка на конкретный экран и на конкретную вкладку
 * внутри него — обычная ссылка, которую можно скопировать.
 *
 * Все страницы открываются прямо с диска по file://: ни сборки, ни сервера, ни модулей —
 * браузер в этом режиме не отдаёт модули из соседних файлов, поэтому файлы подключены
 * обычным <script src>.
 */
(function () {
  'use strict';

  var STORE = 'obod.docs.open';
  var pages = window.DOCS_PAGES || [];

  var here = currentFile();
  var openState = readOpen();
  var ancestors = ancestorSet(here);
  var spy = null;

  /* ---------- мелкие помощники ---------- */

  function currentFile() {
    var path = window.location.pathname || '';
    var file = path.substring(path.lastIndexOf('/') + 1);
    return file || 'index.html';
  }

  function rawHash() {
    var h = window.location.hash || '';
    return h.charAt(0) === '#' ? decodeURIComponent(h.slice(1)) : '';
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) { e.className = cls; }
    if (text !== undefined && text !== null) { e.textContent = text; }
    return e;
  }

  function findPage(file) {
    for (var i = 0; i < pages.length; i++) {
      if (pages[i].file === file) { return pages[i]; }
    }
    return null;
  }

  function childrenOf(file) {
    return pages.filter(function (p) { return p.parent === file; });
  }

  function directChild(parent, cls) {
    if (!parent) { return null; }
    for (var i = 0; i < parent.children.length; i++) {
      var c = parent.children[i];
      if (c.classList && c.classList.contains(cls)) { return c; }
    }
    return null;
  }

  function ancestorSet(file) {
    var seen = {};
    var page = findPage(file);
    while (page && page.parent) {
      seen[page.parent] = true;
      page = findPage(page.parent);
    }
    return seen;
  }

  /* ---------- раскрытые ветки ---------- */

  var memory = null;

  function readOpen() {
    try {
      var raw = window.localStorage.getItem(STORE);
      return raw ? JSON.parse(raw) : {};
    } catch (e) {
      memory = memory || {};
      return memory;
    }
  }

  function writeOpen() {
    try {
      window.localStorage.setItem(STORE, JSON.stringify(openState));
    } catch (e) {
      memory = openState;
    }
  }

  function setOpen(file, open) {
    if (open) { openState[file] = true; } else { delete openState[file]; }
    writeOpen();
  }

  /* ---------- дерево ---------- */

  function renderTree() {
    var host = document.getElementById('tree');
    if (!host) { return; }
    host.textContent = '';
    var root = el('ul', 'tree');
    pages.filter(function (p) { return !p.parent; }).forEach(function (page) {
      root.appendChild(treeBranch(page));
    });
    host.appendChild(root);
  }

  function treeBranch(page) {
    var kids = childrenOf(page.file);
    var tabs = page.tabs || [];
    var hasKids = kids.length > 0 || tabs.length > 0;
    var open = openState[page.file] === true || ancestors[page.file] === true || page.file === here;

    var li = el('li', 'node' + (hasKids ? ' has-kids' : '') + (open ? ' open' : '') +
      (page.file === here ? ' on' : ''));

    var row = el('div', 'row');
    row.appendChild(caret(li, page, hasKids, open));
    row.appendChild(nodeLink(page, page.file === here));
    li.appendChild(row);

    if (hasKids) {
      var box = el('ul', 'kids');
      kids.forEach(function (kid) { box.appendChild(treeBranch(kid)); });
      tabs.forEach(function (tab) { box.appendChild(treeTab(page, tab)); });
      box.hidden = !open;
      li.appendChild(box);
    }

    return li;
  }

  function caret(li, page, hasKids, open) {
    if (!hasKids) {
      var blank = el('span', 'tw blank');
      blank.setAttribute('aria-hidden', 'true');
      return blank;
    }

    var button = el('button', 'tw');
    button.type = 'button';
    button.setAttribute('aria-expanded', open ? 'true' : 'false');
    button.setAttribute('aria-label', 'Раскрыть раздел «' + page.title + '»');
    button.addEventListener('click', function () {
      var next = !li.classList.contains('open');
      li.classList.toggle('open', next);
      var box = directChild(li, 'kids');
      if (box) { box.hidden = !next; }
      button.setAttribute('aria-expanded', next ? 'true' : 'false');
      setOpen(page.file, next);
    });
    return button;
  }

  function nodeLink(page, current) {
    if (current) {
      var span = el('span', 'here', page.title);
      span.setAttribute('aria-current', 'page');
      return span;
    }
    var a = el('a', null, page.title);
    a.setAttribute('href', page.file);
    return a;
  }

  /*
   * Лист дерева — вкладка страницы. Подсвечен тот, что открыт сейчас: текущая страница
   * плюс активная вкладка. Поэтому при переходе по экранам справочника дерево показывает
   * место, а не просто семейство.
   */
  function treeTab(page, tab) {
    var href = page.file + '#' + tab.id;
    var top = topTabset();
    var on = page.file === here && !!top && activeTabOf(top) === tab.id;

    var li = el('li', 'leaf' + (on ? ' on' : ''));
    var row = el('div', 'row');
    var blank = el('span', 'tw blank');
    blank.setAttribute('aria-hidden', 'true');
    row.appendChild(blank);

    if (on) {
      var span = el('span', 'here', tab.title);
      span.setAttribute('aria-current', 'true');
      row.appendChild(span);
    } else {
      var a = el('a', null, tab.title);
      a.setAttribute('href', href);
      row.appendChild(a);
    }

    if (tab.tag) { row.appendChild(el('span', 'tag', tab.tag)); }

    li.appendChild(row);
    return li;
  }

  /* ---------- крошки ---------- */

  function renderCrumbs() {
    var host = document.getElementById('crumbs');
    if (!host) { return; }
    host.textContent = '';

    var chain = [];
    var page = findPage(here);
    while (page) {
      chain.unshift(page);
      page = page.parent ? findPage(page.parent) : null;
    }

    var ol = el('ol');
    var start = document.createElement('li');
    var home = el('a', null, 'OBOD');
    home.setAttribute('href', 'index.html');
    start.appendChild(home);
    ol.appendChild(start);

    chain.forEach(function (item, i) {
      var li = document.createElement('li');
      if (i === chain.length - 1) {
        var span = el('span', null, item.title);
        span.setAttribute('aria-current', 'page');
        li.appendChild(span);
      } else {
        var a = el('a', null, item.title);
        a.setAttribute('href', item.file);
        li.appendChild(a);
      }
      ol.appendChild(li);
    });

    var tab = activeTabTitle();
    if (tab) {
      var li = el('li');
      var span = el('span', null, tab);
      span.setAttribute('aria-current', 'page');
      li.appendChild(span);
      ol.appendChild(li);
    }

    host.appendChild(ol);
  }

  /* ---------- переходы «Назад / Далее» ---------- */

  function renderPager() {
    var host = document.getElementById('pager');
    if (!host) { return; }
    host.textContent = '';

    var i = -1;
    for (var k = 0; k < pages.length; k++) {
      if (pages[k].file === here) { i = k; }
    }
    if (i < 0) { return; }

    if (i > 0) { host.appendChild(pagerLink(pages[i - 1], 'Назад', false)); }
    if (i < pages.length - 1) { host.appendChild(pagerLink(pages[i + 1], 'Далее', true)); }

    if (!host.children.length) { host.hidden = true; }
  }

  function pagerLink(page, label, next) {
    var a = el('a', next ? 'next' : 'prev');
    a.setAttribute('href', page.file);
    a.appendChild(el('span', 'lbl', label));
    a.appendChild(el('span', 'ttl', page.title));
    return a;
  }

  /* ---------- вкладки ---------- */

  function topTabset() {
    return document.querySelector('[data-tabset="screens"]') ||
           document.querySelector('[data-tabset="pages"]');
  }

  function visiblePanel(set) {
    var panels = directChild(set, 'panels');
    if (!panels) { return null; }
    for (var i = 0; i < panels.children.length; i++) {
      if (!panels.children[i].hidden) { return panels.children[i]; }
    }
    return null;
  }

  function activeTabOf(set) {
    var buttons = directChild(set, 'tabs');
    if (!buttons) { return null; }
    for (var i = 0; i < buttons.children.length; i++) {
      if (buttons.children[i].getAttribute('aria-selected') === 'true') {
        return buttons.children[i].getAttribute('data-tab');
      }
    }
    return null;
  }

  function activate(set, id) {
    var buttons = directChild(set, 'tabs');
    var panels = directChild(set, 'panels');
    if (!buttons || !panels || !id) { return false; }

    var found = false;
    for (var i = 0; i < buttons.children.length; i++) {
      var button = buttons.children[i];
      var on = button.getAttribute('data-tab') === id;
      button.setAttribute('aria-selected', on ? 'true' : 'false');
      button.setAttribute('tabindex', on ? '0' : '-1');
      if (on) { found = true; }
    }
    for (var j = 0; j < panels.children.length; j++) {
      panels.children[j].hidden = panels.children[j].getAttribute('data-panel') !== id;
    }
    return found;
  }

  function setupTabs(root) {
    var sets = root.querySelectorAll('.tabset');
    Array.prototype.forEach.call(sets, function (set) {
      var buttons = directChild(set, 'tabs');
      if (!buttons) { return; }
      Array.prototype.forEach.call(buttons.children, function (button) {
        button.addEventListener('click', function () {
          if (activate(set, button.getAttribute('data-tab'))) {
            writeHash(hashFor(set, button.getAttribute('data-tab')));
          }
        });
      });
    });

    /* Кнопка «N позиций» в списке ручек переключает вкладку того же экрана. */
    Array.prototype.forEach.call(root.querySelectorAll('[data-sub]'), function (button) {
      button.addEventListener('click', function () {
        var set = closestTabset(button);
        var sub = button.getAttribute('data-sub');
        if (set && activate(set, sub)) { writeHash(hashFor(set, sub)); }
      });
    });
  }

  function closestTabset(node) {
    while (node && node !== document) {
      if (node.classList && node.classList.contains('tabset')) { return node; }
      node = node.parentNode;
    }
    return null;
  }

  /*
   * Что писать в адрес. У вкладки страницы это её метка, у вкладки экрана справочника —
   * «метка экрана.метка подвкладки»: так ссылка открывает и экран, и нужную его вкладку.
   */
  function hashFor(set, id) {
    var name = set.getAttribute('data-tabset');
    var screens = document.querySelector('[data-tabset="screens"]');

    if (name === 'parts') {
      var screen = screens ? activeTabOf(screens) : null;
      return (screen ? screen + '.' : '') + id;
    }

    var panel = screens ? visiblePanel(screens) : null;
    var parts = panel ? panel.querySelector('[data-tabset="parts"]') : null;
    var sub = parts ? activeTabOf(parts) : null;
    return id + (sub ? '.' + sub : '');
  }

  function writeHash(value) {
    if (rawHash() === value) { return; }
    try {
      window.history.replaceState(null, '', '#' + value);
      applyHash();
    } catch (e) {
      window.location.hash = value;
    }
  }

  /* Открыть вкладки, названные в адресе, и обновить всё, что от них зависит. */
  function applyHash() {
    var raw = rawHash();
    var pieces = raw.split('.');
    var screenId = pieces[0];
    var subId = pieces[1];

    var screens = document.querySelector('[data-tabset="screens"]');
    var pageset = document.querySelector('[data-tabset="pages"]');
    var top = screens || pageset;

    if (top && screenId) { activate(top, screenId); }

    var panel = screens ? visiblePanel(screens) : null;
    var parts = panel ? panel.querySelector('[data-tabset="parts"]') : null;
    if (parts) { activate(parts, subId || activeTabOf(parts) || 'knobs'); }

    renderTree();
    renderCrumbs();
    renderToc();
    openFoldsAt(raw.split('.')[0]);
  }

  /*
   * Цель ссылки может лежать внутри свёрнутого раздела «Под капотом»: закрытый раздел не
   * отрисован, и браузеру некуда прокручивать. Раскрываем складки на пути к цели и только
   * потом прокручиваем. Имена разделам даёт renderToc, поэтому вызов идёт после него.
   */
  function openFoldsAt(id) {
    var target = id ? document.getElementById(id) : null;
    if (!target) { return; }
    var opened = false;
    var node = target;
    while (node && node !== document.documentElement) {
      if (node.tagName === 'DETAILS' && !node.open) { node.open = true; opened = true; }
      node = node.parentElement;
    }
    if (opened) { target.scrollIntoView(); }
  }

  function activeTabTitle() {
    var page = findPage(here);
    if (!page || !page.tabs || !page.tabs.length) { return null; }
    var top = topTabset();
    var id = top ? activeTabOf(top) : null;
    if (!id) { return null; }
    for (var i = 0; i < page.tabs.length; i++) {
      if (page.tabs[i].id === id) { return page.tabs[i].title; }
    }
    return null;
  }

  /* ---------- оглавление «На этой странице» ---------- */

  var TRANSLIT = {
    а: 'a', б: 'b', в: 'v', г: 'g', д: 'd', е: 'e', ё: 'e', ж: 'zh', з: 'z', и: 'i',
    й: 'y', к: 'k', л: 'l', м: 'm', н: 'n', о: 'o', п: 'p', р: 'r', с: 's', т: 't',
    у: 'u', ф: 'f', х: 'h', ц: 'c', ч: 'ch', ш: 'sh', щ: 'sch', ъ: '', ы: 'y', ь: '',
    э: 'e', ю: 'yu', я: 'ya'
  };

  function slug(text) {
    var out = '';
    text = (text || '').toLowerCase();
    for (var i = 0; i < text.length; i++) {
      var ch = text.charAt(i);
      if (TRANSLIT[ch] !== undefined) { out += TRANSLIT[ch]; }
      else if (ch >= 'a' && ch <= 'z') { out += ch; }
      else if (ch >= '0' && ch <= '9') { out += ch; }
      else { out += '-'; }
    }
    out = out.replace(/-+/g, '-').replace(/^-|-$/g, '');
    return out || 'razdel';
  }

  function headingId(heading, used) {
    if (heading.id) { return heading.id; }
    var base = slug(heading.textContent);
    var id = base;
    var n = 2;
    while (used[id] || document.getElementById(id)) { id = base + '-' + n; n++; }
    used[id] = true;
    heading.id = id;
    return id;
  }

  /*
   * Заголовки того, что сейчас видно: скрытые вкладки не в счёт. Второй довод говорит,
   * какие именно заголовки нужны: false — стоящие вне вкладок (описание раздела, оно
   * видно на любой вкладке), true — внутри вкладок, undefined — все подряд. На странице
   * без вкладок в оглавление идёт и сам заголовок страницы, иначе у страницы из одного
   * раздела оглавление осталось бы пустым.
   */
  function visibleHeadings(withTitle, inside) {
    var scope = document.getElementById('main');
    if (!scope) { return []; }
    var list = Array.prototype.filter.call(scope.querySelectorAll('h2, h3'), function (h) {
      if (h.closest('[hidden]')) { return false; }
      if (inside === undefined) { return true; }
      return !!h.closest('[data-panel]') === inside;
    });
    if (withTitle) {
      var title = scope.querySelector('h1');
      if (title) { list.unshift(title); }
    }
    return list;
  }

  function renderToc() {
    var host = document.getElementById('toc-list');
    if (!host) { return; }
    host.textContent = '';
    if (spy) { spy.disconnect(); spy = null; }

    var used = {};
    var links = [];
    var page = findPage(here);
    var tabs = page && page.tabs ? page.tabs : [];
    var top = topTabset();
    var activeTab = top ? activeTabOf(top) : null;

    function add(li, heading) {
      var id = headingId(heading, used);
      var a = el('a', null, heading.textContent);
      a.setAttribute('href', '#' + id);
      li.appendChild(a);
      links.push({ link: a, target: heading });
    }

    /*
     * Описание раздела: его заголовки стоят вне вкладок и видны при любой из них, поэтому
     * идут первыми. У страницы без вкладок этот список собирается ниже целиком, вместе с
     * заголовком страницы.
     */
    if (tabs.length) {
      visibleHeadings(false, false).forEach(function (heading) {
        var li = el('li');
        add(li, heading);
        host.appendChild(li);
      });
    }

    tabs.forEach(function (tab) {
      var li = el('li');
      var a = el('a', null, tab.title);
      a.setAttribute('href', page.file + '#' + tab.id);
      if (tab.id === activeTab) { a.className = 'on'; }
      li.appendChild(a);
      host.appendChild(li);

      if (tab.id !== activeTab) { return; }

      /*
       * У страницы семейства внутри экрана есть свои вкладки: «Ручки», «Списочные
       * значения» и «Действия». В оглавлении они стоят подзаголовками экрана, и ссылка
       * на них открывает сразу и экран, и нужную вкладку внутри него.
       */
      if (page.family && typeof Ref !== 'undefined' && Ref.subtabs) {
        Ref.subtabs.forEach(function (sub) {
          var item = el('li', 'lvl3');
          var link = el('a', null, sub[1]);
          link.setAttribute('href', '#' + tab.id + '.' + sub[0]);
          item.appendChild(link);
          host.appendChild(item);
        });
      }

      visibleHeadings(false, true).forEach(function (heading) {
        var sub = el('li', 'lvl3');
        add(sub, heading);
        host.appendChild(sub);
      });
    });

    if (!tabs.length) {
      var headings = visibleHeadings(true);
      headings.forEach(function (heading) {
        var li = el('li');
        add(li, heading);
        host.appendChild(li);
      });
      if (!headings.length) {
        host.appendChild(el('li', 'toc-empty', 'На этой странице нет разделов.'));
      }
    }

    if (typeof IntersectionObserver !== 'function' || !links.length) { return; }

    spy = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (!entry.isIntersecting) { return; }
        links.forEach(function (item) {
          item.link.classList.toggle('on', item.target === entry.target);
        });
      });
    }, { rootMargin: '-64px 0px -70% 0px' });

    links.forEach(function (item) { spy.observe(item.target); });
  }

  /* ---------- выдвижное дерево на узком экране ---------- */

  function setupDrawer() {
    var toggle = document.querySelector('.nav-toggle');
    var close = document.querySelector('.nav-close');
    var backdrop = document.querySelector('.backdrop');

    function set(open) {
      document.body.classList.toggle('nav-open', open);
      if (toggle) { toggle.setAttribute('aria-expanded', open ? 'true' : 'false'); }
      if (backdrop) { backdrop.hidden = !open; }
    }

    if (toggle) {
      toggle.addEventListener('click', function () {
        set(!document.body.classList.contains('nav-open'));
      });
    }
    if (close) { close.addEventListener('click', function () { set(false); }); }
    if (backdrop) { backdrop.addEventListener('click', function () { set(false); }); }

    document.addEventListener('keydown', function (e) {
      if (e.key === 'Escape') { set(false); }
    });

    document.addEventListener('click', function (e) {
      var a = e.target && e.target.closest ? e.target.closest('a') : null;
      if (a && a.closest('.sidebar')) { set(false); }
    });
  }

  /* ---------- сборка ---------- */

  function setupRef() {
    if (typeof Ref === 'undefined') { return; }

    var family = document.querySelector('[data-ref="family"]');
    if (family) { Ref.page(family, family.getAttribute('data-family')); }

    var list = document.querySelector('[data-ref="list"]');
    if (list) {
      Ref.list(list, {
        href: function (name) {
          var found = pages.filter(function (p) { return p.family === name; })[0];
          return found ? found.file : 'ref.html';
        }
      });
    }
  }

  function init() {
    if (!findPage(here)) { here = 'index.html'; }

    setupRef();
    renderTree();
    renderCrumbs();
    renderPager();
    setupTabs(document);
    setupDrawer();
    applyHash();

    window.addEventListener('hashchange', applyHash);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
