#!/usr/bin/env node
// metrics-report — aggregate enforcement metrics from .claude/.metrics/*.jsonl
// Usage:
//   node metrics-report.js [--since <ISO>] [--event <type>]
//   node metrics-report.js --compare <from> <to> [--since <ISO>] [--event <type>]
//
// <from>/<to> accept a git tag (regex ^v?\d+\.\d+\.\d+$) resolved via
//   `git show -s --format=%cI <tag>` or any ISO date parsable by Date().
// Future: `.claude/metrics/*.json` (pipeline-grained, written by metrics-collect.js)
// could feed a parallel compare mode — kept out of this script on purpose.
'use strict';
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const METRICS_DIR = process.env.MUSTARD_METRICS_DIR
  ? path.resolve(process.env.MUSTARD_METRICS_DIR)
  : path.join(process.cwd(), '.claude', '.metrics');

// ── Arg parsing ────────────────────────────────────────────────────────
const args = process.argv.slice(2);
let sinceFilter = null;
let eventFilter = null;
let compareFrom = null;
let compareTo = null;

for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === '--since' && args[i + 1]) { sinceFilter = new Date(args[++i]); continue; }
  if (a === '--event' && args[i + 1]) { eventFilter = args[++i]; continue; }
  if (a === '--compare') {
    if (!args[i + 1] || !args[i + 2]) {
      process.stderr.write('Error: --compare requires two arguments: --compare <from> <to>\n');
      process.exit(1);
    }
    compareFrom = args[++i];
    compareTo = args[++i];
    continue;
  }
}

if (sinceFilter && isNaN(sinceFilter.getTime())) {
  process.stderr.write('Error: --since value is not a valid date\n');
  process.exit(1);
}

// ── Shared helpers ─────────────────────────────────────────────────────
function readAllEvents() {
  if (!fs.existsSync(METRICS_DIR)) return [];
  const files = fs.readdirSync(METRICS_DIR).filter(f => f.endsWith('.jsonl'));
  const events = [];
  for (const file of files) {
    let content;
    try { content = fs.readFileSync(path.join(METRICS_DIR, file), 'utf8'); }
    catch (_) { continue; }
    for (const raw of content.split('\n')) {
      const line = raw.trim();
      if (!line) continue;
      let entry;
      try { entry = JSON.parse(line); } catch (_) { continue; }
      if (!entry.event) continue;
      events.push(entry);
    }
  }
  return events;
}

function passesFilters(entry) {
  if (sinceFilter && entry.ts && new Date(entry.ts) < sinceFilter) return false;
  if (eventFilter && entry.event !== eventFilter) return false;
  return true;
}

function aggregate(entries) {
  const agg = {};
  for (const entry of entries) {
    const key = entry.event;
    if (!agg[key]) agg[key] = { count: 0, tokensAffected: 0, tokensSaved: 0, notes: new Set() };
    agg[key].count++;
    if (typeof entry.tokens_affected === 'number') agg[key].tokensAffected += entry.tokens_affected;
    // PR1: rtk-rewrite tokens_saved era heurística; números reais vêm de rtk-gain.
    if (typeof entry.tokens_saved === 'number' && entry.event !== 'rtk-rewrite') {
      agg[key].tokensSaved += entry.tokens_saved;
    }
    if (entry.note) agg[key].notes.add(entry.note);
  }
  return agg;
}

// ── Compare mode ───────────────────────────────────────────────────────
const TAG_RE = /^v?\d+\.\d+\.\d+$/;

function resolveEndpoint(value) {
  // Returns { date: Date, source: 'tag'|'iso', raw: value }
  if (TAG_RE.test(value)) {
    let iso;
    try {
      iso = execFileSync('git', ['show', '-s', '--format=%cI', value], {
        encoding: 'utf8',
        timeout: 3000,
        stdio: ['ignore', 'pipe', 'pipe'],
      }).trim();
    } catch (err) {
      process.stderr.write(`Error: could not resolve git tag "${value}" (is git available and the tag present?)\n`);
      process.exit(1);
    }
    const d = new Date(iso);
    if (isNaN(d.getTime())) {
      process.stderr.write(`Error: git returned unparseable date for "${value}": ${iso}\n`);
      process.exit(1);
    }
    return { date: d, source: 'tag', raw: value };
  }
  const d = new Date(value);
  if (isNaN(d.getTime())) {
    process.stderr.write(`Error: "${value}" is not a valid git tag (expected vX.Y.Z) or ISO date\n`);
    process.exit(1);
  }
  return { date: d, source: 'iso', raw: value };
}

function pct(ref, cur) {
  if (ref === 0) {
    if (cur === 0) return '0%';
    return 'n/a';
  }
  const delta = ((cur - ref) / ref) * 100;
  const sign = delta > 0 ? '+' : '';
  return `${sign}${delta.toFixed(1)}%`;
}

function cell(ref, cur) {
  return `${ref}→${cur} (${pct(ref, cur)})`;
}

function runCompare() {
  const fromEp = resolveEndpoint(compareFrom);
  const toEp = resolveEndpoint(compareTo);

  if (fromEp.date >= toEp.date) {
    process.stderr.write(`Error: --compare <from> must be earlier than <to> (got ${fromEp.date.toISOString()} >= ${toEp.date.toISOString()})\n`);
    process.exit(1);
  }

  const newWindow = { start: fromEp.date, end: toEp.date };
  const duration = newWindow.end.getTime() - newWindow.start.getTime();
  const refWindow = {
    start: new Date(newWindow.start.getTime() - duration),
    end: new Date(newWindow.start.getTime()),
  };

  const all = readAllEvents().filter(passesFilters);
  const inNew = all.filter(e => {
    if (!e.ts) return false;
    const t = new Date(e.ts);
    return t >= newWindow.start && t < newWindow.end;
  });
  const inRef = all.filter(e => {
    if (!e.ts) return false;
    const t = new Date(e.ts);
    return t >= refWindow.start && t < refWindow.end;
  });

  if (inNew.length === 0 && inRef.length === 0) {
    console.log('No metrics data in the given windows');
    process.exit(0);
  }

  const refSparse = inRef.length < 5;
  if (refSparse) {
    process.stderr.write(
      `Warning: reference window [${refWindow.start.toISOString()}, ${refWindow.end.toISOString()}) has only ${inRef.length} event(s) (<5). Delta columns may be noisy; showing new-window report anyway.\n`
    );
  }

  const aggNew = aggregate(inNew);
  const aggRef = aggregate(inRef);
  const keys = Array.from(new Set([...Object.keys(aggNew), ...Object.keys(aggRef)])).sort();

  console.log('## Compare');
  console.log('');
  console.log(`- Reference window: ${refWindow.start.toISOString()} → ${refWindow.end.toISOString()} (${inRef.length} events)`);
  console.log(`- New window:       ${newWindow.start.toISOString()} → ${newWindow.end.toISOString()} (${inNew.length} events)`);
  console.log(`- From: ${fromEp.raw} (${fromEp.source})   To: ${toEp.raw} (${toEp.source})`);
  if (refSparse) console.log(`- Note: reference history sparse (<5 events) — deltas advisory only`);
  console.log('');

  console.log('| Event | Count (ref→new, Δ%) | TokensAffected (ref→new, Δ%) | TokensSaved (ref→new, Δ%) |');
  console.log('|-------|---------------------|------------------------------|---------------------------|');
  let tRefC = 0, tNewC = 0, tRefA = 0, tNewA = 0, tRefS = 0, tNewS = 0;
  for (const evt of keys) {
    const r = aggRef[evt] || { count: 0, tokensAffected: 0, tokensSaved: 0 };
    const n = aggNew[evt] || { count: 0, tokensAffected: 0, tokensSaved: 0 };
    tRefC += r.count;      tNewC += n.count;
    tRefA += r.tokensAffected; tNewA += n.tokensAffected;
    tRefS += r.tokensSaved;    tNewS += n.tokensSaved;
    console.log(`| ${evt} | ${cell(r.count, n.count)} | ${cell(r.tokensAffected, n.tokensAffected)} | ${cell(r.tokensSaved, n.tokensSaved)} |`);
  }
  console.log('|-------|---------------------|------------------------------|---------------------------|');
  console.log(`| **TOTAL** | ${cell(tRefC, tNewC)} | ${cell(tRefA, tNewA)} | ${cell(tRefS, tNewS)} |`);

  process.exit(0);
}

if (compareFrom && compareTo) {
  runCompare();
  // runCompare exits — control never reaches default path
}

// ── Default mode (backward-compatible) ─────────────────────────────────
if (!fs.existsSync(METRICS_DIR)) {
  console.log('No metrics data yet');
  process.exit(0);
}

const files = fs.readdirSync(METRICS_DIR).filter(f => f.endsWith('.jsonl'));
if (files.length === 0) {
  console.log('No metrics data yet');
  process.exit(0);
}

// Aggregate: { event -> { count, tokensAffected, tokensSaved, notes: Set } }
const agg = {};

for (const file of files) {
  const filePath = path.join(METRICS_DIR, file);
  let content;
  try { content = fs.readFileSync(filePath, 'utf8'); } catch (_) { continue; }
  for (const raw of content.split('\n')) {
    const line = raw.trim();
    if (!line) continue;
    let entry;
    try { entry = JSON.parse(line); } catch (_) { continue; } // skip malformed
    if (!entry.event) continue;
    if (sinceFilter && entry.ts && new Date(entry.ts) < sinceFilter) continue;
    if (eventFilter && entry.event !== eventFilter) continue;
    const key = entry.event;
    if (!agg[key]) agg[key] = { count: 0, tokensAffected: 0, tokensSaved: 0, notes: new Set() };
    agg[key].count++;
    if (typeof entry.tokens_affected === 'number') agg[key].tokensAffected += entry.tokens_affected;
    // PR1: rtk-rewrite tokens_saved era heurística; números reais vêm de rtk-gain.
    if (typeof entry.tokens_saved === 'number' && entry.event !== 'rtk-rewrite') {
      agg[key].tokensSaved += entry.tokens_saved;
    }
    if (entry.note) agg[key].notes.add(entry.note);
  }
}

const events = Object.keys(agg);
if (events.length === 0) {
  console.log('No metrics data yet');
  process.exit(0);
}

// Render markdown table
const header = '| Event | Count | Tokens Affected | Tokens Saved | Notes |';
const sep    = '|-------|-------|-----------------|--------------|-------|';
console.log(header);
console.log(sep);
let totalSaved = 0;
let totalAffected = 0;
let totalCount = 0;
for (const evt of events.sort()) {
  const { count, tokensAffected, tokensSaved, notes } = agg[evt];
  const noteStr = [...notes].slice(0, 2).join('; ') || '-';
  // When the event records "affected" but no "saved" (e.g. rtk-rewrite,
  // budget-check passing), surface the affected count instead of `-`.
  const affectedCell = tokensAffected > 0 ? tokensAffected : '-';
  const savedCell = tokensSaved > 0 ? tokensSaved : '-';
  console.log(`| ${evt} | ${count} | ${affectedCell} | ${savedCell} | ${noteStr} |`);
  totalSaved += tokensSaved;
  totalAffected += tokensAffected;
  totalCount += count;
}
console.log(sep);
console.log(`| **TOTAL** | ${totalCount} | ${totalAffected || '-'} | ${totalSaved || '-'} | - |`);

// ── RTK Integration ────────────────────────────────────────────────────
// Query RTK for actual savings data (if RTK is installed)
try {
  let rtkAvailable = false;
  try {
    if (process.platform === 'win32') {
      execFileSync('where', ['rtk'], { stdio: 'ignore' });
    } else {
      execFileSync('which', ['rtk'], { stdio: 'ignore' });
    }
    rtkAvailable = true;
  } catch (_) {}

  if (rtkAvailable) {
    const rtkRaw = execFileSync('rtk', ['gain', '--all', '--format', 'json'], {
      encoding: 'utf8',
      timeout: 5000,
      stdio: ['pipe', 'pipe', 'ignore'],
    });
    const rtkData = JSON.parse(rtkRaw);

    console.log('');
    console.log('## RTK Token Savings');
    console.log('');

    if (rtkData.total_saved !== undefined) {
      const totalSaved = rtkData.total_saved || 0;
      const totalOriginal = rtkData.total_original || 0;
      const pct = totalOriginal > 0 ? Math.round((totalSaved / totalOriginal) * 100) : 0;
      console.log(`| Metric | Value |`);
      console.log(`|--------|-------|`);
      console.log(`| Total tokens saved | ${totalSaved.toLocaleString()} |`);
      console.log(`| Total original tokens | ${totalOriginal.toLocaleString()} |`);
      console.log(`| Savings rate | ${pct}% |`);
      console.log(`| Commands rewritten | ${rtkData.total_commands || '-'} |`);
    }

    // Per-command breakdown if available
    if (rtkData.by_command && typeof rtkData.by_command === 'object') {
      const cmds = Object.entries(rtkData.by_command);
      if (cmds.length > 0) {
        console.log('');
        console.log('### By Command');
        console.log('| Command | Saved | Original | Rate |');
        console.log('|---------|-------|----------|------|');
        for (const [cmd, stats] of cmds.sort((a, b) => (b[1].saved || 0) - (a[1].saved || 0)).slice(0, 10)) {
          const saved = stats.saved || 0;
          const orig = stats.original || 0;
          const rate = orig > 0 ? Math.round((saved / orig) * 100) + '%' : '-';
          console.log(`| ${cmd} | ${saved.toLocaleString()} | ${orig.toLocaleString()} | ${rate} |`);
        }
      }
    }
  }
} catch (_) {
  // RTK not installed or gain command failed — skip section silently
}

// ── Correlation: hook rewrites vs RTK actual savings ──────────────────
if (agg['rtk-rewrite']) {
  const hookRewrites = agg['rtk-rewrite'].count;
  const hookEstimatedSaved = agg['rtk-rewrite'].tokensSaved;
  console.log('');
  console.log('## RTK Hook Activity');
  console.log(`| Metric | Value |`);
  console.log(`|--------|-------|`);
  console.log(`| Commands rewritten by hook | ${hookRewrites} |`);
  console.log(`| Estimated tokens saved | ${hookEstimatedSaved > 0 ? hookEstimatedSaved.toLocaleString() : '-'} |`);
}
