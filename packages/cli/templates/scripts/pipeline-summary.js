#!/usr/bin/env bun
'use strict';
/**
 * pipeline-summary.js — renders a "Done / Left / Next Steps / Manual Follow-ups"
 * summary for a spec at CLOSE.
 *
 * Usage: bun pipeline-summary.js --spec-dir <path> [--format markdown|json]
 *
 * Reads:
 *   <spec-dir>/spec.md          (required — exits 1 if missing/unreadable)
 *   .claude/.pipeline-states/<basename(spec-dir)>.json  (optional; fail-open)
 *
 * Output: markdown (default) or JSON ({done,left,nextSteps,followUps}).
 */

const fs = require('node:fs');
const path = require('node:path');
const { SECTIONS } = require(path.join(__dirname, '_lib', 'spec-sections.js'));

// ---------- arg parsing ----------
function parseArgs(argv) {
  const args = { specDir: null, format: 'markdown' };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--spec-dir') args.specDir = argv[++i] || null;
    else if (a === '--format') args.format = argv[++i] || 'markdown';
  }
  return args;
}

function die(msg) {
  process.stderr.write(`pipeline-summary: ${msg}\n`);
  process.exit(1);
}

// ---------- spec parsing ----------
function parseHeader(text) {
  const status = (text.match(/^###\s*Status:\s*(.+)$/m) || [])[1] || '';
  const phase = (text.match(/^###\s*Phase:\s*(.+)$/m) || [])[1] || '';
  let lang = ((text.match(/^###\s*Lang:\s*(.+)$/m) || [])[1] || 'en').trim().toLowerCase();
  // Lang can be "pt | Phase: EXECUTE | …" if on same line, but our regex is per-line.
  // Normalize: take first token if pipe-joined.
  lang = lang.split(/[\s|]/)[0] || 'en';
  if (lang !== 'pt' && lang !== 'en') lang = 'en';
  // Spec name = first heading or "spec"
  const nameMatch = text.match(/^#\s+(.+)$/m);
  const name = nameMatch ? nameMatch[1].trim() : 'spec';
  return { status: status.split('|')[0].trim(), phase: phase.split('|')[0].trim(), lang, name };
}

// Split body into sections keyed by H2 heading text.
function splitSections(text) {
  const lines = text.split(/\r?\n/);
  const sections = {};
  let current = null;
  let buf = [];
  const flush = () => {
    if (current !== null) sections[current] = buf.join('\n');
  };
  for (const line of lines) {
    const m = line.match(/^##\s+(.+?)\s*$/);
    if (m) {
      flush();
      current = m[1].trim();
      buf = [];
    } else if (current !== null) {
      buf.push(line);
    }
  }
  flush();
  return sections;
}

/**
 * Resolve a section from the raw-keyed `sections` object by canonical key.
 * `splitSections` keys by literal heading text, so a PT spec stores its
 * Acceptance Criteria under "Critérios de Aceitação". This walks the canonical
 * key's accepted variants (EN first, then aliases, then PT) and returns the
 * first match — keeping EN behavior identical while also recognizing PT.
 *
 * @param {Record<string,string>} sections — output of splitSections.
 * @param {string} key — a canonical key present in SECTIONS.
 * @returns {string|undefined}
 */
function getSection(sections, key) {
  const variants = SECTIONS[key];
  if (!variants) return undefined;
  for (const name of variants) {
    if (Object.prototype.hasOwnProperty.call(sections, name)) {
      return sections[name];
    }
  }
  return undefined;
}

function parseAC(section) {
  if (!section) return [];
  const out = [];
  const lines = section.split(/\r?\n/);
  for (const raw of lines) {
    const m = raw.match(/^-\s*\[([ xX])\]\s*AC-(\d+):\s*(.*?)(?:\s+—\s+Command:\s*`(.+?)`)?\s*$/);
    if (m) {
      out.push({
        done: m[1].toLowerCase() === 'x',
        id: `AC-${m[2]}`,
        text: m[3].trim(),
        command: m[4] || null,
      });
    }
  }
  return out;
}

function parseBullets(section) {
  if (!section) return [];
  const out = [];
  for (const raw of section.split(/\r?\n/)) {
    const m = raw.match(/^\s*-\s+(.+?)\s*$/);
    if (m && !/^\[[ xX]\]/.test(m[1])) out.push(m[1].trim());
    else if (m) {
      // checkbox bullet: strip "[ ] " / "[x] " prefix
      const cb = m[1].replace(/^\[[ xX]\]\s*/, '').trim();
      if (cb) out.push(cb);
    }
  }
  return out;
}

function parseChecklist(section) {
  if (!section) return { total: 0, done: 0 };
  let total = 0;
  let done = 0;
  for (const raw of section.split(/\r?\n/)) {
    const m = raw.match(/^\s*-\s*\[([ xX])\]/);
    if (m) {
      total++;
      if (m[1].toLowerCase() === 'x') done++;
    }
  }
  return { total, done };
}

function parseFiles(section) {
  if (!section) return [];
  const out = [];
  for (const raw of section.split(/\r?\n/)) {
    const m = raw.match(/^\s*-\s+`?([^\s`]+)`?/);
    if (m && /[\\/.]/.test(m[1])) out.push(m[1]);
  }
  return out;
}

// ---------- heuristics ----------
function followUpsFromFiles(files, lang) {
  const hits = [];
  const seen = new Set();
  const add = (key, msg) => {
    if (!seen.has(key)) {
      seen.add(key);
      hits.push(msg);
    }
  };
  const L = lang === 'pt';
  for (const f of files) {
    const lower = f.toLowerCase();
    if (/\.env/.test(lower) || /(^|[\\/])env([\\/]|$)/.test(lower)) {
      add('env', L
        ? 'Adicionar novas variáveis em `.env.example` + cofre de secrets'
        : 'Add new env vars to `.env.example` + secret manager');
    }
    if (/migration/.test(lower) || /\.sql$/.test(lower)) {
      add('migration', L
        ? 'Rodar migration em staging antes de prod'
        : 'Run migration on staging before prod');
    }
    if (/\.schema\./.test(lower) || /schema\.rs$/.test(lower) || /\.prisma$/.test(lower)) {
      add('schema', L
        ? 'Regerar tipos do ORM / atualizar entity-registry'
        : 'Regenerate ORM types / refresh entity-registry');
    }
    if (/docker-compose/.test(lower)) {
      add('docker', L
        ? 'Rebuildar containers locais antes de pushar'
        : 'Rebuild containers locally before pushing');
    }
  }
  return hits;
}

// ---------- render ----------
function render(model, lang) {
  const L = lang === 'pt';
  const labels = L
    ? { done: "## Feito", left: "## Falta", next: "## Próximos Passos", follow: "## Follow-ups Manuais", nothing: "Nada pendente." }
    : { done: "## What's Done", left: "## What's Left", next: "## Next Steps", follow: "## Manual Follow-ups", nothing: "Nothing pending." };

  const out = [];

  // Done
  out.push(labels.done);
  for (const line of model.done) out.push(`- ${line}`);
  out.push('');

  // Left
  out.push(labels.left);
  if (model.left.length === 0) out.push(`- ${labels.nothing}`);
  else for (const line of model.left) out.push(`- ${line}`);
  out.push('');

  // Next Steps (numbered)
  out.push(labels.next);
  model.nextSteps.forEach((s, i) => out.push(`${i + 1}. ${s}`));
  if (model.nextSteps.length === 0) out.push(`1. ${labels.nothing}`);
  out.push('');

  // Manual Follow-ups (omit section entirely if empty)
  if (model.followUps.length > 0) {
    out.push(labels.follow);
    for (const line of model.followUps) out.push(`- ${line}`);
    out.push('');
  }

  return out.join('\n').replace(/\n+$/, '\n');
}

// ---------- build model ----------
function buildModel({ header, sections, state, lang }) {
  const L = lang === 'pt';
  const acList = parseAC(getSection(sections, 'acceptanceCriteria'));
  const acDone = acList.filter(a => a.done);
  const acFailed = acList.filter(a => !a.done);
  const concerns = parseBullets(getSection(sections, 'concerns') || sections['Concerns Surfaced']);
  // The close-out checklist: EN specs use "## Checklist" specifically (a spec
  // may also carry a separate "## Tasks" — don't let it shadow). PT specs
  // collapse both onto "## Tarefas".
  const checklist = parseChecklist(sections['Checklist'] || sections['Tarefas']);
  const files = parseFiles(getSection(sections, 'files'));

  // Done lines
  const done = [];
  done.push(L
    ? `Spec: ${header.name} (Status: ${header.status || 'unknown'})`
    : `Spec: ${header.name} (Status: ${header.status || 'unknown'})`);
  if (checklist.total > 0) {
    done.push(L
      ? `Checklist: ${checklist.done}/${checklist.total} passos completos`
      : `Checklist: ${checklist.done}/${checklist.total} steps completed`);
  }
  if (acList.length > 0) {
    done.push(L
      ? `AC aprovados: ${acDone.length}/${acList.length}`
      : `AC passed: ${acDone.length}/${acList.length}`);
  }
  if (files.length > 0) {
    done.push(L
      ? `Arquivos tocados: ${files.length}`
      : `Files touched: ${files.length}`);
  }

  // Left lines
  const left = [];
  for (const ac of acFailed) {
    const cmdSuffix = ac.command ? ` — Command: \`${ac.command}\`` : '';
    left.push(`${ac.id}: ${ac.text}${cmdSuffix}`);
  }
  for (const c of concerns) left.push(L ? `Concern: ${c}` : `Concern: ${c}`);
  const deferred = Array.isArray(state.metrics && state.metrics.deferred) ? state.metrics.deferred : [];
  const partial = Array.isArray(state.metrics && state.metrics.partial) ? state.metrics.partial : [];
  const escalations = Array.isArray(state.escalations) ? state.escalations : [];
  for (const d of deferred) left.push(L ? `Deferred: ${formatStateItem(d)}` : `Deferred: ${formatStateItem(d)}`);
  for (const p of partial) left.push(L ? `Partial: ${formatStateItem(p)}` : `Partial: ${formatStateItem(p)}`);
  for (const e of escalations) left.push(L ? `Escalation: ${formatStateItem(e)}` : `Escalation: ${formatStateItem(e)}`);

  // Next Steps
  const nextSteps = [];
  if (acFailed.length > 0) {
    const first = acFailed[0];
    nextSteps.push(L
      ? `Rerodar AC reprovado (${first.id})${first.command ? `: \`${first.command}\`` : ''}`
      : `Rerun failing AC (${first.id})${first.command ? `: \`${first.command}\`` : ''}`);
  }
  if (concerns.length > 0) {
    nextSteps.push(L
      ? 'Resolver concerns acumulados antes de fechar'
      : 'Resolve outstanding concerns before closing');
  }
  if (acFailed.length === 0 && concerns.length === 0) {
    // happy path: commit + push + PR
    nextSteps.push(L ? 'Rodar `git add` nos arquivos modificados' : 'Run `git add` on modified files');
    nextSteps.push(L ? 'Criar commit (`git commit -m "..."`)' : 'Create commit (`git commit -m "..."`)');
    nextSteps.push(L ? 'Push para o remoto (`git push`)' : 'Push to remote (`git push`)');
    nextSteps.push(L ? 'Abrir PR e solicitar review' : 'Open PR and request review');
  } else {
    nextSteps.push(L
      ? 'Revalidar suite local (`bun test` ou comando do projeto)'
      : 'Re-run local test suite (`bun test` or project command)');
    nextSteps.push(L
      ? 'Atualizar checklist na spec após cada correção'
      : 'Update spec checklist after each fix');
  }
  // cap to 5
  if (nextSteps.length > 5) nextSteps.length = 5;

  // Manual follow-ups
  const followUps = followUpsFromFiles(files, lang);

  return { done, left, nextSteps, followUps };
}

function formatStateItem(item) {
  if (item == null) return '';
  if (typeof item === 'string') return item;
  if (typeof item === 'object') {
    if (item.reason && item.id) return `${item.id}: ${item.reason}`;
    if (item.reason) return item.reason;
    if (item.id) return item.id;
    return JSON.stringify(item);
  }
  return String(item);
}

// ---------- main ----------
function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.specDir) die('missing --spec-dir flag');

  const specFile = path.join(args.specDir, 'spec.md');
  let specText;
  try {
    specText = fs.readFileSync(specFile, 'utf8');
  } catch (err) {
    die(`cannot read ${specFile}: ${err.message}`);
  }

  const header = parseHeader(specText);
  const sections = splitSections(specText);

  // pipeline-state (fail-open)
  let state = {};
  try {
    const specBase = path.basename(path.resolve(args.specDir));
    const stateFile = path.join(process.cwd(), '.claude', '.pipeline-states', `${specBase}.json`);
    if (fs.existsSync(stateFile)) {
      state = JSON.parse(fs.readFileSync(stateFile, 'utf8')) || {};
    }
  } catch (_) {
    state = {};
  }

  const model = buildModel({ header, sections, state, lang: header.lang });

  if (args.format === 'json') {
    process.stdout.write(JSON.stringify(model, null, 2) + '\n');
    return;
  }

  process.stdout.write(render(model, header.lang));
}

if (require.main === module) {
  main();
}

module.exports = { parseArgs, parseHeader, splitSections, parseAC, parseBullets, parseChecklist, parseFiles, followUpsFromFiles, buildModel, render };
