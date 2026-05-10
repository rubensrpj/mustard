#!/usr/bin/env node
'use strict';

process.title = 'mustard-dashboard';

const http = require('http');
const fs = require('fs');
const path = require('path');
const url = require('url');
const { execFileSync } = require('child_process');

const { generatePrdMarkdown, slugify } = require('./dashboard-prd-template.js');
const { renderHtml } = require('./dashboard-ui.js');
const { ENV_CATALOG, isKnownKey, isValidValue, defaultsMap } = require('./dashboard-env-catalog.js');
const { COMMANDS, CATEGORIES } = require('./dashboard-commands-catalog.js');

const PORT = 7878;
const HOST = '127.0.0.1';
const ROOT = process.cwd();
const CLAUDE_DIR = path.join(ROOT, '.claude');
const PID_FILE = path.join(CLAUDE_DIR, '.dashboard.pid');
const SPEC_DIR = path.join(CLAUDE_DIR, 'spec');
const STATES_DIR = path.join(CLAUDE_DIR, '.pipeline-states');
const EVENTS_FILE = path.join(CLAUDE_DIR, '.harness', 'events.jsonl');
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

// Parse checklist items from markdown text
function parseChecklist(text) {
  const lines = text.split(/\r?\n/);
  const items = [];
  let inChecklist = false;
  for (const line of lines) {
    if (/^##\s+Checklist\s*$/i.test(line)) { inChecklist = true; continue; }
    if (inChecklist && /^##\s+/.test(line)) { inChecklist = false; continue; }
    if (inChecklist) {
      const done = line.match(/^\s*-\s+\[x\]\s+(.+)$/i);
      const pending = line.match(/^\s*-\s+\[\s\]\s+(.+)$/i);
      if (done) items.push({ text: done[1].trim(), done: true });
      else if (pending) items.push({ text: pending[1].trim(), done: false });
    }
  }
  const total = items.length;
  const doneCount = items.filter(i => i.done).length;
  return { total, done: doneCount, percent: total > 0 ? Math.round((doneCount / total) * 100) : 0, items };
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
  for (let i = 0; i < Math.min(lines.length, 30); i++) {
    const line = lines[i];
    if (/^###\s*Status:/i.test(line)) {
      const parts = line.replace(/^###\s*/i, '').split('|');
      const kv = {};
      for (const p of parts) {
        const m = p.match(/^\s*([^:]+?)\s*:\s*(.+?)\s*$/);
        if (m) kv[m[1].toLowerCase()] = m[2].trim();
      }
      if (kv.status) result.status = kv.status;
      if (kv.phase) result.phase = kv.phase;
      if (kv.scope) result.scope = kv.scope;
      if (kv.wave) result.wave = kv.wave;
      continue;
    }
    const cp = line.match(/^###\s*Checkpoint:\s*(.+)$/i);
    if (cp) { result.checkpoint = cp[1].trim(); continue; }
  }

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
  sendJson(res, 200, parsed);
}

function parseMetricsMarkdown(md) {
  const out = {
    summary: [],
    hookEvents: [],
    rtkSavings: { tokens: 0, rate: 0, commands: 0 },
    last7Days: [],
  };
  const lines = md.split(/\r?\n/);
  let section = null;

  for (let i = 0; i < lines.length; i++) {
    const ln = lines[i];
    if (/^##\s+Summary/i.test(ln)) { section = 'summary'; continue; }
    if (/^##\s+Last 7 Days/i.test(ln)) { section = 'days'; continue; }
    if (/^##\s+Enforcement Events/i.test(ln)) { section = 'hooks'; continue; }
    if (/^##\s+RTK Token Economy/i.test(ln)) { section = 'rtk'; continue; }
    if (/^##\s+/.test(ln)) { section = null; continue; }

    if (section === 'summary' && ln.trim()) {
      out.summary.push(ln.trim());
    } else if (section === 'days' && /^\|\s*\d{4}-\d{2}-\d{2}/.test(ln)) {
      const cells = ln.split('|').map(s => s.trim()).filter(Boolean);
      if (cells.length >= 2) {
        const events = parseInt(cells[1], 10);
        if (!Number.isNaN(events)) out.last7Days.push({ day: cells[0], events });
      }
    } else if (section === 'hooks' && ln.startsWith('|') && !/^\|\s*-+/.test(ln)) {
      const cells = ln.split('|').map(s => s.trim());
      if (cells.length >= 5 && cells[1] && cells[1] !== 'Event' && !cells[1].startsWith('**TOTAL')) {
        const count = parseInt(cells[2], 10);
        if (!Number.isNaN(count)) {
          out.hookEvents.push({
            event: cells[1],
            count,
            tokensAffected: cells[3] === '-' ? 0 : parseInt(cells[3], 10) || 0,
            tokensSaved: cells[4] === '-' ? 0 : parseInt(cells[4], 10) || 0,
          });
        }
      }
    } else if (section === 'rtk') {
      const saved = ln.match(/Total saved:\s*([\d.]+)k tokens/i);
      if (saved) out.rtkSavings.tokens = Math.round(parseFloat(saved[1]) * 1000);
      const rate = ln.match(/Savings rate:\s*(\d+)%/i);
      if (rate) out.rtkSavings.rate = parseInt(rate[1], 10);
      const cmds = ln.match(/Commands rewritten:\s*(\d+)/i);
      if (cmds) out.rtkSavings.commands = parseInt(cmds[1], 10);
    }
  }
  return out;
}

function handleEvents(res, query) {
  const requested = parseInt(query.n, 10);
  let n = Number.isNaN(requested) ? 200 : requested;
  if (n < 1) n = 1;
  if (n > 1000) n = 1000;

  if (!fs.existsSync(EVENTS_FILE)) return sendJson(res, 200, { events: [] });

  let content;
  try { content = fs.readFileSync(EVENTS_FILE, 'utf8'); }
  catch (e) { return sendJson(res, 500, { error: e.message }); }

  const lines = content.split(/\r?\n/).filter(Boolean);
  const tail = lines.slice(-n);
  const events = [];
  for (const line of tail) {
    try { events.push(JSON.parse(line)); } catch (_) {}
  }
  sendJson(res, 200, { events });
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

  // Aggregate metrics across candidates
  let phase = null, lastActivity = null, apiCalls = 0, retries = 0, hasMetrics = false;
  let agentAttempts = {}, toolBreakdown = {};
  for (const c of candidates) {
    const p = path.join(STATES_DIR, c + '.metrics.json');
    if (!fs.existsSync(p)) continue;
    const m = safe(() => JSON.parse(fs.readFileSync(p, 'utf8')));
    if (!m || !m.metrics) continue;
    hasMetrics = true;
    if (m.previousPhase && !phase) phase = m.previousPhase;
    if (m.metrics.updatedAt && (!lastActivity || m.metrics.updatedAt > lastActivity)) lastActivity = m.metrics.updatedAt;
    if (m.metrics.apiCalls != null) apiCalls += m.metrics.apiCalls;
    if (m.metrics.retries != null) retries += m.metrics.retries;
    if (m.metrics.agentAttempts) for (const k of Object.keys(m.metrics.agentAttempts)) agentAttempts[k] = (agentAttempts[k] || 0) + m.metrics.agentAttempts[k];
    if (m.metrics.toolBreakdown) for (const k of Object.keys(m.metrics.toolBreakdown)) toolBreakdown[k] = (toolBreakdown[k] || 0) + m.metrics.toolBreakdown[k];
  }

  // Match harness events: target is one of the candidate names OR the full path
  const matchSet = new Set(candidates);
  matchSet.add(specName);
  if (specName.indexOf('/') >= 0) matchSet.add(specName.split('/')[0]); // epic name too

  const events = [];
  if (fs.existsSync(EVENTS_FILE)) {
    const content = safe(() => fs.readFileSync(EVENTS_FILE, 'utf8')) || '';
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

  let isLive = false;
  if (lastActivity) {
    const t = Date.parse(lastActivity);
    if (!isNaN(t)) isLive = (Date.now() - t) < 5 * 60 * 1000;
  }

  // Fallback: also read spec.md / wave-plan.md content so the panel always shows context
  let specMd = null, specRelPath = null, summary = '', checklist = null;
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
    const idx = text.split(/\r?\n/).findIndex(l => /^##\s+Summary\s*$/i.test(l));
    if (idx >= 0) {
      const buf = []; const lines = text.split(/\r?\n/);
      for (let i = idx + 1; i < lines.length; i++) { if (/^##\s+/.test(lines[i])) break; buf.push(lines[i]); }
      summary = buf.join('\n').trim();
    }
    const items = []; let inCl = false;
    for (const line of text.split(/\r?\n/)) {
      if (/^##\s+Checklist\s*$/i.test(line)) { inCl = true; continue; }
      if (inCl && /^##\s+/.test(line)) break;
      if (!inCl) continue;
      const dn = line.match(/^\s*-\s+\[x\]\s+(.+)$/i);
      const pn = line.match(/^\s*-\s+\[\s\]\s+(.+)$/i);
      if (dn) items.push({ text: dn[1].trim(), done: true });
      else if (pn) items.push({ text: pn[1].trim(), done: false });
    }
    if (items.length) {
      const dc = items.filter(i => i.done).length;
      checklist = { total: items.length, done: dc, percent: Math.round((dc / items.length) * 100), items };
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

  sendJson(res, 200, {
    events: events.slice(-80),
    phase,
    lastActivity,
    apiCalls: hasMetrics ? apiCalls : null,
    retries: hasMetrics ? retries : null,
    agentAttempts: hasMetrics ? agentAttempts : null,
    toolBreakdown: hasMetrics ? toolBreakdown : null,
    candidates,
    isLive,
    summary,
    checklist,
    specPath: specRelPath,
  });
}

function handleTelemetryExtra(res) {
  const out = {
    pipelineAggregates: { totalApiCalls: 0, totalRetries: 0, agentAttempts: {}, toolBreakdown: {}, runs: 0, pass1: 0 },
    phaseDistribution: {},
    knowledgeEntries: 0,
    activeAging: { lt7d: 0, d7_30: 0, gt30d: 0 },
    storageBreakdown: {},
    detectInfo: null,
    activeNow: [],
  };

  // Pipeline metrics aggregation (from .pipeline-states/*.metrics.json)
  if (fs.existsSync(STATES_DIR)) {
    const files = safe(() => fs.readdirSync(STATES_DIR)) || [];
    for (const f of files) {
      if (!f.endsWith('.metrics.json')) continue;
      const m = safe(() => JSON.parse(fs.readFileSync(path.join(STATES_DIR, f), 'utf8')));
      if (!m || !m.metrics) continue;
      out.pipelineAggregates.runs++;
      out.pipelineAggregates.totalApiCalls += m.metrics.apiCalls || 0;
      out.pipelineAggregates.totalRetries += m.metrics.retries || 0;
      if ((m.metrics.retries || 0) === 0) out.pipelineAggregates.pass1++;
      if (m.metrics.agentAttempts) for (const k of Object.keys(m.metrics.agentAttempts)) out.pipelineAggregates.agentAttempts[k] = (out.pipelineAggregates.agentAttempts[k] || 0) + m.metrics.agentAttempts[k];
      if (m.metrics.toolBreakdown) for (const k of Object.keys(m.metrics.toolBreakdown)) out.pipelineAggregates.toolBreakdown[k] = (out.pipelineAggregates.toolBreakdown[k] || 0) + m.metrics.toolBreakdown[k];
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
      const phaseM = text.match(/^###\s*Status:[^\n]*?Phase:\s*([A-Z_]+)/im);
      const phase = phaseM ? phaseM[1].toUpperCase() : 'UNKNOWN';
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

      // Detect "live now" — look at sub-wave metrics if epic, else direct
      let liveLast = null;
      if (file.endsWith('wave-plan.md')) {
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
        if (!isNaN(ts) && (Date.now() - ts) < 5 * 60 * 1000) {
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

const server = http.createServer((req, res) => {
  const parsed = url.parse(req.url, true);
  const route = req.method + ' ' + parsed.pathname;
  const log = (s) => console.log(`[${new Date().toISOString()}] ${req.method} ${parsed.pathname} ${s}`);

  try {
    if (route === 'GET /') {
      const html = renderHtml(BRANCH);
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

server.on('error', (err) => {
  if (err.code === 'EADDRINUSE') {
    console.error(`[mustard-dashboard] port ${PORT} already in use. Stop the other process or run "/mustard:dashboard stop".`);
  } else {
    console.error(`[mustard-dashboard] server error: ${err.message}`);
  }
  process.exit(1);
});

server.listen(PORT, HOST, () => {
  try { fs.writeFileSync(PID_FILE, String(process.pid), 'utf8'); } catch (_) {}
  console.log(`[mustard-dashboard] listening on http://${HOST}:${PORT} (pid ${process.pid}, branch ${BRANCH})`);
});

function shutdown(signal) {
  console.log(`[mustard-dashboard] received ${signal}, shutting down`);
  try { if (fs.existsSync(PID_FILE)) fs.unlinkSync(PID_FILE); } catch (_) {}
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(0), 2000).unref();
}

process.on('SIGINT', () => shutdown('SIGINT'));
process.on('SIGTERM', () => shutdown('SIGTERM'));
