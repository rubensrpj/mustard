#!/usr/bin/env node
'use strict';

/**
 * @deprecated Mustard 2.x — Dashboard local em JS sera removido na 3.0.
 * Substituido pelo produto standalone "mustard-dashboard" (Tauri desktop app).
 * Veja: spec `mustard-dashboard-1-0-standalone-tauri` ou docs/mcp-tools.md.
 * DEPRECATED-NOTICE-MUSTARD: keyword grep-able pra AC #6.
 */

process.title = 'mustard-dashboard';

const http = require('http');
const fs = require('fs');
const path = require('path');
const url = require('url');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

const { generatePrdMarkdown, slugify } = require('./dashboard-prd-template.js');
const { renderHtml } = require('./dashboard-ui.js');
const { ENV_CATALOG, isKnownKey, isValidValue, defaultsMap } = require('./dashboard-env-catalog.js');
const { COMMANDS, CATEGORIES } = require('./dashboard-commands-catalog.js');
// Mustard 2.0: EventStore is the primary read path. Returns null on Node (no
// bun:sqlite) or when Mustard repo isn't reachable from cwd — callers fall
// back to legacy filesystem reads (events.jsonl, .pipeline-states/*.metrics.json).
const { getStore: _getEventStore } = require('./_lib/event-store.js');

const PORT_BASE = 7878;
const PORT_RANGE = 100;
const HOST = '127.0.0.1';
const ROOT = process.cwd();

// Deterministic port from ROOT path. Same project → same port; different
// projects → (almost always) different ports. Collisions resolved by probing.
function hashPort(root) {
  const h = crypto.createHash('sha1').update(root).digest();
  const n = h.readUInt32BE(0) % PORT_RANGE;
  return PORT_BASE + n;
}

// Probe /api/info on a port. cb(null, info) on success, cb(err) otherwise.
function probeInfo(port, cb) {
  const req = http.request({
    host: HOST, port, method: 'GET', path: '/api/info', timeout: 1000,
  }, (res) => {
    if (res.statusCode !== 200) { res.resume(); return cb(new Error('status ' + res.statusCode)); }
    let body = '';
    res.setEncoding('utf8');
    res.on('data', (c) => { body += c; if (body.length > 4096) req.destroy(); });
    res.on('end', () => { try { cb(null, JSON.parse(body)); } catch (e) { cb(e); } });
  });
  req.on('error', cb);
  req.on('timeout', () => { req.destroy(new Error('timeout')); });
  req.end();
}
const CLAUDE_DIR = path.join(ROOT, '.claude');
const PID_FILE = path.join(CLAUDE_DIR, '.dashboard.pid');
const PORT_FILE = path.join(CLAUDE_DIR, '.dashboard.port');
const SPEC_DIR = path.join(CLAUDE_DIR, 'spec');
const STATES_DIR = path.join(CLAUDE_DIR, '.pipeline-states');
const EVENTS_FILE = path.join(CLAUDE_DIR, '.harness', 'events.jsonl');

// Monorepo: descobre todos os events.jsonl dos subprojetos (apps/*, packages/*,
// backend/**, etc.) e agrega na hora de servir /api/events. Cache de 10s para
// evitar fs.readdir a cada request.
const IGNORE_DIRS = new Set(['node_modules', '.git', 'dist', 'build', '.next', 'bin', 'obj', '.claude.backup']);
const HARNESS_SCAN_MAX_DEPTH = 5;
let _harnessCache = { files: null, at: 0 };
function discoverHarnessFiles() {
  if (Date.now() - _harnessCache.at < 10000 && _harnessCache.files) return _harnessCache.files;
  const out = [];
  function walk(dir, depth) {
    if (depth > HARNESS_SCAN_MAX_DEPTH) return;
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch (_) { return; }
    for (const ent of entries) {
      if (!ent.isDirectory()) continue;
      if (IGNORE_DIRS.has(ent.name) || ent.name.startsWith('.claude.backup')) continue;
      const sub = path.join(dir, ent.name);
      if (ent.name === '.claude') {
        const f = path.join(sub, '.harness', 'events.jsonl');
        if (fs.existsSync(f)) out.push(f);
        continue;
      }
      if (ent.name.startsWith('.')) continue;
      walk(sub, depth + 1);
    }
  }
  walk(ROOT, 0);
  _harnessCache = { files: out, at: Date.now() };
  return out;
}
const DETECT_CACHE = path.join(CLAUDE_DIR, '.detect-cache.json');
const SETTINGS_FILE = path.join(CLAUDE_DIR, 'settings.json');
const MAX_BODY = 100 * 1024;

function readGitBranch() {
  try {
    const head = fs.readFileSync(path.join(ROOT, '.git', 'HEAD'), 'utf8').trim();
    if (head.startsWith('ref: refs/heads/')) return head.slice('ref: refs/heads/'.length);
    return head.slice(0, 8);
  } catch (_) { return 'unknown'; }
}

function safe(fn) { try { return fn(); } catch (_) { return null; } }

// Memoised EventStore accessor scoped to CLAUDE_DIR. Returns null when the
// SQLite-backed store is unavailable (Node without bun:sqlite, Mustard repo
// not findable, or DB init failure). Every caller MUST handle null by falling
// back to legacy filesystem reads.
let _storeChecked = false;
let _storeRef = null;
function getStore() {
  if (_storeChecked) return _storeRef;
  _storeChecked = true;
  try { _storeRef = _getEventStore(CLAUDE_DIR); } catch (_) { _storeRef = null; }
  return _storeRef;
}

// `--check` smoke test: verify EventStore initialises and returns coherent
// pipelineHealth data, exit without starting the server. Used by AC #8 of the
// Mustard 2.0 Phase 1 spec. Exit codes: 0 ok (store OK or legacy fallback ok),
// 1 nothing readable. Never blocks on UI — runs in <500ms.
if (process.argv.includes('--check')) {
  const out = { ok: false, mode: 'check', store: null, specs: 0, events: null, fallback: false };
  try {
    const store = getStore();
    if (store) {
      out.store = 'event-store';
      try { out.specs = store.specs().length; } catch (_) {}
      try { out.events = store.eventCount(); } catch (_) {}
      // OK if the store initialised cleanly — empty DB is a valid post-init state
      // (migration not yet run). Caller can inspect counts to distinguish.
      out.ok = true;
    } else {
      // Fallback: legacy events.jsonl + spec/active scan. Considered OK if
      // either source has any signal.
      out.fallback = true;
      out.store = 'legacy';
      const activeDir = path.join(SPEC_DIR, 'active');
      if (fs.existsSync(activeDir)) {
        try { out.specs = fs.readdirSync(activeDir, { withFileTypes: true }).filter(d => d.isDirectory()).length; } catch (_) {}
      }
      if (fs.existsSync(EVENTS_FILE)) {
        out.events = 0;
        try {
          const content = fs.readFileSync(EVENTS_FILE, 'utf8');
          out.events = content.split(/\r?\n/).filter(Boolean).length;
        } catch (_) {}
      }
      out.ok = out.specs > 0 || (out.events != null && out.events > 0);
    }
  } catch (e) {
    out.error = e.message;
  }
  try { console.log(JSON.stringify(out)); } catch (_) {}
  process.exit(out.ok ? 0 : 1);
}

// Parse checklist items from markdown text.
// Order of precedence: ## Checklist > ## Tasks (with Wave/Review sub-headings).
// Items carry `wave` (number, 'review', or null) inferred from nearest `### ... Wave N`/`### ... Review` heading.
function parseChecklist(text) {
  const lines = text.split(/\r?\n/);
  const checklistItems = [];
  const taskItems = [];
  let mode = null; // 'checklist' | 'tasks' | null
  let waveCtx = null;
  for (const line of lines) {
    if (/^##\s+Checklist\s*$/i.test(line)) { mode = 'checklist'; waveCtx = null; continue; }
    if (/^##\s+(Tasks|Plano|Plan)\s*$/i.test(line)) { mode = 'tasks'; waveCtx = null; continue; }
    if (mode && /^##\s+/.test(line)) { mode = null; waveCtx = null; continue; }
    if (!mode) continue;
    if (mode === 'tasks' && /^###\s+/.test(line)) {
      const wm = line.match(/wave\s+(\d+)/i);
      if (wm) { waveCtx = Number(wm[1]); continue; }
      if (/review/i.test(line)) { waveCtx = 'review'; continue; }
      waveCtx = null;
      continue;
    }
    const done = line.match(/^\s*-\s+\[x\]\s+(.+)$/i);
    const pending = line.match(/^\s*-\s+\[\s\]\s+(.+)$/i);
    if (!done && !pending) continue;
    const item = { text: (done || pending)[1].trim(), done: !!done, wave: mode === 'tasks' ? waveCtx : null };
    (mode === 'checklist' ? checklistItems : taskItems).push(item);
  }
  const items = checklistItems.length > 0 ? checklistItems : taskItems;
  const total = items.length;
  const doneCount = items.filter(i => i.done).length;
  return { total, done: doneCount, percent: total > 0 ? Math.round((doneCount / total) * 100) : 0, items };
}

// Read pipeline-state JSON (e.g. 2026-05-11-detail-rollout-all.json).
// Returns null if not a wave plan or file absent.
function readWavePlanState(dirName) {
  const p = path.join(STATES_DIR, `${dirName}.json`);
  if (!fs.existsSync(p)) return null;
  const ps = safe(() => JSON.parse(fs.readFileSync(p, 'utf8')));
  if (!ps || !ps.isWavePlan) return null;
  return ps;
}

function waveStatusFor(waveId, ps) {
  if (Array.isArray(ps.failedWaves) && ps.failedWaves.includes(waveId)) return 'failed';
  if (Array.isArray(ps.completedWaves) && ps.completedWaves.includes(waveId)) return 'completed';
  if (ps.currentWave === waveId) return 'current';
  return 'pending';
}

// Parse header fields from spec.md/wave-plan.md. Accepts:
//   "### Status: X | Phase: Y | Scope: Z | Wave: W"  (single-line pipe-separated)
//   "### Phase: EXECUTE"                              (one field per heading line)
const HEADER_KEYS = new Set(['status', 'phase', 'scope', 'wave', 'checkpoint']);
function parseSpecHeader(text) {
  const lines = text.split(/\r?\n/);
  const out = { status: null, phase: null, scope: null, wave: null, checkpoint: null };
  for (let i = 0; i < Math.min(lines.length, 30); i++) {
    const m = lines[i].match(/^###\s*(.+)$/);
    if (!m) continue;
    const parts = m[1].split('|');
    for (const p of parts) {
      const kv = p.match(/^\s*([^:]+?)\s*:\s*(.+?)\s*$/);
      if (!kv) continue;
      const k = kv[1].toLowerCase();
      if (HEADER_KEYS.has(k) && out[k] == null) out[k] = kv[2].trim();
    }
  }
  return out;
}

// Cached read of all harness events (root + monorepo subprojects). Bounded
// tail per file to keep memory predictable — wave activity lookups don't need
// the full history.
let _harnessLinesCache = { lines: null, at: 0 };
const HARNESS_TAIL = 2000;
function readHarnessEventLines() {
  if (Date.now() - _harnessLinesCache.at < 5000 && _harnessLinesCache.lines) return _harnessLinesCache.lines;
  const files = [];
  if (fs.existsSync(EVENTS_FILE)) files.push(EVENTS_FILE);
  for (const f of discoverHarnessFiles()) if (f !== EVENTS_FILE) files.push(f);
  const out = [];
  for (const file of files) {
    let content;
    try { content = fs.readFileSync(file, 'utf8'); } catch (_) { continue; }
    const lines = content.split(/\r?\n/).filter(Boolean);
    const tail = lines.slice(-HARNESS_TAIL);
    for (const line of tail) out.push(line);
  }
  _harnessLinesCache = { lines: out, at: Date.now() };
  return out;
}

// Return Map<waveId, lastTsMillis> for events matching a given spec name.
function harnessActivityByWave(specName) {
  const byWave = new Map();
  if (!specName) return byWave;
  for (const line of readHarnessEventLines()) {
    let ev;
    try { ev = JSON.parse(line); } catch (_) { continue; }
    const pl = ev.payload || {};
    const tgt = ev.spec || pl.spec;
    if (tgt !== specName) continue;
    const w = typeof ev.wave === 'number' ? ev.wave : (typeof pl.wave === 'number' ? pl.wave : null);
    if (w == null) continue;
    const t = Date.parse(ev.ts || ev.timestamp);
    if (isNaN(t)) continue;
    const prev = byWave.get(w);
    if (!prev || t > prev) byWave.set(w, t);
  }
  return byWave;
}

// Extract current wave from wave-plan.md content
function parseCurrentWave(text) {
  const lines = text.split(/\r?\n/);
  // Look for "Wave X/Y" in status line
  for (let i = 0; i < Math.min(lines.length, 10); i++) {
    const m = lines[i].match(/wave:\s*(\d+\/\d+)/i);
    if (m) return m[1];
  }
  // Find last wave heading that isn't completed
  let lastWave = null;
  for (const line of lines) {
    const wm = line.match(/^###\s+Wave\s+(\d+)/i);
    if (wm) lastWave = wm[1];
  }
  return lastWave ? lastWave : null;
}

function listSpecsIn(state, baseDir) {
  const out = [];
  if (!fs.existsSync(baseDir)) return out;
  const entries = safe(() => fs.readdirSync(baseDir, { withFileTypes: true })) || [];
  for (const ent of entries) {
    if (!ent.isDirectory()) continue;
    const dir = path.join(baseDir, ent.name);
    const specPath = path.join(dir, 'spec.md');
    const wavePlanPath = path.join(dir, 'wave-plan.md');

    if (fs.existsSync(specPath)) {
      out.push(parseSpecFile(specPath, ent.name, state));
    } else if (fs.existsSync(wavePlanPath)) {
      // Epic: keep one entry, attach sub-waves as `waves` instead of flattening
      const epic = parseSpecFile(wavePlanPath, ent.name, state);
      epic.isEpic = true;
      epic.waves = [];
      const subEntries = safe(() => fs.readdirSync(dir, { withFileTypes: true })) || [];
      for (const sub of subEntries) {
        if (!sub.isDirectory()) continue;
        const subSpec = path.join(dir, sub.name, 'spec.md');
        if (fs.existsSync(subSpec)) {
          const w = parseSpecFile(subSpec, sub.name, state);
          w.parent = ent.name;
          epic.waves.push(w);
        }
      }
      out.push(epic);
    }
  }
  return out;
}

function parseSpecFile(absPath, name, state) {
  const rel = path.relative(ROOT, absPath).replace(/\\/g, '/');
  const result = {
    name,
    path: rel,
    state,
    status: null,
    phase: null,
    scope: null,
    wave: null,
    checkpoint: null,
    summary: '',
    checklist: { total: 0, done: 0, percent: 0, items: [] },
    lastActivity: null,
    apiCalls: null,
    retries: null,
    currentWave: null,
  };
  let text;
  try { text = fs.readFileSync(absPath, 'utf8'); }
  catch (_) { return result; }

  const lines = text.split(/\r?\n/);
  const hdr = parseSpecHeader(text);
  if (hdr.status) result.status = hdr.status;
  if (hdr.phase) result.phase = hdr.phase;
  if (hdr.scope) result.scope = hdr.scope;
  if (hdr.wave) result.wave = hdr.wave;
  if (hdr.checkpoint) result.checkpoint = hdr.checkpoint;

  const summaryIdx = lines.findIndex(l => /^##\s+Summary\s*$/i.test(l));
  if (summaryIdx >= 0) {
    const buf = [];
    for (let i = summaryIdx + 1; i < lines.length; i++) {
      if (/^##\s+/.test(lines[i])) break;
      buf.push(lines[i]);
    }
    result.summary = buf.join('\n').trim().slice(0, 300);
  }

  // Parse checklist
  result.checklist = parseChecklist(text);

  // Parse current wave (for wave-plan.md)
  if (absPath.endsWith('wave-plan.md')) {
    result.currentWave = parseCurrentWave(text);

    // Aggregate checklists and metrics from sub-wave spec.md files
    const epicDir = path.dirname(absPath);
    const subDirs = safe(() => fs.readdirSync(epicDir, { withFileTypes: true })) || [];
    const aggItems = [];
    let aggLastActivity = null;
    let aggApiCalls = 0;
    let aggRetries = 0;
    let hasMetrics = false;
    for (const sub of subDirs) {
      if (!sub.isDirectory()) continue;
      const subSpec = path.join(epicDir, sub.name, 'spec.md');
      if (!fs.existsSync(subSpec)) continue;
      const subText = safe(() => fs.readFileSync(subSpec, 'utf8'));
      if (!subText) continue;
      const subChecklist = parseChecklist(subText);
      const prefix = `[${sub.name}]`;
      for (const item of subChecklist.items) {
        aggItems.push({ text: `${prefix} ${item.text}`, done: item.done });
      }
      const subMetrics = path.join(STATES_DIR, `${sub.name}.metrics.json`);
      if (fs.existsSync(subMetrics)) {
        const m = safe(() => JSON.parse(fs.readFileSync(subMetrics, 'utf8')));
        if (m && m.metrics) {
          hasMetrics = true;
          if (m.metrics.updatedAt && (!aggLastActivity || m.metrics.updatedAt > aggLastActivity)) {
            aggLastActivity = m.metrics.updatedAt;
          }
          if (m.metrics.apiCalls != null) aggApiCalls += m.metrics.apiCalls;
          if (m.metrics.retries != null) aggRetries += m.metrics.retries;
        }
      }
    }
    if (aggItems.length > 0) {
      const doneCount = aggItems.filter(i => i.done).length;
      result.checklist = {
        total: aggItems.length,
        done: doneCount,
        percent: Math.round((doneCount / aggItems.length) * 100),
        items: aggItems,
      };
    }
    if (hasMetrics) {
      result.lastActivity = aggLastActivity;
      result.apiCalls = aggApiCalls;
      result.retries = aggRetries;
    }
  }

  if (!absPath.endsWith('wave-plan.md')) {
    // Read metrics from .pipeline-states/
    const dirName = path.basename(path.dirname(absPath));
    const metricsPath = path.join(STATES_DIR, `${dirName}.metrics.json`);
    if (fs.existsSync(metricsPath)) {
      const m = safe(() => JSON.parse(fs.readFileSync(metricsPath, 'utf8')));
      if (m && m.metrics) {
        result.lastActivity = m.metrics.updatedAt || null;
        result.apiCalls = m.metrics.apiCalls != null ? m.metrics.apiCalls : null;
        result.retries = m.metrics.retries != null ? m.metrics.retries : null;
      }
    }

    // Inline wave plan: single spec.md declaring waves in pipeline-state JSON.
    const ps = readWavePlanState(dirName);
    if (ps && Array.isArray(ps.waves) && ps.waves.length > 0) {
      result.isWavePlan = true;
      result.currentWave = ps.currentWave != null ? String(ps.currentWave) : null;
      result.totalWaves = ps.totalWaves || ps.waves.length;
      result.wave = result.currentWave ? `${result.currentWave}/${result.totalWaves}` : result.wave;
      result.completedWaves = ps.completedWaves || [];
      result.failedWaves = ps.failedWaves || [];

      const parentDir = path.dirname(absPath);
      const activityByWave = harnessActivityByWave(dirName);
      const aggregatedItems = [];
      // Pipeline-state's phaseName tracks the live wave phase; parent spec.md
      // header is set at approval and never refreshed.
      const livePhase = ps.phaseName || ps.phase || result.phase || 'EXECUTE';

      result.waves = ps.waves.map((w) => {
        // Sub-wave spec.md is sourced from state.json's `w.spec` when present;
        // otherwise fall back to conventional `wave-{id}-{slug}` directory.
        const fallbackDir = `wave-${w.id}${w.slug ? '-' + w.slug : ''}`;
        const subSpecRel = w.spec || `${fallbackDir}/spec.md`;
        const subSpecAbs = path.join(parentDir, subSpecRel);
        const subText = fs.existsSync(subSpecAbs) ? safe(() => fs.readFileSync(subSpecAbs, 'utf8')) : null;

        let displayName = w.name || w.slug || fallbackDir;
        if (subText) {
          const titleM = subText.match(/^#\s+(.+?)\s*$/m);
          if (titleM) displayName = titleM[1].replace(/^Wave\s+\d+\s*[—\-:]?\s*/i, '').trim() || displayName;
        }

        const subChecklist = subText ? parseChecklist(subText) : { items: [] };
        const wItems = subChecklist.items.length > 0
          ? subChecklist.items
          : (result.checklist.items || []).filter((it) => it.wave === w.id);
        const wDone = wItems.filter((it) => it.done).length;
        for (const it of wItems) aggregatedItems.push({ text: it.text, done: it.done, wave: w.id });

        const status = waveStatusFor(w.id, ps);
        const tsMs = activityByWave.get(w.id) || null;
        const lastActivity = tsMs ? new Date(tsMs).toISOString() : null;

        return {
          id: w.id,
          name: displayName,
          slug: w.slug || null,
          files: w.files || null,
          entities: w.entities || null,
          status,
          phase: status === 'current' ? livePhase : status === 'completed' ? 'DONE' : status === 'failed' ? 'FAILED' : null,
          checklist: {
            total: wItems.length,
            done: wDone,
            percent: wItems.length > 0 ? Math.round((wDone / wItems.length) * 100) : 0,
            items: wItems,
          },
          lastActivity,
        };
      });

      // Roll aggregated sub-wave checklist + most recent harness activity up
      // to the epic-level card. Without this, the spec card stays at 0/0
      // because the parent spec.md only narrates waves — items live in
      // sub-wave specs.
      if (aggregatedItems.length > 0) {
        const doneCount = aggregatedItems.filter(i => i.done).length;
        result.checklist = {
          total: aggregatedItems.length,
          done: doneCount,
          percent: Math.round((doneCount / aggregatedItems.length) * 100),
          items: aggregatedItems,
        };
      }
      let maxTs = result.lastActivity ? Date.parse(result.lastActivity) : 0;
      for (const w of result.waves) {
        if (!w.lastActivity) continue;
        const t = Date.parse(w.lastActivity);
        if (!isNaN(t) && t > maxTs) maxTs = t;
      }
      if (maxTs) result.lastActivity = new Date(maxTs).toISOString();
      result.phase = livePhase;
    }
  }

  return result;
}

function listAllSpecs() {
  return [
    ...listSpecsIn('active', path.join(SPEC_DIR, 'active')),
    ...listSpecsIn('completed', path.join(SPEC_DIR, 'completed')),
  ];
}

function sendJson(res, status, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': Buffer.byteLength(body),
    'Cache-Control': 'no-store',
  });
  res.end(body);
}

function send(res, status, contentType, body) {
  res.writeHead(status, {
    'Content-Type': contentType,
    'Content-Length': Buffer.byteLength(body),
    'Cache-Control': 'no-store',
  });
  res.end(body);
}

function readBody(req, limit, cb) {
  let total = 0;
  const chunks = [];
  req.on('data', chunk => {
    total += chunk.length;
    if (total > limit) {
      cb(new Error('payload too large'));
      req.destroy();
      return;
    }
    chunks.push(chunk);
  });
  req.on('end', () => cb(null, Buffer.concat(chunks).toString('utf8')));
  req.on('error', err => cb(err));
}

// ── Handlers ──────────────────────────────────────────────────────────

function handleSpecs(res) {
  const specs = listAllSpecs();
  sendJson(res, 200, { specs });
}

function handleSpec(res, query) {
  const rel = (query.path || '').replace(/\\/g, '/');
  if (!rel) return sendJson(res, 400, { error: 'path required' });
  if (rel.includes('..')) return sendJson(res, 400, { error: 'invalid path' });

  const abs = path.resolve(ROOT, rel);
  const specRoot = path.resolve(SPEC_DIR);
  if (!abs.startsWith(specRoot + path.sep) && abs !== specRoot) {
    return sendJson(res, 400, { error: 'path outside spec directory' });
  }
  if (!fs.existsSync(abs)) return sendJson(res, 404, { error: 'spec not found' });

  let markdown = '';
  try { markdown = fs.readFileSync(abs, 'utf8'); }
  catch (e) { return sendJson(res, 500, { error: e.message }); }

  let metrics = null;
  const dirName = path.basename(path.dirname(abs));
  const metricsPath = path.join(STATES_DIR, `${dirName}.metrics.json`);
  if (fs.existsSync(metricsPath)) {
    metrics = safe(() => JSON.parse(fs.readFileSync(metricsPath, 'utf8')));
  }

  sendJson(res, 200, { markdown, metrics });
}

function handleMetrics(res) {
  let raw = '';
  try {
    raw = execFileSync('node', [path.join(CLAUDE_DIR, 'scripts', 'metrics-collect.js')], {
      cwd: ROOT,
      timeout: 5000,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      maxBuffer: 4 * 1024 * 1024,
      windowsHide: true,
    });
  } catch (e) {
    return sendJson(res, 200, { error: `metrics-collect failed: ${e.message}` });
  }

  const parsed = parseMetricsMarkdown(raw);
  // Mustard 2.0 Phase 2: attach real OpenTelemetry-derived token usage from
  // subagent spans. Fail-open: returns null when EventStore isn't reachable
  // or no spans are recorded yet — UI renders a neutral placeholder.
  parsed.tokenUsage = buildTokenUsage(getStore());
  sendJson(res, 200, parsed);
}

/**
 * Aggregate real token usage from subagent spans recorded by
 * `templates/hooks/subagent-tracker.js`. Buckets:
 *   - byPhase: keyed by `phase` column (ANALYZE/PLAN/EXECUTE/QA/CLOSE/...).
 *   - byModel: keyed by `model` column (claude-opus-4-7, ...).
 *   - byAgent: keyed by `attributes['mustard.agent_type']` (general-purpose,
 *     Explore, Plan, Bash).
 *
 * Each bucket exposes { input, output, cost, count }. `costUsd` uses the
 * snapshot pricing table from `dist/telemetry/pricing.js` (fail-open via
 * `_lib/pricing.js` wrapper).
 *
 * Contract:
 *   - store == null               → returns null (legacy/no telemetry)
 *   - store but zero spans        → returns empty buckets, totals = 0
 *   - store with spans            → fully populated payload
 *
 * Hard cap: 5000 spans per query (chronological ASC). Older spans are
 * ignored — telemetry surfaces recent activity, history lives in spans.jsonl.
 */
function buildTokenUsage(store) {
  if (!store) return null;
  let spans = [];
  try { spans = store.spans({ limit: 5000 }) || []; }
  catch (_) { return null; }

  const empty = {
    byPhase: {}, byModel: {}, byAgent: {},
    totalInput: 0, totalOutput: 0, costUsd: 0, spanCount: 0,
  };
  if (!spans.length) return empty;

  const { costUsd } = require('./_lib/pricing.js');
  const byPhase = {};
  const byModel = {};
  const byAgent = {};
  let totalInput = 0, totalOutput = 0, totalCost = 0;

  function bucketAdd(bucket, key, input, output, cost) {
    if (!bucket[key]) bucket[key] = { input: 0, output: 0, cost: 0, count: 0 };
    bucket[key].input += input;
    bucket[key].output += output;
    bucket[key].cost += cost;
    bucket[key].count += 1;
  }

  for (const s of spans) {
    const input = s.inputTokens || 0;
    const output = s.outputTokens || 0;
    const model = s.model || 'unknown';
    const cost = costUsd(model, input, output);
    totalInput += input;
    totalOutput += output;
    totalCost += cost;

    bucketAdd(byPhase, s.phase || 'UNKNOWN', input, output, cost);
    bucketAdd(byModel, model, input, output, cost);

    // `attributes` is already an object (event-store.js parses JSON in rowToSpan),
    // but accept stringified shape too for defensive parity with raw rows.
    let attrs = s.attributes;
    if (typeof attrs === 'string') {
      try { attrs = JSON.parse(attrs); } catch (_) { attrs = {}; }
    }
    if (!attrs || typeof attrs !== 'object') attrs = {};
    const agent = attrs['mustard.agent_type'] || 'unknown';
    bucketAdd(byAgent, agent, input, output, cost);
  }

  return {
    byPhase, byModel, byAgent,
    totalInput, totalOutput,
    costUsd: totalCost,
    spanCount: spans.length,
  };
}

function parseMetricsMarkdown(md) {
  const out = {
    summary: [],
    hookEvents: [],
    rtkSavings: { tokens: 0, rate: 0, commands: 0 },
    last7Days: [],
    pipelineHealth: null,
    knowledgeGrowth: null,
  };
  const lines = md.split(/\r?\n/);
  let section = null;

  for (let i = 0; i < lines.length; i++) {
    const ln = lines[i];
    if (/^##\s+Summary/i.test(ln)) { section = 'summary'; continue; }
    if (/^##\s+Last 7 Days/i.test(ln)) { section = 'days'; continue; }
    if (/^##\s+Pipeline Health/i.test(ln)) { section = 'health'; out.pipelineHealth = {}; continue; }
    if (/^##\s+Knowledge Growth/i.test(ln)) { section = 'knowledge'; out.knowledgeGrowth = {}; continue; }
    if (/^##\s+All Hook Events/i.test(ln)) { section = 'hooks-new'; continue; }
    if (/^##\s+Enforcement Events/i.test(ln)) { section = 'hooks'; continue; }
    if (/^##\s+RTK Token Economy/i.test(ln)) { section = 'rtk'; continue; }
    if (/^##\s+Token Economy/i.test(ln)) { section = 'economy'; continue; }
    if (/^##\s+/.test(ln)) { section = null; continue; }

    if (section === 'summary' && ln.trim()) {
      out.summary.push(ln.trim());
    } else if (section === 'days' && /^\|\s*\d{4}-\d{2}-\d{2}/.test(ln)) {
      const cells = ln.split('|').map(s => s.trim()).filter(Boolean);
      if (cells.length >= 2) {
        const events = parseInt(cells[1], 10);
        if (!Number.isNaN(events)) out.last7Days.push({ day: cells[0], events });
      }
    } else if (section === 'hooks-new' && ln.startsWith('|') && !/^\|\s*-+/.test(ln)) {
      // Format: | Event | Count | Category | Tokens Cut |
      // The markdown column header (produced by metrics-collect.js) keeps the
      // legacy label; here we relabel only the JSON field name so downstream
      // payloads expose `tokensCut` exclusively (Phase 2 telemetry contract).
      const cells = ln.split('|').map(s => s.trim());
      if (cells.length >= 5 && cells[1] && cells[1] !== 'Event' && !cells[1].startsWith('**TOTAL')) {
        const count = parseInt(cells[2], 10);
        if (!Number.isNaN(count)) {
          out.hookEvents.push({
            event: cells[1],
            count,
            category: cells[3] || 'other',
            tokensAffected: 0,
            tokensCut: cells[4] === '-' ? 0 : parseInt(cells[4], 10) || 0,
          });
        }
      }
    } else if (section === 'hooks' && ln.startsWith('|') && !/^\|\s*-+/.test(ln)) {
      // Legacy format: | Event | Count | Tokens Affected | Tokens Cut |
      const cells = ln.split('|').map(s => s.trim());
      if (cells.length >= 5 && cells[1] && cells[1] !== 'Event' && !cells[1].startsWith('**TOTAL')) {
        const count = parseInt(cells[2], 10);
        if (!Number.isNaN(count)) {
          out.hookEvents.push({
            event: cells[1],
            count,
            category: 'other',
            tokensAffected: cells[3] === '-' ? 0 : parseInt(cells[3], 10) || 0,
            tokensCut: cells[4] === '-' ? 0 : parseInt(cells[4], 10) || 0,
          });
        }
      }
    } else if (section === 'rtk' || section === 'economy') {
      const saved = ln.match(/(?:Total saved:|RTK[^:]*:)\s*~?([\d.]+)k tokens/i);
      if (saved) out.rtkSavings.tokens = Math.round(parseFloat(saved[1]) * 1000);
      const rate = ln.match(/(\d+)%\s*(?:rate|savings rate)/i) || ln.match(/Savings rate:\s*(\d+)%/i);
      if (rate) out.rtkSavings.rate = parseInt(rate[1], 10);
      const cmds = ln.match(/(\d+)\s*commands/i) || ln.match(/Commands rewritten:\s*(\d+)/i);
      if (cmds) out.rtkSavings.commands = parseInt(cmds[1], 10);
    } else if (section === 'health' && /^\s*-\s+/.test(ln)) {
      const total = ln.match(/Total pipelines tracked:\s*(\d+)\s*\(active:\s*(\d+)\s*·\s*archived:\s*(\d+)\)/i);
      if (total) { out.pipelineHealth.totalSpecs = parseInt(total[1], 10); out.pipelineHealth.activeCount = parseInt(total[2], 10); out.pipelineHealth.archivedCount = parseInt(total[3], 10); }
      const pass1 = ln.match(/Pass@1[^:]*:\s*(\d+)%\s*\((\d+)\/(\d+)\)/i);
      if (pass1) { out.pipelineHealth.pass1Pct = parseInt(pass1[1], 10); out.pipelineHealth.pass1Count = parseInt(pass1[2], 10); }
      const dur = ln.match(/Avg duration:\s*(.+)$/i);
      if (dur) out.pipelineHealth.avgDuration = dur[1].trim();
      const api = ln.match(/Avg API calls per pipeline:\s*(\d+)/i);
      if (api) out.pipelineHealth.avgApiCalls = parseInt(api[1], 10);
      const ret = ln.match(/Avg hook retries per pipeline:\s*([\d.]+)/i);
      if (ret) out.pipelineHealth.avgRetries = parseFloat(ret[1]);
      const worst = ln.match(/Worst phase:\s*(\w+)\s*\((\d+)\s*total retries across\s*(\d+)/i);
      if (worst) out.pipelineHealth.worstPhase = { phase: worst[1], totalRetries: parseInt(worst[2], 10), affected: parseInt(worst[3], 10) };
      const l0 = ln.match(/L0 delegation ratio:\s*(\d+)%\s*\((\d+)\s*delegated\s*\/\s*(\d+)\s*direct\)/i);
      if (l0) { out.pipelineHealth.l0Pct = parseInt(l0[1], 10); out.pipelineHealth.l0Delegated = parseInt(l0[2], 10); out.pipelineHealth.l0Direct = parseInt(l0[3], 10); }
    } else if (section === 'knowledge' && /^\s*-\s+/.test(ln)) {
      const kb = ln.match(/Knowledge entries:\s*(\d+)\s*\(avg confidence:\s*([\d.]+)\)/i);
      if (kb) { out.knowledgeGrowth.entries = parseInt(kb[1], 10); out.knowledgeGrowth.avgConfidence = parseFloat(kb[2]); }
      const dec = ln.match(/Decisions captured:\s*(\d+)/i);
      if (dec) out.knowledgeGrowth.decisions = parseInt(dec[1], 10);
      const les = ln.match(/Lessons learned:\s*(\d+)/i);
      if (les) out.knowledgeGrowth.lessons = parseInt(les[1], 10);
    }
  }
  return out;
}

function handleEvents(res, query) {
  const requested = parseInt(query.n, 10);
  let n = Number.isNaN(requested) ? 200 : requested;
  if (n < 1) n = 1;
  if (n > 1000) n = 1000;

  const files = [];
  if (fs.existsSync(EVENTS_FILE)) files.push(EVENTS_FILE);
  for (const f of discoverHarnessFiles()) {
    if (f !== EVENTS_FILE) files.push(f);
  }

  const events = [];
  for (const file of files) {
    let content;
    try { content = fs.readFileSync(file, 'utf8'); } catch (_) { continue; }
    const subRoot = path.relative(ROOT, path.dirname(path.dirname(path.dirname(file)))).replace(/\\/g, '/');
    const source = subRoot || '.';
    const lines = content.split(/\r?\n/).filter(Boolean);
    const tail = lines.slice(-n);
    for (const line of tail) {
      try {
        const ev = JSON.parse(line);
        if (!ev._source) ev._source = source;
        events.push(ev);
      } catch (_) {}
    }
  }
  events.sort(function(a, b){
    return (Date.parse(a.ts || a.timestamp) || 0) - (Date.parse(b.ts || b.timestamp) || 0);
  });
  sendJson(res, 200, { events: events.slice(-n) });
}

function readSettings() {
  if (!fs.existsSync(SETTINGS_FILE)) return null;
  return safe(() => JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8')));
}

function handleSettingsGet(res) {
  const s = readSettings();
  const env = (s && s.env) || {};
  const defaults = defaultsMap();
  const values = {};
  for (const k of Object.keys(defaults)) {
    values[k] = env[k] != null ? env[k] : defaults[k];
  }
  sendJson(res, 200, { catalog: ENV_CATALOG, values });
}

function handleSettingsPost(res, body) {
  let payload;
  try { payload = JSON.parse(body || '{}'); }
  catch (_) { return sendJson(res, 400, { ok: false, error: 'invalid JSON' }); }
  const updates = payload.values || {};
  for (const k of Object.keys(updates)) {
    if (!isKnownKey(k)) return sendJson(res, 400, { ok: false, error: 'unknown key: ' + k });
    if (!isValidValue(k, updates[k])) return sendJson(res, 400, { ok: false, error: 'invalid value for ' + k + ': ' + updates[k] });
  }
  const s = readSettings() || {};
  s.env = s.env || {};
  for (const k of Object.keys(updates)) s.env[k] = String(updates[k]);
  try { fs.writeFileSync(SETTINGS_FILE, JSON.stringify(s, null, 2) + '\n', 'utf8'); }
  catch (e) { return sendJson(res, 500, { ok: false, error: e.message }); }
  sendJson(res, 200, { ok: true, values: s.env });
}

function handleSpecLive(res, query) {
  const specName = String(query.spec || '').trim();
  if (!specName) return sendJson(res, 400, { error: 'spec required' });
  const waveFilterRaw = query.wave != null ? String(query.wave).trim() : '';
  const waveFilter = waveFilterRaw !== '' && !isNaN(Number(waveFilterRaw)) ? Number(waveFilterRaw) : null;

  // Build candidate metric file names. Accept "epic/wave" → also try just "wave".
  const candidates = [];
  if (specName.indexOf('/') >= 0) {
    candidates.push(specName.split('/').pop()); // bare wave dir name
  } else {
    candidates.push(specName);
    // If it's an epic dir, also collect every wave inside
    const epicDir = path.join(SPEC_DIR, 'active', specName);
    if (fs.existsSync(epicDir)) {
      const subs = safe(() => fs.readdirSync(epicDir, { withFileTypes: true })) || [];
      for (const s of subs) if (s.isDirectory()) candidates.push(s.name);
    }
  }

  // Aggregate metrics across candidates. Order of preference:
  //   1. EventStore.metrics(spec)               (Mustard 2.0, single source of truth)
  //   2. .claude/metrics/*.json                 (post-Wave 4 archive)
  //   3. .claude/.pipeline-states/*.metrics.json (legacy sidecar)
  const metricsDir = path.join(CLAUDE_DIR, 'metrics');
  let phase = null, lastActivity = null, apiCalls = 0, retries = 0, hasMetrics = false;
  let dispatchFailuresByPhase = {}, toolBreakdown = {};
  const _store = getStore();
  for (const c of candidates) {
    let m = null;
    let prevPhase = null;
    if (_store) {
      try {
        const pm = _store.metrics(c);
        if (pm) {
          m = {
            apiCalls: pm.apiCalls,
            retries: pm.retries,
            toolBreakdown: pm.toolBreakdown || {},
            dispatchFailuresByPhase: pm.dispatchFailuresByPhase || {},
            updatedAt: pm.updatedAt || null,
          };
        }
      } catch (_) {}
    }
    if (!m) {
      const pNew = path.join(metricsDir, c + '.json');
      const pLeg = path.join(STATES_DIR, c + '.metrics.json');
      let raw = null, isNew = false;
      if (fs.existsSync(pNew)) { raw = safe(() => JSON.parse(fs.readFileSync(pNew, 'utf8'))); isNew = true; }
      else if (fs.existsSync(pLeg)) { raw = safe(() => JSON.parse(fs.readFileSync(pLeg, 'utf8'))); }
      if (raw) {
        m = isNew ? raw : raw.metrics;
        if (!isNew && raw.previousPhase) prevPhase = raw.previousPhase;
      }
    }
    if (!m) continue;
    hasMetrics = true;
    if (prevPhase && !phase) phase = prevPhase;
    if (m.updatedAt && (!lastActivity || m.updatedAt > lastActivity)) lastActivity = m.updatedAt;
    if (m.apiCalls != null) apiCalls += m.apiCalls;
    if (m.retries != null) retries += m.retries;
    if (m.dispatchFailuresByPhase) for (const k of Object.keys(m.dispatchFailuresByPhase)) dispatchFailuresByPhase[k] = (dispatchFailuresByPhase[k] || 0) + m.dispatchFailuresByPhase[k];
    if (m.toolBreakdown) for (const k of Object.keys(m.toolBreakdown)) toolBreakdown[k] = (toolBreakdown[k] || 0) + m.toolBreakdown[k];
  }

  // Match harness events: target is one of the candidate names OR the full path
  const matchSet = new Set(candidates);
  matchSet.add(specName);
  if (specName.indexOf('/') >= 0) matchSet.add(specName.split('/')[0]); // epic name too

  const events = [];
  // EventStore primary path: query per candidate spec name (matches root cwd only,
  // not subprojects — for monorepo subproject harness files we still fall back
  // to filesystem reads below).
  let storeUsedForEvents = false;
  if (_store) {
    try {
      for (const cand of matchSet) {
        const rows = _store.query({ spec: cand });
        if (Array.isArray(rows) && rows.length) {
          for (const ev of rows) events.push(ev);
        }
      }
      storeUsedForEvents = true;
    } catch (_) { storeUsedForEvents = false; }
  }
  // Filesystem fallback: read events.jsonl when EventStore unavailable, AND
  // always scan subproject harness files (EventStore is per-root only).
  const eventFiles = [];
  if (!storeUsedForEvents && fs.existsSync(EVENTS_FILE)) eventFiles.push(EVENTS_FILE);
  for (const f of discoverHarnessFiles()) if (f !== EVENTS_FILE) eventFiles.push(f);
  for (const file of eventFiles) {
    const content = safe(() => fs.readFileSync(file, 'utf8')) || '';
    const lines = content.split(/\r?\n/).filter(Boolean);
    const tail = lines.slice(-2000);
    for (const line of tail) {
      try {
        const ev = JSON.parse(line);
        const target = ev.spec || (ev.payload && ev.payload.spec) || '';
        if (!target) continue;
        if (matchSet.has(target)) { events.push(ev); continue; }
        // Also accept a target that is "epic/wave" or where target startsWith one of our candidates
        for (const cand of matchSet) {
          if (target === cand || target.indexOf(cand + '/') === 0 || cand.indexOf(target + '/') === 0) { events.push(ev); break; }
        }
      } catch (_) {}
    }
  }
  events.sort(function(a, b){ return (Date.parse(a.ts||a.timestamp)||0) - (Date.parse(b.ts||b.timestamp)||0); });

  let isLive = false;
  if (lastActivity) {
    const t = Date.parse(lastActivity);
    if (!isNaN(t)) isLive = (Date.now() - t) < 5 * 60 * 1000;
  }

  // Fallback: also read spec.md / wave-plan.md content so the panel always shows context
  let specMd = null, specRelPath = null, summary = '', checklist = null;
  let status = null, scope = null, waveLabel = null, checkpoint = null, specPhase = null;
  function tryReadSpec(absDir, label) {
    if (specMd) return;
    if (!fs.existsSync(absDir)) return;
    const sp = path.join(absDir, 'spec.md');
    const wp = path.join(absDir, 'wave-plan.md');
    const file = fs.existsSync(sp) ? sp : (fs.existsSync(wp) ? wp : null);
    if (!file) return;
    const text = safe(() => fs.readFileSync(file, 'utf8'));
    if (!text) return;
    specMd = text;
    specRelPath = path.relative(ROOT, file).replace(/\\/g, '/');
    const lines = text.split(/\r?\n/);

    // Parse "### Status: X | Phase: Y | Scope: Z | Wave: W" header
    for (let i = 0; i < Math.min(lines.length, 30); i++) {
      const ln = lines[i];
      if (/^###\s*Status:/i.test(ln)) {
        const parts = ln.replace(/^###\s*/i, '').split('|');
        for (const p of parts) {
          const m = p.match(/^\s*([^:]+?)\s*:\s*(.+?)\s*$/);
          if (!m) continue;
          const k = m[1].toLowerCase(), v = m[2].trim();
          if (k === 'status') status = v;
          else if (k === 'phase') specPhase = v;
          else if (k === 'scope') scope = v;
          else if (k === 'wave') waveLabel = v;
        }
      }
      const cp = ln.match(/^###\s*Checkpoint:\s*(.+)$/i);
      if (cp) checkpoint = cp[1].trim();
    }

    const idx = lines.findIndex(l => /^##\s+Summary\s*$/i.test(l));
    if (idx >= 0) {
      const buf = [];
      for (let i = idx + 1; i < lines.length; i++) { if (/^##\s+/.test(lines[i])) break; buf.push(lines[i]); }
      summary = buf.join('\n').trim();
    }
    const parsed = parseChecklist(text);
    if (parsed.total > 0) checklist = parsed;

    // Epic case: when reading wave-plan.md, aggregate checklists from sub-wave spec.md files
    // (mirrors the aggregation in parseSpecFile so live monitor and spec card agree).
    if (file === wp) {
      const aggItems = [];
      const subDirs = safe(() => fs.readdirSync(absDir, { withFileTypes: true })) || [];
      for (const sub of subDirs) {
        if (!sub.isDirectory()) continue;
        const subSpec = path.join(absDir, sub.name, 'spec.md');
        if (!fs.existsSync(subSpec)) continue;
        const subText = safe(() => fs.readFileSync(subSpec, 'utf8'));
        if (!subText) continue;
        const subLines = subText.split(/\r?\n/);
        let inSub = false;
        for (const line of subLines) {
          if (/^##\s+Checklist\s*$/i.test(line)) { inSub = true; continue; }
          if (inSub && /^##\s+/.test(line)) { inSub = false; continue; }
          if (!inSub) continue;
          const dn = line.match(/^\s*-\s+\[x\]\s+(.+)$/i);
          const pn = line.match(/^\s*-\s+\[\s\]\s+(.+)$/i);
          if (dn) aggItems.push({ text: `[${sub.name}] ${dn[1].trim()}`, done: true });
          else if (pn) aggItems.push({ text: `[${sub.name}] ${pn[1].trim()}`, done: false });
        }
      }
      if (aggItems.length) {
        const dc = aggItems.filter(i => i.done).length;
        checklist = {
          total: aggItems.length,
          done: dc,
          percent: Math.round((dc / aggItems.length) * 100),
          items: aggItems,
        };
      }
    }
  }
  if (specName.indexOf('/') >= 0) {
    const parts = specName.split('/');
    tryReadSpec(path.join(SPEC_DIR, 'active', parts[0], parts[1]));
    tryReadSpec(path.join(SPEC_DIR, 'completed', parts[0], parts[1]));
  } else {
    tryReadSpec(path.join(SPEC_DIR, 'active', specName));
    tryReadSpec(path.join(SPEC_DIR, 'completed', specName));
  }

  // Phase priority: metrics.previousPhase > spec.md "Phase:" header
  const finalPhase = phase || specPhase || null;

  // Inline wave plan enrichment (only when querying epic/single spec by bare name)
  let waveStateOut = null;
  if (specName.indexOf('/') < 0) {
    const ps = readWavePlanState(specName);
    if (ps && Array.isArray(ps.waves) && ps.waves.length > 0) {
      const allItems = (checklist && checklist.items) || [];
      waveStateOut = {
        currentWave: ps.currentWave != null ? String(ps.currentWave) : null,
        totalWaves: ps.totalWaves || ps.waves.length,
        completedWaves: ps.completedWaves || [],
        failedWaves: ps.failedWaves || [],
        waves: ps.waves.map((w) => {
          const wItems = allItems.filter((it) => it.wave === w.id);
          const wDone = wItems.filter((it) => it.done).length;
          const status = waveStatusFor(w.id, ps);
          return {
            id: w.id,
            name: w.name,
            files: w.files || null,
            entities: w.entities || null,
            status,
            checklist: {
              total: wItems.length,
              done: wDone,
              percent: wItems.length > 0 ? Math.round((wDone / wItems.length) * 100) : 0,
            },
          };
        }),
      };
    }
  }

  // If caller asked for a specific wave, narrow events + checklist to it and
  // surface a waveContext so the panel can label itself by wave.
  let outEvents = events;
  let outChecklist = checklist;
  let waveContext = null;
  let outPhase = finalPhase;
  let outApiCalls = hasMetrics ? apiCalls : null;
  let outRetries = hasMetrics ? retries : null;
  let outLastActivity = lastActivity;
  if (waveFilter != null) {
    outEvents = events.filter((ev) => {
      const pl = ev.payload || {};
      const w = typeof ev.wave === 'number' ? ev.wave : (typeof pl.wave === 'number' ? pl.wave : null);
      return w === waveFilter;
    });
    if (outChecklist && Array.isArray(outChecklist.items)) {
      const wItems = outChecklist.items.filter((it) => it.wave === waveFilter);
      const wDone = wItems.filter((it) => it.done).length;
      outChecklist = {
        total: wItems.length,
        done: wDone,
        percent: wItems.length > 0 ? Math.round((wDone / wItems.length) * 100) : 0,
        items: wItems,
      };
    }
    if (waveStateOut) {
      const wd = waveStateOut.waves.find((w) => w.id === waveFilter);
      if (wd) {
        waveContext = { id: wd.id, name: wd.name, files: wd.files, entities: wd.entities, status: wd.status };
        if (wd.status === 'current') outPhase = finalPhase || 'EXECUTE';
        else if (wd.status === 'completed') outPhase = 'DONE';
        else if (wd.status === 'failed') outPhase = 'FAILED';
        else if (wd.status === 'pending') outPhase = 'PENDING';
      }
    }
    // Wave-scoped apiCalls/retries derived from filtered events instead of metrics-aggregated totals.
    outApiCalls = outEvents.filter((e) => e.event === 'tool.use').length;
    outRetries = outEvents.filter((e) => e.payload && e.payload.retry).length;
    const lastEv = outEvents[outEvents.length - 1];
    outLastActivity = lastEv ? (lastEv.ts || lastEv.timestamp || lastActivity) : null;
  }

  sendJson(res, 200, {
    events: outEvents.slice(-80),
    phase: outPhase,
    status,
    scope,
    wave: waveStateOut ? `${waveStateOut.currentWave}/${waveStateOut.totalWaves}` : waveLabel,
    checkpoint,
    lastActivity: outLastActivity,
    apiCalls: outApiCalls,
    retries: outRetries,
    dispatchFailuresByPhase: hasMetrics ? dispatchFailuresByPhase : null,
    toolBreakdown: hasMetrics ? toolBreakdown : null,
    candidates,
    isLive,
    summary,
    checklist: outChecklist,
    specPath: specRelPath,
    isWavePlan: !!waveStateOut,
    waveState: waveStateOut,
    waveContext,
  });
}

function handleTelemetryExtra(res) {
  const out = {
    pipelineAggregates: { totalApiCalls: 0, totalRetries: 0, dispatchFailuresByPhase: {}, toolBreakdown: {}, runs: 0, pass1: 0 },
    phaseDistribution: {},
    knowledgeEntries: 0,
    activeAging: { lt7d: 0, d7_30: 0, gt30d: 0 },
    storageBreakdown: {},
    detectInfo: null,
    activeNow: [],
  };

  // Pipeline metrics aggregation. Order of preference:
  //   1. EventStore.specs() → metrics()       (Mustard 2.0)
  //   2. .claude/metrics/*.json                (post-Wave 4 archive)
  //   3. .claude/.pipeline-states/*.metrics.json (legacy sidecar)
  const seen = new Set();
  const _storeAgg = getStore();
  if (_storeAgg) {
    try {
      for (const spec of _storeAgg.specs()) {
        const m = _storeAgg.metrics(spec.name);
        if (!m) continue;
        seen.add(spec.name);
        out.pipelineAggregates.runs++;
        out.pipelineAggregates.totalApiCalls += m.apiCalls || 0;
        out.pipelineAggregates.totalRetries += m.retries || 0;
        if ((m.retries || 0) === 0) out.pipelineAggregates.pass1++;
        if (m.dispatchFailuresByPhase) for (const k of Object.keys(m.dispatchFailuresByPhase)) out.pipelineAggregates.dispatchFailuresByPhase[k] = (out.pipelineAggregates.dispatchFailuresByPhase[k] || 0) + m.dispatchFailuresByPhase[k];
        if (m.toolBreakdown) for (const k of Object.keys(m.toolBreakdown)) out.pipelineAggregates.toolBreakdown[k] = (out.pipelineAggregates.toolBreakdown[k] || 0) + m.toolBreakdown[k];
      }
    } catch (_) {}
  }
  // Filesystem fallback (and supplement for specs not yet projected into DB).
  // Schema differs: archive stores fields at top-level; legacy nests under `metrics`.
  const sources = [
    { dir: path.join(CLAUDE_DIR, 'metrics'), unwrap: (m) => m },
    { dir: STATES_DIR, unwrap: (m) => m && m.metrics, suffix: '.metrics.json' },
  ];
  for (const src of sources) {
    if (!fs.existsSync(src.dir)) continue;
    const files = safe(() => fs.readdirSync(src.dir)) || [];
    for (const f of files) {
      if (!f.endsWith('.json')) continue;
      if (src.suffix && !f.endsWith(src.suffix)) continue;
      if (!src.suffix && f.endsWith('.metrics.json')) continue;
      const specKey = f.replace(/\.(metrics\.)?json$/, '');
      if (seen.has(specKey)) continue;
      const raw = safe(() => JSON.parse(fs.readFileSync(path.join(src.dir, f), 'utf8')));
      const m = raw && src.unwrap(raw);
      if (!m) continue;
      seen.add(specKey);
      out.pipelineAggregates.runs++;
      out.pipelineAggregates.totalApiCalls += m.apiCalls || 0;
      out.pipelineAggregates.totalRetries += m.retries || 0;
      if ((m.retries || 0) === 0) out.pipelineAggregates.pass1++;
      const phaseSrc = m.dispatchFailuresByPhase || null;
      if (phaseSrc) for (const k of Object.keys(phaseSrc)) out.pipelineAggregates.dispatchFailuresByPhase[k] = (out.pipelineAggregates.dispatchFailuresByPhase[k] || 0) + phaseSrc[k];
      if (m.toolBreakdown) for (const k of Object.keys(m.toolBreakdown)) out.pipelineAggregates.toolBreakdown[k] = (out.pipelineAggregates.toolBreakdown[k] || 0) + m.toolBreakdown[k];
    }
  }

  // Phase distribution + aging from active specs
  const activeSpecsDir = path.join(SPEC_DIR, 'active');
  if (fs.existsSync(activeSpecsDir)) {
    const dirs = safe(() => fs.readdirSync(activeSpecsDir, { withFileTypes: true })) || [];
    for (const d of dirs) {
      if (!d.isDirectory()) continue;
      const specPath = path.join(activeSpecsDir, d.name, 'spec.md');
      const wavePath = path.join(activeSpecsDir, d.name, 'wave-plan.md');
      const file = fs.existsSync(specPath) ? specPath : (fs.existsSync(wavePath) ? wavePath : null);
      if (!file) continue;
      const text = safe(() => fs.readFileSync(file, 'utf8')) || '';
      const hdr = parseSpecHeader(text);
      // Inline wave plans expose the live phase via pipeline-state JSON;
      // parent spec.md header is set at approval and never refreshed.
      const ps = readWavePlanState(d.name);
      const phaseRaw = (ps && (ps.phaseName || ps.phase)) || hdr.phase;
      const phase = phaseRaw ? phaseRaw.toUpperCase().replace(/[^A-Z_]/g, '_') : 'UNKNOWN';
      out.phaseDistribution[phase] = (out.phaseDistribution[phase] || 0) + 1;

      const cpm = text.match(/^###\s*Checkpoint:\s*(\d{4}-\d{2}-\d{2})/im);
      if (cpm) {
        const t = Date.parse(cpm[1]);
        if (!isNaN(t)) {
          const days = (Date.now() - t) / 86400000;
          if (days < 7) out.activeAging.lt7d++;
          else if (days <= 30) out.activeAging.d7_30++;
          else out.activeAging.gt30d++;
        }
      }

      // Detect "live now" — prefer harness events (closest to reality), fall
      // back to per-wave metrics files. Inline wave plans (no per-wave
      // metrics) rely on the event stream entirely.
      let liveLast = null;
      const activityByWave = harnessActivityByWave(d.name);
      if (activityByWave.size > 0) {
        let maxTs = 0, maxWave = null;
        for (const [w, t] of activityByWave.entries()) {
          if (t > maxTs) { maxTs = t; maxWave = w; }
        }
        liveLast = { t: new Date(maxTs).toISOString(), wave: maxWave };
      } else if (file.endsWith('wave-plan.md')) {
        const subs = safe(() => fs.readdirSync(path.join(activeSpecsDir, d.name), { withFileTypes: true })) || [];
        for (const s of subs) {
          if (!s.isDirectory()) continue;
          const mp = path.join(STATES_DIR, s.name + '.metrics.json');
          if (!fs.existsSync(mp)) continue;
          const m = safe(() => JSON.parse(fs.readFileSync(mp, 'utf8')));
          const u = m && m.metrics && m.metrics.updatedAt;
          if (u && (!liveLast || u > liveLast.t)) liveLast = { t: u, wave: s.name };
        }
      } else {
        const mp = path.join(STATES_DIR, d.name + '.metrics.json');
        if (fs.existsSync(mp)) {
          const m = safe(() => JSON.parse(fs.readFileSync(mp, 'utf8')));
          const u = m && m.metrics && m.metrics.updatedAt;
          if (u) liveLast = { t: u, wave: null };
        }
      }
      if (liveLast) {
        const ts = Date.parse(liveLast.t);
        // Defense vs phantom tagging: require the pipeline-state file itself to
        // be fresh, not just the event stream. Stale tagging of idle PLAN specs
        // (root cause of the false "Processando" banner) is now caught here too.
        const psPath = path.join(STATES_DIR, d.name + '.json');
        const psFresh = fs.existsSync(psPath) && (Date.now() - fs.statSync(psPath).mtimeMs) < 10 * 60 * 1000;
        if (!isNaN(ts) && (Date.now() - ts) < 5 * 60 * 1000 && psFresh) {
          out.activeNow.push({ spec: d.name, wave: liveLast.wave, lastActivity: liveLast.t });
        }
      }
    }
  }

  // Knowledge entries
  const knowledgePath = path.join(CLAUDE_DIR, 'knowledge.json');
  if (fs.existsSync(knowledgePath)) {
    const k = safe(() => JSON.parse(fs.readFileSync(knowledgePath, 'utf8')));
    if (k) {
      if (Array.isArray(k.entries)) out.knowledgeEntries = k.entries.length;
      else if (Array.isArray(k)) out.knowledgeEntries = k.length;
      else if (k.entries && typeof k.entries === 'object') out.knowledgeEntries = Object.keys(k.entries).length;
    }
  }

  // Storage breakdown (sizes only for key dirs, depth-first)
  function dirSize(p) {
    if (!fs.existsSync(p)) return 0;
    let total = 0;
    const stack = [p];
    while (stack.length) {
      const d = stack.pop();
      const ents = safe(() => fs.readdirSync(d, { withFileTypes: true })) || [];
      for (const e of ents) {
        const full = path.join(d, e.name);
        if (e.isDirectory()) stack.push(full);
        else { const st = safe(() => fs.statSync(full)); if (st) total += st.size; }
      }
    }
    return total;
  }
  out.storageBreakdown = {
    metrics: dirSize(path.join(CLAUDE_DIR, '.metrics')),
    harness: dirSize(path.join(CLAUDE_DIR, '.harness')),
    pipelineStates: dirSize(STATES_DIR),
    agentMemory: dirSize(path.join(CLAUDE_DIR, '.agent-memory')),
    spec: dirSize(SPEC_DIR),
    knowledge: fs.existsSync(knowledgePath) ? safe(() => fs.statSync(knowledgePath).size) || 0 : 0,
  };

  if (fs.existsSync(DETECT_CACHE)) {
    const dc = safe(() => JSON.parse(fs.readFileSync(DETECT_CACHE, 'utf8')));
    if (dc) out.detectInfo = { subprojects: (dc.subprojects || []).length, lastScan: dc.lastScan };
  }

  sendJson(res, 200, out);
}

function handleCommands(res) {
  sendJson(res, 200, { commands: COMMANDS, categories: CATEGORIES });
}

function handleInfo(res, port) {
  sendJson(res, 200, { root: ROOT, pid: process.pid, branch: BRANCH, port });
}

function handleProjects(res) {
  const projects = [{ name: '(root)', path: '.', role: 'root' }];
  if (fs.existsSync(DETECT_CACHE)) {
    const cache = safe(() => JSON.parse(fs.readFileSync(DETECT_CACHE, 'utf8')));
    if (cache && Array.isArray(cache.subprojects)) {
      for (const sub of cache.subprojects) {
        projects.push({ name: sub.name, path: sub.path, role: sub.role || 'general' });
      }
    }
  }
  sendJson(res, 200, { projects });
}

function handlePrdPost(res, body) {
  let payload;
  try { payload = JSON.parse(body || '{}'); }
  catch (_) { return sendJson(res, 400, { ok: false, error: 'invalid JSON' }); }

  const type = payload.type === 'bugfix' ? 'bugfix' : 'feature';
  const title = String(payload.title || '').trim();
  if (!title) return sendJson(res, 400, { ok: false, error: 'title required' });

  let slug = String(payload.slug || '').trim();
  if (!slug) slug = slugify(title);
  if (!slug) return sendJson(res, 400, { ok: false, error: 'unable to derive slug' });

  const date = new Date().toISOString().slice(0, 10);
  const dirName = `${date}-${slug}`;
  const targetDir = path.join(SPEC_DIR, 'active', dirName);

  if (fs.existsSync(targetDir)) {
    return sendJson(res, 409, { ok: false, error: `spec directory already exists: ${dirName}` });
  }

  let markdown;
  try {
    markdown = generatePrdMarkdown({
      type, title, slug,
      summary: payload.summary,
      why: payload.why,
      boundaries: payload.boundaries,
      checklist: payload.checklist,
      acceptanceCriteria: payload.acceptanceCriteria,
      decisionsNotObvious: payload.decisionsNotObvious,
      nonGoals: payload.nonGoals,
      scope: payload.scope,
      project: payload.project,
    });
  } catch (e) {
    return sendJson(res, 400, { ok: false, error: e.message });
  }

  try {
    fs.mkdirSync(targetDir, { recursive: true });
    fs.writeFileSync(path.join(targetDir, 'spec.md'), markdown, 'utf8');
  } catch (e) {
    return sendJson(res, 500, { ok: false, error: `write failed: ${e.message}` });
  }

  const relPath = path.relative(ROOT, path.join(targetDir, 'spec.md')).replace(/\\/g, '/');
  sendJson(res, 200, {
    ok: true,
    path: relPath,
    nextStep: `/mustard:resume ${dirName}`,
  });
}

// ── Icons (Heroicons outline 24x24, inline SVG) ───────────────────────

const ICONS = {
  home: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5"><path stroke-linecap="round" stroke-linejoin="round" d="M2.25 12l8.954-8.955c.44-.439 1.152-.439 1.591 0L21.75 12M4.5 9.75v10.125c0 .621.504 1.125 1.125 1.125H9.75v-4.875c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125V21h4.125c.621 0 1.125-.504 1.125-1.125V9.75M8.25 21h8.25" /></svg>`,
  document: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5"><path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" /></svg>`,
  chartBar: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5"><path stroke-linecap="round" stroke-linejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" /></svg>`,
  plus: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15" /></svg>`,
  sun: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5"><path stroke-linecap="round" stroke-linejoin="round" d="M12 3v2.25m6.364.386l-1.591 1.591M21 12h-2.25m-.386 6.364l-1.591-1.591M12 18.75V21m-4.773-4.227l-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0z" /></svg>`,
  moon: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5"><path stroke-linecap="round" stroke-linejoin="round" d="M21.752 15.002A9.718 9.718 0 0118 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 003 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 009.002-5.998z" /></svg>`,
  refresh: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4"><path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182m0-4.991v4.99" /></svg>`,
  chevronDown: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4"><path stroke-linecap="round" stroke-linejoin="round" d="M19.5 8.25l-7.5 7.5-7.5-7.5" /></svg>`,
  check: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4"><path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" /></svg>`,
  xMark: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" /></svg>`,
};

// ── Server ────────────────────────────────────────────────────────────

const BRANCH = readGitBranch();
let BOUND_PORT = hashPort(ROOT);

// ── SSE: realtime spec changes ────────────────────────────────────────
const sseClients = new Set();
const sseWatchers = [];
let sseHeartbeat = null;
let sseDebounceTimer = null;
let sseDebouncedSpecs = new Set();
let sseDebouncedPaths = new Set();

function sseSpecNameFromPath(p) {
  // p is absolute or relative; normalize separators
  const n = String(p || '').replace(/\\/g, '/');
  let m = n.match(/spec\/(?:active|completed)\/([^\/]+)/);
  if (m) return m[1];
  m = n.match(/\.pipeline-states\/([^\/]+)\.json$/);
  if (m) return m[1];
  return null;
}

function sseFlush() {
  sseDebounceTimer = null;
  const specNames = Array.from(sseDebouncedSpecs);
  const paths = Array.from(sseDebouncedPaths);
  sseDebouncedSpecs = new Set();
  sseDebouncedPaths = new Set();
  if (!specNames.length && !paths.length) return;
  const payload = `event: change\ndata: ${JSON.stringify({ ts: Date.now(), specNames, paths })}\n\n`;
  for (const res of Array.from(sseClients)) {
    try { res.write(payload); } catch (_) { sseClients.delete(res); }
  }
}

function sseWatch(dir) {
  if (!fs.existsSync(dir)) {
    console.log(`[mustard-dashboard] sse: skipping (not present) ${dir}`);
    return;
  }
  try {
    const w = fs.watch(dir, { recursive: true }, (_event, filename) => {
      const rel = filename ? String(filename).replace(/\\/g, '/') : '';
      const full = rel ? path.join(dir, rel).replace(/\\/g, '/') : dir.replace(/\\/g, '/');
      sseDebouncedPaths.add(full);
      const name = sseSpecNameFromPath(full) || sseSpecNameFromPath(rel);
      if (name) sseDebouncedSpecs.add(name);
      if (sseDebounceTimer) clearTimeout(sseDebounceTimer);
      sseDebounceTimer = setTimeout(sseFlush, 250);
    });
    w.on('error', (e) => console.log(`[mustard-dashboard] sse watcher error (${dir}): ${e.message}`));
    sseWatchers.push(w);
  } catch (e) {
    console.log(`[mustard-dashboard] sse: watch failed for ${dir}: ${e.message}`);
  }
}

function handleSpecsStream(req, res) {
  res.writeHead(200, {
    'Content-Type': 'text/event-stream',
    'Cache-Control': 'no-cache, no-transform',
    'Connection': 'keep-alive',
    'X-Accel-Buffering': 'no',
  });
  res.write(': connected\n\n');
  res.write(`event: hello\ndata: ${JSON.stringify({ ts: Date.now() })}\n\n`);
  sseClients.add(res);
  const cleanup = () => { sseClients.delete(res); try { res.end(); } catch (_) {} };
  req.on('close', cleanup);
  res.on('close', cleanup);
  req.on('error', cleanup);
}

function startSseInfra() {
  sseWatch(path.join(SPEC_DIR, 'active'));
  sseWatch(path.join(SPEC_DIR, 'completed'));
  sseWatch(STATES_DIR);
  sseHeartbeat = setInterval(() => {
    for (const res of Array.from(sseClients)) {
      try { res.write(': ping\n\n'); } catch (_) { sseClients.delete(res); }
    }
  }, 20000);
  if (sseHeartbeat.unref) sseHeartbeat.unref();
}

const server = http.createServer((req, res) => {
  const parsed = url.parse(req.url, true);
  const route = req.method + ' ' + parsed.pathname;
  const log = (s) => console.log(`[${new Date().toISOString()}] ${req.method} ${parsed.pathname} ${s}`);

  try {
    if (route === 'GET /') {
      const html = renderHtml(BRANCH, ROOT, BOUND_PORT);
      send(res, 200, 'text/html; charset=utf-8', html);
      return log(200);
    }
    if (route === 'GET /api/specs') { handleSpecs(res); return log(200); }
    if (route === 'GET /api/spec') { handleSpec(res, parsed.query); return log(200); }
    if (route === 'GET /api/metrics') { handleMetrics(res); return log(200); }
    if (route === 'GET /api/events') { handleEvents(res, parsed.query); return log(200); }
    if (route === 'GET /api/projects') { handleProjects(res); return log(200); }
    if (route === 'GET /api/commands') { handleCommands(res); return log(200); }
    if (route === 'GET /api/spec/live') { handleSpecLive(res, parsed.query); return log(200); }
    if (route === 'GET /api/telemetry-extra') { handleTelemetryExtra(res); return log(200); }
    if (route === 'GET /api/info') { handleInfo(res, BOUND_PORT); return log(200); }
    if (route === 'GET /api/specs/stream') { handleSpecsStream(req, res); return log(200); }
    if (route === 'GET /api/settings') { handleSettingsGet(res); return log(200); }
    if (route === 'POST /api/settings') {
      readBody(req, MAX_BODY, (err, body) => {
        if (err) { sendJson(res, 500, { ok: false, error: err.message }); return log(500); }
        try { handleSettingsPost(res, body); log(200); }
        catch (e) { sendJson(res, 500, { ok: false, error: e.message }); log(500); }
      });
      return;
    }

    if (route === 'POST /api/prd') {
      readBody(req, MAX_BODY, (err, body) => {
        if (err) {
          if (err.message === 'payload too large') {
            sendJson(res, 413, { ok: false, error: 'payload too large' });
            return log(413);
          }
          sendJson(res, 500, { ok: false, error: err.message });
          return log(500);
        }
        try { handlePrdPost(res, body); log(200); }
        catch (e) { sendJson(res, 500, { ok: false, error: e.message }); log(500); }
      });
      return;
    }

    sendJson(res, 404, { error: 'not found' });
    log(404);
  } catch (e) {
    try { sendJson(res, 500, { error: e.message }); } catch (_) {}
    log(500);
  }
});

// Try ports in order: hash(ROOT), hash(ROOT)+1, ..., wrapping inside [PORT_BASE, PORT_BASE+PORT_RANGE).
// On EADDRINUSE: probe /api/info — if that port already serves THIS ROOT, exit cleanly
// (someone else won the race / dashboard already running). If it serves a different root,
// move to the next candidate.
function tryListen(attempt) {
  if (attempt >= PORT_RANGE) {
    console.error(`[mustard-dashboard] no free port in [${PORT_BASE}, ${PORT_BASE + PORT_RANGE}) for ${ROOT}`);
    process.exit(1);
  }
  const candidate = PORT_BASE + ((hashPort(ROOT) - PORT_BASE + attempt) % PORT_RANGE);

  const onError = (err) => {
    if (err.code === 'EADDRINUSE') {
      probeInfo(candidate, (probeErr, info) => {
        if (!probeErr && info && info.root === ROOT) {
          console.log(`[mustard-dashboard] already running on http://${HOST}:${candidate} for ${ROOT} (pid ${info.pid})`);
          process.exit(0);
        }
        tryListen(attempt + 1);
      });
      return;
    }
    console.error(`[mustard-dashboard] server error: ${err.message}`);
    process.exit(1);
  };

  server.once('error', onError);
  server.listen(candidate, HOST, () => {
    server.removeListener('error', onError);
    BOUND_PORT = candidate;
    try { fs.writeFileSync(PID_FILE, String(process.pid), 'utf8'); } catch (_) {}
    try { fs.writeFileSync(PORT_FILE, String(candidate), 'utf8'); } catch (_) {}
    try { startSseInfra(); } catch (e) { console.log(`[mustard-dashboard] sse startup failed: ${e.message}`); }
    console.log(`[mustard-dashboard] listening on http://${HOST}:${candidate} (pid ${process.pid}, branch ${BRANCH}, root ${ROOT})`);
  });
}

tryListen(0);

function shutdown(signal) {
  console.log(`[mustard-dashboard] received ${signal}, shutting down`);
  try { if (fs.existsSync(PID_FILE)) fs.unlinkSync(PID_FILE); } catch (_) {}
  try { if (fs.existsSync(PORT_FILE)) fs.unlinkSync(PORT_FILE); } catch (_) {}
  try { if (sseHeartbeat) clearInterval(sseHeartbeat); } catch (_) {}
  for (const w of sseWatchers) { try { w.close(); } catch (_) {} }
  for (const res of Array.from(sseClients)) { try { res.end(); } catch (_) {} }
  sseClients.clear();
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(0), 2000).unref();
}

process.on('SIGINT', () => shutdown('SIGINT'));
process.on('SIGTERM', () => shutdown('SIGTERM'));
