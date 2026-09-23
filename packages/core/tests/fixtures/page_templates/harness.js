// Roda um template de página do Mustard no Node, como o claude.ai o abriria,
// e diz em JSON o que a página mostra. É o apoio do teste dos templates
// (`platform::page_templates`): o teste escreve na entrada padrão o HTML
// preenchido, o banco de dados da página e os passos (ler a tela, buscar,
// trocar de aba, abrir uma onda pelo gráfico ou pelo endereço, baixar o .md,
// receber uma cópia nova), e lê a resposta na saída.
//
// A página roda com uma imitação pequena do DOM (só o que os templates usam)
// e das capacidades do claude.ai: o banco de dados (`db`), com a leitura em
// páginas, o aviso de mudança e o aviso de falha da escuta (o passo `fail`),
// e o salvar arquivo (`downloads`).
'use strict';
const fs = require('fs');
const vm = require('vm');

const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const errors = [];
process.on('unhandledRejection', (e) => errors.push(String((e && e.stack) || e)));

// ---------------------------------------------------------------------------
// O DOM, só no tamanho que os templates usam
// ---------------------------------------------------------------------------
function htmlText(html) {
  return html.replace(/<[^>]*>/g, '').replace(/&lt;/g, '<').replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, '&');
}
class Node_ {
  constructor() { this.childNodes = []; this.parentNode = null; }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
  get parentElement() { return this.parentNode instanceof Element ? this.parentNode : null; }
  appendChild(c) {
    if (c.parentNode) c.parentNode.removeChild(c);
    c.parentNode = this;
    this.childNodes.push(c);
    return c;
  }
  removeChild(c) {
    const i = this.childNodes.indexOf(c);
    if (i >= 0) this.childNodes.splice(i, 1);
    c.parentNode = null;
    return c;
  }
  remove() { if (this.parentNode) this.parentNode.removeChild(this); }
  get textContent() { return this.childNodes.map((c) => c.textContent).join(''); }
  set textContent(v) {
    this.childNodes.forEach((c) => { c.parentNode = null; });
    this.childNodes = [];
    const s = String(v);
    if (s) this.appendChild(new Text(s));
  }
}
class Text extends Node_ {
  constructor(v) { super(); this.data = String(v); }
  get textContent() { return this.data; }
  set textContent(v) { this.data = String(v); }
}
class Element extends Node_ {
  constructor(tag) { super(); this.tagName = tag.toUpperCase(); this.attrs = new Map(); this.listeners = {}; this._value = ''; this.html = null; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  removeAttribute(k) { this.attrs.delete(k); }
  hasAttribute(k) { return this.attrs.has(k); }
  get id() { return this.getAttribute('id') || ''; }
  get className() { return this.getAttribute('class') || ''; }
  set className(v) { this.setAttribute('class', v); }
  get classList() {
    const el = this;
    const list = () => el.className.split(/\s+/).filter(Boolean);
    return {
      contains: (c) => list().includes(c),
      add: (c) => { if (!list().includes(c)) el.className = list().concat([c]).join(' '); },
      remove: (c) => { el.className = list().filter((x) => x !== c).join(' '); },
      toggle: (c) => { const on = !list().includes(c); el.className = on ? list().concat([c]).join(' ') : list().filter((x) => x !== c).join(' '); return on; },
    };
  }
  get hidden() { return this.hasAttribute('hidden'); }
  set hidden(v) { if (v) this.setAttribute('hidden', ''); else this.removeAttribute('hidden'); }
  get open() { return this.hasAttribute('open'); }
  set open(v) { if (v) this.setAttribute('open', ''); else this.removeAttribute('open'); }
  get value() { return this._value; }
  set value(v) { this._value = String(v); }
  get textContent() { return super.textContent; }
  set textContent(v) { this.html = null; super.textContent = v; }
  get innerHTML() { return this.html === null ? '' : this.html; }
  set innerHTML(v) {
    super.textContent = '';
    this.html = String(v);
    if (this.html) this.appendChild(new Text(htmlText(this.html)));
  }
  addEventListener(type, fn) { (this.listeners[type] = this.listeners[type] || []).push(fn); }
  fire(type, extra) { return (this.listeners[type] || []).map((fn) => fn(Object.assign({ type, target: this, preventDefault() {} }, extra || {}))); }
  click() { this.clicked = true; }
  closest() { return null; }
}
function walk(root, pred, out) {
  out = out || [];
  (root.childNodes || []).forEach((c) => {
    if (c instanceof Element) {
      if (pred(c)) out.push(c);
      walk(c, pred, out);
    }
  });
  return out;
}
const has = (el, cls) => el.className.split(/\s+/).includes(cls);
const one = (root, pred) => walk(root, pred)[0] || null;
const byClass = (root, cls) => one(root, (e) => has(e, cls));
const text = (el) => (el ? el.textContent : null);

const html = input.html;
const catalogJson = (/<script type="application\/json" id="mustard-catalog">([\s\S]*?)<\/script>/.exec(html) || [])[1];
const script = (/<script id="mustard-page">([\s\S]*?)<\/script>/.exec(html) || [])[1];
if (catalogJson === undefined || script === undefined) {
  process.stdout.write(JSON.stringify({ errors: ['the template has no catalog or no page script'] }), () => process.exit(0));
  return;
}
const documentElement = new Element('html');
const head = new Element('head');
const body = new Element('body');
documentElement.appendChild(head);
documentElement.appendChild(body);
const appEl = new Element('div');
appEl.setAttribute('id', 'app');
body.appendChild(appEl);
const catalogEl = new Element('script');
catalogEl.setAttribute('id', 'mustard-catalog');
catalogEl.textContent = catalogJson;
body.appendChild(catalogEl);
const document = {
  documentElement, head, body, title: '',
  createElement: (tag) => new Element(tag),
  // O gráfico das ondas é SVG: o elemento é o mesmo, só o nome muda.
  createElementNS: (ns, tag) => { const el = new Element(tag); el.ns = ns; return el; },
  createTextNode: (s) => new Text(s),
  getElementById: (id) => one(documentElement, (e) => e.getAttribute('id') === id),
};

// ---------------------------------------------------------------------------
// O banco de dados e o salvar arquivo do claude.ai
// ---------------------------------------------------------------------------
const reads = [];
function makeDb(state) {
  const listeners = [];
  const docsOf = (path) => state[path] || [];
  const snap = (id, data) => ({
    id, exists: data !== null && data !== undefined,
    data: () => (data === null || data === undefined ? undefined : JSON.parse(JSON.stringify(data))),
    metadata: { fromCache: false, hasPendingWrites: false },
  });
  function query(path, filters, order, lim) {
    function run() {
      let docs = docsOf(path).filter((d) => filters.every(([f, op, v]) => {
        const x = d.data[f];
        if (op === '>') return typeof x === typeof v && x > v;
        if (op === '==') return x === v;
        throw new Error('the stand-in store does not know the operator ' + op);
      }));
      if (order) {
        const [f, dir] = order;
        docs = docs.slice().sort((a, b) => {
          const x = a.data[f], y = b.data[f];
          if (x === undefined) return 1;
          if (y === undefined) return -1;
          return (x < y ? -1 : x > y ? 1 : 0) * (dir === 'desc' ? -1 : 1);
        });
      } else docs = docs.slice().sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
      if (lim) docs = docs.slice(0, lim);
      const out = docs.map((d) => snap(d.id, d.data));
      return { docs: out, size: out.length, empty: !out.length, docChanges: () => [], metadata: { fromCache: false, hasPendingWrites: false } };
    }
    return {
      where: (f, op, v) => query(path, filters.concat([[f, op, v]]), order, lim),
      orderBy: (f, dir) => query(path, filters, [f, dir || 'asc'], lim),
      limit: (n) => {
        if (!(Number.isInteger(n) && n >= 1 && n <= 1000)) throw new TypeError('limit out of range: ' + n);
        return query(path, filters, order, n);
      },
      get: async () => { const r = run(); reads.push({ path, filters, order, limit: lim, size: r.size }); return r; },
      onSnapshot: (next, onError) => { const l = () => next(run()); listeners.push({ path, run: l, err: onError }); setTimeout(l, 0); return () => {}; },
      doc: (id) => docRef(path + '/' + id),
    };
  }
  function docRef(path) {
    const cut = path.lastIndexOf('/');
    const col = path.slice(0, cut), id = path.slice(cut + 1);
    const find = () => { const d = docsOf(col).find((x) => x.id === id); return snap(id, d ? d.data : null); };
    return {
      id, path,
      get: async () => { reads.push({ path }); return find(); },
      onSnapshot: (next, onError) => { const l = () => next(find()); listeners.push({ path, run: l, err: onError }); setTimeout(l, 0); return () => {}; },
    };
  }
  // notify() reencena toda escuta, como um documento ou uma coleção mudando
  // de verdade; notifyError(path) chama o erro da escuta daquele caminho,
  // como a conexão com o banco caindo no meio da leitura.
  return {
    db: { collection: (p) => query(p, [], null, 0), doc: docRef },
    notify: () => listeners.forEach((x) => x.run()),
    notifyError: (path) => listeners.filter((x) => x.path === path).forEach((x) => x.err && x.err(new Error('a leitura falhou'))),
  };
}
const store = input.db ? makeDb(input.db) : null;
const saves = [];
// `input.downloads === false` imita o claude.ai sem o salvar arquivo; sem o
// campo (a maioria dos testes), a capacidade vem, como no claude.ai de
// verdade.
const hasDownloads = input.downloads !== false;
const downloads = { save: async (req) => { saves.push({ filename: req.filename, data: String(req.data) }); return { status: 'saved' }; } };
const claude = { use: async (name) => (name === 'db' ? (store ? store.db : null) : name === 'downloads' ? (hasDownloads ? downloads : null) : null) };

// O endereço da página: `input.hash` é o `#…` com que ela abre, e o passo
// `hash` troca o endereço e avisa, como o navegador faz.
const location = { hash: input.hash || '' };
const windowListeners = {};
const sandbox = {
  document, console, setTimeout, clearTimeout, URL, Blob, claude, location,
  requestAnimationFrame: (fn) => setTimeout(fn, 0),
  addEventListener: (type, fn) => { (windowListeners[type] = windowListeners[type] || []).push(fn); },
};
sandbox.window = sandbox;
vm.createContext(sandbox);
try {
  vm.runInContext(script, sandbox, { filename: 'page.js' });
} catch (e) {
  errors.push(String((e && e.stack) || e));
}

// ---------------------------------------------------------------------------
// O que a página mostra
// ---------------------------------------------------------------------------
function fieldsOf(el) {
  const dl = one(el, (e) => e.tagName === 'DL');
  if (!dl) return [];
  const out = [];
  for (let i = 0; i + 1 < dl.childNodes.length; i += 2) {
    out.push([dl.childNodes[i].textContent, dl.childNodes[i + 1].textContent, dl.childNodes[i + 1].innerHTML]);
  }
  return out;
}
function tagOf(el, cls) { const t = one(el, (e) => has(e, 'tag') && has(e, cls)); return t ? t.textContent : null; }
// Como `walk`, sem entrar num <details> de dentro: o cartão de um critério
// traz as execuções dele, e o de um ponto traz a resposta, cada qual no seu
// próprio cartão.
function own(root, pred, out) {
  out = out || [];
  (root.childNodes || []).forEach((c) => {
    if (c instanceof Element) {
      if (pred(c)) out.push(c);
      if (c.tagName !== 'DETAILS') own(c, pred, out);
    }
  });
  return out;
}
const ownOne = (root, pred) => own(root, pred)[0] || null;
function ownFields(el) {
  const dl = ownOne(el, (e) => e.tagName === 'DL');
  if (!dl) return [];
  const out = [];
  for (let i = 0; i + 1 < dl.childNodes.length; i += 2) {
    out.push([dl.childNodes[i].textContent, dl.childNodes[i + 1].textContent, dl.childNodes[i + 1].innerHTML]);
  }
  return out;
}
// Um cartão: no alto (`top`, os nomes de classe na ordem) o código, o selo
// do tipo, as marcas, o selo da prova e a data; embaixo o título.
function scrapeItem(el) {
  const summary = el.childNodes[0];
  const top = byClass(summary, 'top');
  const bodyEl = el.childNodes[1];
  const prose = bodyEl ? ownOne(bodyEl, (e) => has(e, 'prose')) : null;
  const runs = bodyEl ? ownOne(bodyEl, (e) => has(e, 'runs')) : null;
  const answer = bodyEl ? ownOne(bodyEl, (e) => has(e, 'answer')) : null;
  const typeTag = top ? top.childNodes.find((c) => has(c, 'tag') && !has(c, 'mark') && !has(c, 'extra')) : null;
  const pill = top ? top.childNodes.find((c) => has(c, 'pill')) : null;
  return {
    code: el.getAttribute('data-code'), type: el.getAttribute('data-type'), anchored: el.getAttribute('id') !== null,
    top: top ? top.childNodes.map((c) => c.className) : [], below: summary.childNodes.map((c) => c.className),
    codeShown: top ? text(byClass(top, 'c')) : null,
    title: text(byClass(summary, 't')), tag: typeTag ? typeTag.textContent : null,
    who: typeTag && has(typeTag, 'who') ? typeTag.textContent : null, mark: tagOf(summary, 'mark'),
    extra: top ? top.childNodes.filter((c) => has(c, 'extra')).map((c) => c.textContent) : [],
    status: pill ? pill.textContent : null, statusClass: pill ? pill.className : null,
    date: text(byClass(summary, 'when')), text: prose ? prose.textContent : '',
    html: prose ? prose.innerHTML : '', fields: bodyEl ? ownFields(bodyEl) : [], hidden: el.hidden, open: el.open,
    runs: runs ? runs.childNodes.filter((c) => c.tagName === 'DETAILS').map(scrapeItem) : [],
    answer: answer ? scrapeItem(answer.childNodes.find((c) => c.tagName === 'DETAILS')) : null,
  };
}
function scrapeTable(t) {
  return { headers: walk(t, (e) => e.tagName === 'TH').map((c) => c.textContent),
    rows: walk(t, (e) => e.tagName === 'TR').filter((r) => r.childNodes[0].tagName === 'TD').map((r) => r.childNodes.map((c) => c.textContent)) };
}
// O texto que está à vista: fora de todo elemento escondido.
function shownText(el) {
  if (el instanceof Text) return el.data;
  if (el.hidden) return '';
  return el.childNodes.map(shownText).join('');
}
function scrapePrompt(d) {
  return { owner: d.getAttribute('data-owner'), summary: text(d.firstChild), text: text(d.lastChild), html: d.lastChild.innerHTML, hidden: d.hidden, open: d.open };
}
function scrapeSpec() {
  const board = document.getElementById('board');
  const byId = (id) => document.getElementById(id);
  const tiles = byId('tiles');
  const now = byId('now');
  const chart = byId('chart');
  const detail = byId('detail');
  const tabs = byId('tabs');
  const phase = byClass(appEl, 'phase');
  const branch = byClass(appEl, 'branch');
  const work = now ? byClass(now, 'work') : null;
  return {
    // A ordem dos blocos do painel, pelo id de cada um.
    blocks: board ? board.childNodes.map((c) => c.getAttribute('id')) : [],
    head: (byId('head') || { childNodes: [] }).childNodes.map((c) => c.tagName + (c.className ? '.' + c.className : '')),
    title: text(one(appEl, (e) => e.tagName === 'H1')),
    phase: phase && !phase.hidden ? phase.textContent : null,
    branch: branch && !branch.hidden ? branch.textContent : null,
    goal: byId('goal') ? { label: text(byClass(byId('goal'), 'lbl')), text: text(byClass(byId('goal'), 'prose')), html: byClass(byId('goal'), 'prose').innerHTML } : null,
    tiles: tiles ? tiles.childNodes.map((x) => ({
      id: x.getAttribute('data-tile'), tag: x.tagName, href: x.getAttribute('href'), key: text(byClass(x, 'k')),
      value: text(byClass(x, 'v')), meter: byClass(x, 'meter') !== null,
      meterWidth: byClass(x, 'meter') ? byClass(x, 'meter').childNodes[0].getAttribute('style') : null,
      lines: x.childNodes.filter((c) => has(c, 's')).map((c) => c.textContent),
    })) : [],
    now: work ? {
      heading: text(one(now, (e) => e.tagName === 'H2')),
      parts: work.childNodes.map((c) => {
        if (has(c, 'grp')) return { kind: 'group', id: c.getAttribute('id'), text: c.textContent };
        if (has(c, 'after')) return { kind: 'after', text: c.textContent };
        const sum = c.childNodes[0];
        return { kind: c.getAttribute('data-kind'), id: c.getAttribute('id'), open: c.open,
          pill: text(byClass(sum, 'pill')), pillClass: byClass(sum, 'pill').className,
          title: text(byClass(sum, 'w-title')), meta: text(byClass(sum, 'w-meta')),
          body: text(c.childNodes[1]), tasks: walk(c.childNodes[1], (e) => e.tagName === 'LI').map((li) => li.textContent),
          files: walk(c.childNodes[1], (e) => e.tagName === 'CODE').map((x) => x.textContent) };
      }),
    } : null,
    chart: chart ? {
      heading: text(one(chart, (e) => e.tagName === 'H2')),
      bars: walk(chart, (e) => e.tagName === 'RECT').map((r) => ({ wave: Number(r.getAttribute('data-wave')), class: r.getAttribute('class'),
        title: text(one(r, (e) => e.tagName === 'TITLE')), height: Number(r.getAttribute('height')), focusable: r.getAttribute('tabindex') === '0' })),
      labels: walk(chart, (e) => e.tagName === 'TEXT').map((x) => x.textContent),
      legend: walk(byClass(chart, 'legend') || chart, (e) => e.tagName === 'I').map((i) => [i.className, i.parentNode.textContent]),
    } : null,
    detail: detail ? {
      wave: detail.getAttribute('data-wave') === null ? null : Number(detail.getAttribute('data-wave')),
      number: text(byClass(detail, 'dn')), meta: text(detail.childNodes[1] || null),
      tasks: walk(detail, (e) => e.tagName === 'OL').flatMap((o) => o.childNodes.map((li) => li.textContent)),
      measures: fieldsOf(detail).map((f) => [f[0], f[1]]),
      prompts: walk(detail, (e) => e.tagName === 'DETAILS' && has(e, 'prompt') && !has(e, 'delivered')).map(scrapePrompt),
      steps: walk(byClass(detail, 'steps') || new Element('x'), (e) => e.tagName === 'LI').map((li) => li.textContent),
      delivered: (() => { const d = one(detail, (e) => has(e, 'delivered')); return d ? text(d.lastChild) : null; })(),
      commit: text(byClass(detail, 'commit')),
      text: detail.textContent,
    } : null,
    findBeforeTabs: board ? board.childNodes.findIndex((c) => c.getAttribute('id') === 'find') + 1 === board.childNodes.findIndex((c) => c.getAttribute('id') === 'tabs') : false,
    tabs: tabs ? tabs.childNodes.map((b) => ({ anchor: b.getAttribute('data-tab'), label: text(byClass(b, 'tl')),
      count: text(one(b, (e) => e.tagName === 'I')), selected: b.getAttribute('aria-selected') === 'true' })) : [],
    sections: walk(byId('panels') || new Element('x'), (e) => e.tagName === 'SECTION' && has(e, 'panel')).map((p) => ({
      id: p.getAttribute('id'), hidden: p.hidden,
      items: p.childNodes.filter((c) => c.tagName === 'DETAILS' && has(c, 'item')).map(scrapeItem),
      tables: p.childNodes.filter((c) => has(c, 'table')).map(scrapeTable),
      more: (() => { const m = p.childNodes.find((c) => has(c, 'more')); return m && !m.hidden ? m.textContent : null; })(),
      empty: (() => { const m = p.childNodes.find((c) => has(c, 'empty-tab')); return m && !m.hidden ? m.textContent : null; })(),
    })),
    // Todo o texto que a página tem, e o que está à vista.
    text: appEl.textContent,
    shown: shownText(appEl),
    classes: walk(appEl, () => true).map((e) => e.className).join(' '),
    tags: walk(appEl, () => true).map((e) => e.tagName).join(' '),
    ids: walk(appEl, (e) => e.getAttribute('id') !== null).map((e) => e.getAttribute('id')),
    state: appEl.getAttribute('data-state'), status: text(document.getElementById('status')),
    statusHidden: document.getElementById('status').hidden,
    search: byId('q') ? byId('q').getAttribute('placeholder') : null,
    download: text(document.getElementById('download')),
    downloadHidden: document.getElementById('download') ? document.getElementById('download').hidden : null,
    headings: walk(appEl, (e) => /^H[1-6]$/.test(e.tagName)).map((e) => e.tagName),
  };
}
function scrapeProject() {
  return {
    state: appEl.getAttribute('data-state'), status: text(document.getElementById('status')),
    statusHidden: document.getElementById('status').hidden, title: text(one(appEl, (e) => e.tagName === 'H1')),
    stages: text(byClass(appEl, 'stages')),
    groups: walk(appEl, (e) => e.tagName === 'DETAILS' && has(e, 'group')).map((g) => ({
      id: g.getAttribute('id'), title: text(byClass(g, 'gt')),
      rows: walk(g, (e) => e.tagName === 'DETAILS' && has(e, 'item')).map((r) => ({
        spec: r.getAttribute('data-spec'), code: text(byClass(r, 'c')), title: text(byClass(r, 't')),
        status: tagOf(r, 'state'), date: text(byClass(r, 'when')), fields: fieldsOf(r),
      })),
    })),
  };
}

// ---------------------------------------------------------------------------
// Os passos
// ---------------------------------------------------------------------------
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(check) {
  for (let waited = 0; waited < 5000; waited += 10) {
    if (check()) return true;
    await pause(10);
  }
  return false;
}
(async () => {
  const results = {};
  const renders = () => Number(appEl.getAttribute('data-renders') || 0);
  for (const step of input.steps) {
    if (step.do === 'wait') {
      if (!(await until(() => appEl.getAttribute('data-state') !== 'loading'))) errors.push('the page never left the loading state');
    } else if (step.do === 'scrape') {
      results[step.as] = input.page === 'project' ? scrapeProject() : scrapeSpec();
    } else if (step.do === 'search') {
      const q = document.getElementById('q');
      q.value = step.value;
      q.fire('input');
    } else if (step.do === 'tab') {
      document.getElementById('tab-' + step.value).fire('click');
    } else if (step.do === 'bar') {
      // Abre uma onda pelo gráfico: um clique, ou o Enter com a barra em foco.
      const bar = one(appEl, (e) => e.tagName === 'RECT' && e.getAttribute('data-wave') === String(step.value));
      if (!bar) errors.push('no bar for wave ' + step.value);
      else if (step.key) bar.fire('keydown', { key: step.key });
      else bar.fire('click');
    } else if (step.do === 'hash') {
      location.hash = '#' + step.value;
      (windowListeners.hashchange || []).forEach((fn) => fn({ type: 'hashchange' }));
    } else if (step.do === 'more') {
      const panel = document.getElementById(step.value);
      const more = panel && panel.childNodes.find((c) => has(c, 'more'));
      if (!more) errors.push('no show-more in ' + step.value);
      else more.childNodes[0].fire('click');
    } else if (step.do === 'download') {
      await Promise.all(document.getElementById('download').fire('click'));
      results[step.as] = saves.length ? saves[saves.length - 1] : null;
    } else if (step.do === 'copy') {
      const before = renders();
      Object.keys(step.set || {}).forEach((path) => {
        const docs = input.db[path] = input.db[path] || [];
        step.set[path].forEach((doc) => {
          const at = docs.findIndex((d) => d.id === doc.id);
          if (at >= 0) docs[at] = doc; else docs.push(doc);
        });
      });
      Object.keys(step.delete || {}).forEach((path) => {
        input.db[path] = (input.db[path] || []).filter((d) => !step.delete[path].includes(d.id));
      });
      store.notify();
      if (!(await until(() => renders() > before))) errors.push('the page did not read the new copy');
    } else if (step.do === 'reads') {
      results[step.as] = reads.slice();
    } else if (step.do === 'fail') {
      store.notifyError(step.path);
    }
  }
  results.errors = errors;
  // A saída vai inteira antes de o processo acabar: num cano, a escrita é
  // assíncrona e o fim do processo cortaria o resto.
  process.stdout.write(JSON.stringify(results), () => process.exit(0));
})();
