#!/usr/bin/env bun
'use strict';
/**
 * metrics — unified CLI dispatcher for pipeline + hook metrics.
 *
 * Usage:
 *   metrics.js collect [--hooks-only]
 *   metrics.js report [--since <ISO>] [--event <type>] [--compare <from> <to>]
 *
 * Without a subcommand (or an unknown one): prints usage and exits 0.
 */

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const { getRtkGain } = require('./_rtk-gain.js');

const argv = process.argv.slice(2);
const subcommand = argv[0];

// ── Shared normalisation helpers ──────────────────────────────────────────────

const IGNORE_DIRS = new Set(['node_modules', '.git', 'dist', 'build', '.next', 'bin', 'obj']);

function normalizePath(p) {
  const abs = path.resolve(p);
  return process.platform === 'win32' ? abs.toLowerCase() : abs;
}

function discoverMetricsDirs(rootMetricsDir) {
  const out = [];
  const seen = new Set();
  function addDir(d) {
    const key = normalizePath(d);
    if (seen.has(key)) return;
    seen.add(key);
    out.push(d);
  }
  if (fs.existsSync(rootMetricsDir)) addDir(rootMetricsDir);
  const projectRoot = path.dirname(path.dirname(rootMetricsDir));
  function walk(dir, depth) {
    if (depth > 5) return;
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const ent of entries) {
      if (!ent.isDirectory()) continue;
      if (IGNORE_DIRS.has(ent.name) || ent.name.startsWith('.claude.backup')) continue;
      const sub = path.join(dir, ent.name);
      if (ent.name === '.claude') {
        const m = path.join(sub, '.metrics');
        if (fs.existsSync(m)) addDir(m);
        continue;
      }
      if (ent.name.startsWith('.')) continue;
      walk(sub, depth + 1);
    }
  }
  walk(projectRoot, 0);
  return out;
}

const ALWAYS_TRUSTED_EVENTS = new Set([
  'memory-auto-extract',
  'pre-compact',
  'spec-hygiene-move',
  'budget-check',
  'session-memory',
  'context-lazy-load',
  'skill-filter',
  'refs-filter',
]);

const EVENT_CATEGORY = {
  'auto-format': 'workflow',
  'bash-safety': 'prevention',
  'bash-native-redirect': 'redirection',
  'budget-check': 'prevention',
  'checklist-auto-mark': 'workflow',
  'close-gate': 'prevention',
  'enforce-registry': 'prevention',
  'memory-auto-extract': 'extraction',
  'model-routing-gate': 'routing',
  'pre-compact': 'extraction',
  'session-memory': 'extraction',
  'context-lazy-load': 'extraction',
  'skill-filter': 'extraction',
  'refs-filter': 'extraction',
  'delegation': 'isolation',
  'review-gate': 'prevention',
  'rtk-rewrite': 'rtk',
  'skill-size-gate': 'workflow',
  'skill-validate-gate': 'prevention',
  'spec-hygiene-move': 'extraction',
  'spec-size-gate': 'workflow',
  'tool-use-counter': 'prevention',
  'duplication-check': 'prevention',
  'convention-check': 'prevention',
  'file-guard': 'prevention',
  'guard-verify': 'prevention',
  'followup-cancel-gate': 'prevention',
  'output-budget': 'routing-advisory',
  'recommended-skills-audit': 'routing-advisory',
};

function aggregateHookEvents(metricsDir) {
  const result = { byEvent: {}, byDay: {}, byCategory: {}, total: 0 };
  const dirs = discoverMetricsDirs(metricsDir);
  for (const dir of dirs) {
    let files;
    try { files = fs.readdirSync(dir).filter(f => f.endsWith('.jsonl')); } catch { continue; }
    for (const file of files) {
      let content;
      try { content = fs.readFileSync(path.join(dir, file), 'utf8'); }
      catch { continue; }
      for (const raw of content.split('\n')) {
        const line = raw.trim();
        if (!line) continue;
        let entry;
        try { entry = JSON.parse(line); } catch { continue; }
        if (!entry.event) continue;
        const k = entry.event;
        const category = (typeof entry.category === 'string' && entry.category)
          || EVENT_CATEGORY[k]
          || 'other';
        if (!result.byEvent[k]) result.byEvent[k] = { count: 0, tokensAffected: 0, tokensSaved: 0, category };
        result.byEvent[k].count++;
        result.total++;
        if (typeof entry.tokens_affected === 'number') result.byEvent[k].tokensAffected += entry.tokens_affected;
        const trustTokens =
          entry.event !== 'rtk-rewrite' &&
          typeof entry.tokens_saved === 'number' &&
          ALWAYS_TRUSTED_EVENTS.has(entry.event);
        if (trustTokens) result.byEvent[k].tokensSaved += entry.tokens_saved;
        if (!result.byCategory[category]) result.byCategory[category] = { count: 0, tokensSaved: 0 };
        result.byCategory[category].count++;
        if (trustTokens) result.byCategory[category].tokensSaved += entry.tokens_saved;
        if (entry.ts) {
          const day = String(entry.ts).slice(0, 10);
          result.byDay[day] = (result.byDay[day] || 0) + 1;
        }
      }
    }
  }
  return result;
}

// ── Shared small helpers ──────────────────────────────────────────────────────

function readJson(p) {
  try { return JSON.parse(fs.readFileSync(p, 'utf8')); }
  catch { return null; }
}

function safe(fn) {
  try { return fn(); } catch { return null; }
}

function pct(ref, cur) {
  if (ref === 0) return cur === 0 ? '0%' : 'n/a';
  const d = ((cur - ref) / ref) * 100;
  return `${d > 0 ? '+' : ''}${d.toFixed(1)}%`;
}

function cell(ref, cur) {
  return `${ref}→${cur} (${pct(ref, cur)})`;
}

function formatMs(ms) {
  if (ms < 60000) return `${Math.round(ms / 1000)}s`;
  const m = Math.floor(ms / 60000);
  const s = Math.round((ms % 60000) / 1000);
  if (m < 60) return `${m}m${s}s`;
  const h = Math.floor(m / 60);
  return `${h}h${m % 60}m`;
}

function formatDuration(start, end) {
  return formatMs(end.getTime() - start.getTime());
}

// ── COLLECT subcommand ────────────────────────────────────────────────────────
// (formerly metrics-collect.js)

function runCollect(collectArgs) {
  const HOOKS_ONLY = collectArgs.includes('--hooks-only');

  const cwd = process.cwd();
  const claudeDir = path.join(cwd, '.claude');

  const specs = HOOKS_ONLY ? { active: [], orphaned: [] } : collectSpecs(claudeDir);
  const archives = HOOKS_ONLY ? [] : collectArchives(claudeDir);
  const hookEvents = aggregateHookEvents(path.join(claudeDir, '.metrics'));
  const rtk = safe(() => getRtkGain({ timeout: 3000 }));
  const weekly = HOOKS_ONLY ? null : buildWeekly(path.join(claudeDir, '.metrics'));

  const parts = [];
  parts.push('# Pipeline Metrics');
  parts.push('');

  const summaryLines = buildSummary({ specs, archives, hookEvents, rtk });
  if (summaryLines.length > 0) {
    parts.push('## Summary');
    for (const l of summaryLines) parts.push(l);
    parts.push('');
  }

  if (!HOOKS_ONLY) {
    renderSpecs(parts, specs.active, 'Active');
    renderSpecs(parts, specs.orphaned, 'Orphaned');
    if (specs.orphaned.length > 0) {
      parts.push(`> ${specs.orphaned.length} orphaned pipeline state(s) detected. Run \`/mustard:complete {spec-name}\` or \`/mustard:maint\` to reconcile.`);
      parts.push('');
    }
    renderArchives(parts, archives);
  }

  if (weekly && weekly.hasData) {
    parts.push('## Last 7 Days');
    parts.push('');
    parts.push('| Day | Events |');
    parts.push('|-----|--------|');
    for (const [day, count] of weekly.days) parts.push(`| ${day} | ${count} |`);
    parts.push('');
    if (weekly.delta) {
      parts.push(`- Current week: ${weekly.currentCount} events`);
      parts.push(`- Previous week: ${weekly.prevCount} events`);
      parts.push(`- Delta: ${weekly.delta}`);
      parts.push('');
    }
  }

  if (!HOOKS_ONLY) {
    const health = buildPipelineHealth({ specs, archives });
    if (health.totalSpecs > 0) {
      parts.push('## Pipeline Health');
      parts.push('');
      parts.push(`- Total pipelines tracked: ${health.totalSpecs} (active: ${health.activeCount} · archived: ${health.archivedCount})`);
      parts.push(`- Pass@1 (no hook retries): ${health.pass1Pct}% (${health.pass1Count}/${health.totalSpecs})`);
      if (health.avgDurationMs > 0) parts.push(`- Avg duration: ${formatMs(health.avgDurationMs)}`);
      if (health.avgApiCalls > 0) parts.push(`- Avg API calls per pipeline: ${health.avgApiCalls}`);
      if (health.avgRetries > 0) parts.push(`- Avg hook retries per pipeline: ${health.avgRetries}`);
      if (health.worstPhase) {
        parts.push(`- Worst phase: ${health.worstPhase.phase} (${health.worstPhase.totalRetries} total retries across ${health.worstPhase.affected} pipelines)`);
      }
      if (health.l0Direct + health.l0Delegated > 0) {
        parts.push(`- L0 delegation ratio: ${health.l0Pct}% (${health.l0Delegated} delegated / ${health.l0Direct} direct)`);
      }
      parts.push('');
    }

    const knowledge = readKnowledgeStats(path.join(claudeDir, 'knowledge.json'));
    const decisions = readMemoryStats(path.join(claudeDir, 'memory', 'decisions.json'));
    const lessons = readMemoryStats(path.join(claudeDir, 'memory', 'lessons.json'));
    if (knowledge.total > 0 || decisions.total > 0 || lessons.total > 0) {
      parts.push('## Knowledge Growth');
      parts.push('');
      if (knowledge.total > 0) parts.push(`- Knowledge entries: ${knowledge.total} (avg confidence: ${knowledge.avgConfidence})`);
      if (decisions.total > 0) parts.push(`- Decisions captured: ${decisions.total}`);
      if (lessons.total > 0) parts.push(`- Lessons learned: ${lessons.total}`);
      parts.push('');
    }
  }

  if (hookEvents.total > 0) {
    const cats = hookEvents.byCategory || {};
    const extract = cats['extraction'] || { count: 0, tokensSaved: 0 };
    const prevent = cats['prevention'] || { count: 0, tokensSaved: 0 };
    const workflow = cats['workflow']  || { count: 0, tokensSaved: 0 };
    const routing  = (cats['routing']?.count || 0) + (cats['routing-advisory']?.count || 0);
    const redirect = cats['redirection'] || { count: 0, tokensSaved: 0 };

    parts.push('## Token Economy (measured)');
    parts.push('');
    parts.push('Only deltas backed by real bytes/chars. Hooks of prevention/workflow/routing are surfaced separately as counts (not tokens).');
    parts.push('');
    if (rtk && rtk.saved > 0) {
      parts.push(`- **RTK** (CLI output filtering): ${Math.round(rtk.saved / 1000)}k tokens · ${Math.round(rtk.pct)}% rate · ${rtk.commands} commands`);
    }
    if (extract.tokensSaved > 0) {
      parts.push(`- **Extraction** (memory/pre-compact/spec-hygiene, bytes-based): ${Math.round(extract.tokensSaved / 1000)}k tokens · ${extract.count} events`);
    }
    if (prevent.tokensSaved > 0) {
      parts.push(`- **Prevention** (context-budget blocks, measured overflow): ${Math.round(prevent.tokensSaved / 1000)}k tokens · ${prevent.count} events`);
    }
    parts.push('');

    if (prevent.count > 0) {
      parts.push('## Incidents Prevented (counts, not tokens)');
      parts.push('');
      const preventEvents = Object.entries(hookEvents.byEvent)
        .filter(([, e]) => e.category === 'prevention')
        .sort((a, b) => b[1].count - a[1].count);
      parts.push('| Hook | Blocks |');
      parts.push('|------|--------|');
      for (const [k, e] of preventEvents) parts.push(`| ${k} | ${e.count} |`);
      parts.push('');
    }

    if (workflow.count > 0) {
      parts.push('## Workflow Automations (counts)');
      parts.push('');
      const wfEvents = Object.entries(hookEvents.byEvent)
        .filter(([, e]) => e.category === 'workflow')
        .sort((a, b) => b[1].count - a[1].count);
      parts.push('| Hook | Runs |');
      parts.push('|------|------|');
      for (const [k, e] of wfEvents) parts.push(`| ${k} | ${e.count} |`);
      parts.push('');
    }

    if (routing > 0 || redirect.count > 0) {
      parts.push('## Routing & Redirection (counts)');
      parts.push('');
      const rEvents = Object.entries(hookEvents.byEvent)
        .filter(([, e]) => e.category === 'routing' || e.category === 'routing-advisory' || e.category === 'redirection')
        .sort((a, b) => b[1].count - a[1].count);
      parts.push('| Hook | Events | Category |');
      parts.push('|------|--------|----------|');
      for (const [k, e] of rEvents) parts.push(`| ${k} | ${e.count} | ${e.category} |`);
      parts.push('');
    }

    parts.push('## All Hook Events (raw)');
    parts.push('');
    parts.push('| Event | Count | Category | Tokens Saved |');
    parts.push('|-------|-------|----------|--------------|');
    let tc = 0, ts = 0;
    for (const evt of Object.keys(hookEvents.byEvent).sort()) {
      const e = hookEvents.byEvent[evt];
      const sav = e.tokensSaved > 0 ? e.tokensSaved : '-';
      parts.push(`| ${evt} | ${e.count} | ${e.category} | ${sav} |`);
      tc += e.count;
      ts += e.tokensSaved;
    }
    parts.push('|-------|-------|----------|--------------|');
    parts.push(`| **TOTAL** | ${tc} | — | ${ts || '-'} |`);
    parts.push('');
  } else if (rtk && rtk.saved > 0) {
    parts.push('## Token Economy (measured)');
    parts.push('');
    parts.push(`- **RTK** (CLI output filtering): ${Math.round(rtk.saved / 1000)}k tokens · ${Math.round(rtk.pct)}% rate · ${rtk.commands} commands`);
    parts.push('');
  }

  if (parts.length <= 2) {
    parts.push('No metrics data found. Run a pipeline first.');
  }

  console.log(parts.join('\n'));
  process.exit(0);
}

// collect helpers ──────────────────────────────────────────────────────────────

function collectSpecs(claudeDir) {
  const statesDir = path.join(claudeDir, '.pipeline-states');
  const activeSpecDir = path.join(claudeDir, 'spec', 'active');
  const out = { active: [], orphaned: [] };
  if (!fs.existsSync(statesDir)) return out;

  let harnessViews = null;
  try { harnessViews = require('./event-projections.js'); } catch (_) {}

  let harnessEvents = [];
  if (harnessViews) {
    try {
      const eventsPath = path.join(claudeDir, '.harness', 'events.jsonl');
      harnessEvents = harnessViews.readEventsSync(eventsPath);
    } catch (_) {}
  }

  const seen = new Set();
  for (const f of fs.readdirSync(statesDir)) {
    if (f.endsWith('.json') && !f.endsWith('.metrics.json')) {
      seen.add(f.slice(0, -'.json'.length));
    }
  }

  for (const name of seen) {
    const mainPath = path.join(statesDir, `${name}.json`);
    const main = readJson(mainPath);

    let m = null;
    if (harnessViews && harnessEvents.length > 0) {
      try {
        const ps = harnessViews.buildPipelineState(harnessEvents, { spec: name });
        if (ps && ps.metrics && ps.metrics.apiCalls > 0) m = ps.metrics;
      } catch (_) {}
    }
    if (!m) m = (main && main.metrics) || null;
    if (!m) continue;

    const specPath = path.join(activeSpecDir, name);
    const isOrphaned = !fs.existsSync(specPath);
    const entry = { name, metrics: m, isOrphaned, main };
    (isOrphaned ? out.orphaned : out.active).push(entry);
  }
  return out;
}

function collectArchives(claudeDir) {
  const metricsDir = path.join(claudeDir, 'metrics');
  if (!fs.existsSync(metricsDir)) return [];
  const files = fs.readdirSync(metricsDir).filter(f => f.endsWith('.json'));
  const out = [];
  for (const f of files) {
    const data = readJson(path.join(metricsDir, f));
    if (!data) continue;
    out.push({ name: f.replace(/\.json$/, ''), metrics: data });
  }
  return out;
}

function buildWeekly(metricsDir) {
  const agg = aggregateHookEvents(metricsDir);
  if (agg.total === 0) return { hasData: false };
  const now = new Date();
  const days = [];
  for (let i = 6; i >= 0; i--) {
    const d = new Date(now.getTime() - i * 86400000);
    const key = d.toISOString().slice(0, 10);
    days.push([key, agg.byDay[key] || 0]);
  }
  let currentCount = 0, prevCount = 0;
  for (let i = 0; i < 7; i++) {
    const d = new Date(now.getTime() - i * 86400000).toISOString().slice(0, 10);
    currentCount += agg.byDay[d] || 0;
  }
  for (let i = 7; i < 14; i++) {
    const d = new Date(now.getTime() - i * 86400000).toISOString().slice(0, 10);
    prevCount += agg.byDay[d] || 0;
  }
  const delta = (currentCount || prevCount) ? cell(prevCount, currentCount) : null;
  return { hasData: days.some(d => d[1] > 0), days, currentCount, prevCount, delta };
}

function buildSummary({ specs, archives, hookEvents, rtk }) {
  const lines = [];
  const activeN = specs.active.length;
  const orphanN = specs.orphaned.length;
  const totalSpecs = activeN + orphanN;

  if (totalSpecs > 0) {
    lines.push(`→ ${totalSpecs} pipeline${totalSpecs === 1 ? '' : 's'} tracked (log) · ${archives.length} archived`);
  } else if (archives.length > 0) {
    lines.push(`→ ${archives.length} archived pipeline${archives.length === 1 ? '' : 's'}`);
  }

  if (orphanN > 0) {
    lines.push(`⚠ ${orphanN} orphaned state${orphanN === 1 ? '' : 's'} (spec not in active/) — run /mustard:maint`);
  }

  if (totalSpecs > 0) {
    let pass = 0;
    for (const group of [specs.active, specs.orphaned]) {
      for (const s of group) if ((s.metrics.retries || 0) === 0) pass++;
    }
    const p = Math.round((pass / totalSpecs) * 100);
    const prefix = p >= 80 ? '✓' : p >= 50 ? '→' : '⚠';
    lines.push(`${prefix} Pass@1 (hook-level): ${p}% (${pass}/${totalSpecs} without hook retries)`);
  }

  if (rtk && rtk.saved > 0) {
    lines.push(`✓ RTK savings: ~${Math.round(rtk.saved / 1000)}k tokens (${Math.round(rtk.pct)}%)`);
  }

  const alert = findTopAlert([...specs.active, ...specs.orphaned]);
  if (alert) lines.push(`⚠ ${alert}`);

  return lines;
}

function findTopAlert(allSpecs) {
  let worst = null;
  for (const s of allSpecs) {
    const phaseSrc = (s.metrics && s.metrics.dispatchFailuresByPhase) || {};
    for (const [phase, n] of Object.entries(phaseSrc)) {
      if (n >= 3 && (!worst || n > worst.n)) worst = { name: s.name, phase, n };
    }
  }
  if (!worst) return null;
  return `1 pipeline with ${worst.n} retries in ${worst.phase} (${worst.name})`;
}

function renderSpecs(parts, list, label) {
  const sorted = list.slice().sort((a, b) => b.name.localeCompare(a.name));
  for (const s of sorted) {
    const m = s.metrics;
    const duration = m.startedAt ? formatDuration(new Date(m.startedAt), new Date(m.updatedAt || Date.now())) : 'unknown';
    parts.push(`## ${label}: ${s.name}`);
    parts.push(`- Duration: ${duration}`);
    parts.push(`- API calls: ${m.apiCalls || 0}`);
    parts.push(`- Hook retries: ${m.retries || 0}`);

    if (m.toolBreakdown && Object.keys(m.toolBreakdown).length > 0) {
      const top = Object.entries(m.toolBreakdown)
        .sort((a, b) => b[1] - a[1])
        .slice(0, 3)
        .map(([t, n]) => `${t}:${n}`)
        .join(', ');
      parts.push(`- Top tools: ${top}`);
    }

    if (m.dispatchFailuresByPhase && Object.keys(m.dispatchFailuresByPhase).length > 0) {
      const entries = Object.entries(m.dispatchFailuresByPhase).map(([phase, n]) => {
        return `${phase}:${n}${n >= 3 ? ' ⚠' : ''}`;
      });
      parts.push(`- Dispatch failures by phase: ${entries.join(', ')}`);
    }

    if (m.gate_saves !== undefined) parts.push(`- Gate saves: ${m.gate_saves}`);
    if (m.wave_reentry !== undefined) parts.push(`- Wave reentries: ${m.wave_reentry}`);

    if (m.skillHits && Object.keys(m.skillHits).length > 0) {
      parts.push('- Skill hits:');
      for (const [agent, hits] of Object.entries(m.skillHits).sort()) {
        const p = hits.loaded > 0 ? Math.round((hits.read / hits.loaded) * 100) + '%' : '—';
        parts.push(`  - ${agent}: ${hits.read}/${hits.loaded} (${p})`);
      }
    }

    if (s.isOrphaned) {
      parts.push('- Spec: not in spec/active/ (likely completed without /mustard:complete)');
    }
    parts.push('');
  }
}

function renderArchives(parts, archives) {
  if (archives.length === 0) return;
  parts.push('## Completed Pipelines');
  parts.push('');

  let totalCalls = 0, totalRetries = 0, totalDurationMs = 0, count = 0;
  const sorted = archives.slice().sort((a, b) => b.name.localeCompare(a.name)).slice(0, 10);
  for (const a of sorted) {
    const m = a.metrics;
    const duration = m.durationMs ? formatMs(m.durationMs) : 'unknown';
    parts.push(`### ${a.name}`);
    parts.push(`- Duration: ${duration}`);
    parts.push(`- API calls: ${m.apiCalls || 0}`);
    parts.push(`- Hook retries: ${m.retries || 0}`);
    if (m.rtkSavings) {
      parts.push(`- RTK savings: ${m.rtkSavings.pct}% (${Math.round((m.rtkSavings.saved || 0) / 1000)}k tokens)`);
    }
    parts.push('');
    totalCalls += m.apiCalls || 0;
    totalRetries += m.retries || 0;
    totalDurationMs += m.durationMs || 0;
    count++;
  }
  if (count > 0) {
    parts.push(`## Averages (last ${count} pipelines)`);
    parts.push(`- Avg duration: ${formatMs(Math.round(totalDurationMs / count))}`);
    parts.push(`- Avg API calls: ${Math.round(totalCalls / count)}`);
    parts.push(`- Avg hook retries: ${Math.round(totalRetries / count)}`);
    parts.push('');
  }

  let pass1Count = 0, retrySum = 0;
  for (const a of archives) {
    if ((a.metrics.retries || 0) === 0) pass1Count++;
    retrySum += a.metrics.retries || 0;
  }
  const p = Math.round((pass1Count / archives.length) * 100);
  const avg = (retrySum / archives.length).toFixed(1);
  parts.push('## Pass@1 Metrics (archived)');
  parts.push(`- Pass@1 (hook-level): ${p}% (${pass1Count}/${archives.length} completed with zero hook retries)`);
  parts.push(`- Avg hook retries per pipeline: ${avg}`);
  parts.push('');
}

function buildPipelineHealth({ specs, archives }) {
  const allSpecs = [...specs.active, ...specs.orphaned, ...archives.map(a => ({ name: a.name, metrics: a.metrics }))];
  const totalSpecs = allSpecs.length;
  if (totalSpecs === 0) {
    return { totalSpecs: 0, activeCount: 0, archivedCount: 0, pass1Count: 0, pass1Pct: 0, avgDurationMs: 0, avgApiCalls: 0, avgRetries: 0, worstPhase: null, l0Pct: 0, l0Direct: 0, l0Delegated: 0 };
  }
  let pass1 = 0, totalDuration = 0, totalApiCalls = 0, totalRetries = 0;
  let l0Direct = 0, l0Delegated = 0;
  const phaseRetries = {}, phaseAffected = {};
  for (const s of allSpecs) {
    const m = s.metrics || {};
    if ((m.retries || 0) === 0) pass1++;
    if (m.durationMs) totalDuration += m.durationMs;
    else if (m.startedAt && m.updatedAt) totalDuration += new Date(m.updatedAt).getTime() - new Date(m.startedAt).getTime();
    totalApiCalls += m.apiCalls || 0;
    totalRetries += m.retries || 0;
    const tb = m.toolBreakdown || {};
    l0Direct += (tb.Bash || 0) + (tb.Edit || 0) + (tb.Write || 0);
    l0Delegated += (tb.Agent || 0) + (tb.Task || 0);
    const phaseSrc = m.dispatchFailuresByPhase || {};
    for (const [phase, n] of Object.entries(phaseSrc)) {
      if (typeof n !== 'number' || n <= 0) continue;
      phaseRetries[phase] = (phaseRetries[phase] || 0) + n;
      phaseAffected[phase] = (phaseAffected[phase] || 0) + 1;
    }
  }
  let worstPhase = null;
  for (const [phase, n] of Object.entries(phaseRetries)) {
    if (!worstPhase || n > worstPhase.totalRetries) {
      worstPhase = { phase, totalRetries: n, affected: phaseAffected[phase] };
    }
  }
  const l0Total = l0Direct + l0Delegated;
  const l0Pct = l0Total > 0 ? Math.round((l0Delegated / l0Total) * 100) : 0;
  return {
    totalSpecs,
    activeCount: specs.active.length + specs.orphaned.length,
    archivedCount: archives.length,
    pass1Count: pass1,
    pass1Pct: Math.round((pass1 / totalSpecs) * 100),
    avgDurationMs: Math.round(totalDuration / totalSpecs),
    avgApiCalls: Math.round(totalApiCalls / totalSpecs),
    avgRetries: Math.round((totalRetries / totalSpecs) * 10) / 10,
    worstPhase,
    l0Pct,
    l0Direct,
    l0Delegated,
  };
}

function readKnowledgeStats(p) {
  try {
    if (!fs.existsSync(p)) return { total: 0, avgConfidence: '0.0' };
    const kb = JSON.parse(fs.readFileSync(p, 'utf8'));
    const entries = Array.isArray(kb.entries) ? kb.entries : [];
    if (entries.length === 0) return { total: 0, avgConfidence: '0.0' };
    const sumConf = entries.reduce((s, e) => s + (typeof e.confidence === 'number' ? e.confidence : 0), 0);
    return { total: entries.length, avgConfidence: (sumConf / entries.length).toFixed(2) };
  } catch { return { total: 0, avgConfidence: '0.0' }; }
}

function readMemoryStats(p) {
  try {
    if (!fs.existsSync(p)) return { total: 0 };
    const data = JSON.parse(fs.readFileSync(p, 'utf8'));
    const entries = Array.isArray(data.entries) ? data.entries : [];
    return { total: entries.length };
  } catch { return { total: 0 }; }
}

// ── REPORT subcommand ─────────────────────────────────────────────────────────
// (formerly metrics-report.js)

function runReport(reportArgs) {
  const METRICS_DIR = process.env.MUSTARD_METRICS_DIR
    ? path.resolve(process.env.MUSTARD_METRICS_DIR)
    : path.join(process.cwd(), '.claude', '.metrics');

  let sinceFilter = null;
  let eventFilter = null;
  let compareFrom = null;
  let compareTo = null;

  for (let i = 0; i < reportArgs.length; i++) {
    const a = reportArgs[i];
    if (a === '--since' && reportArgs[i + 1]) { sinceFilter = new Date(reportArgs[++i]); continue; }
    if (a === '--event' && reportArgs[i + 1]) { eventFilter = reportArgs[++i]; continue; }
    if (a === '--compare') {
      if (!reportArgs[i + 1] || !reportArgs[i + 2]) {
        process.stderr.write('Error: --compare requires two arguments: --compare <from> <to>\n');
        process.exit(1);
      }
      compareFrom = reportArgs[++i];
      compareTo = reportArgs[++i];
      continue;
    }
  }

  if (sinceFilter && isNaN(sinceFilter.getTime())) {
    process.stderr.write('Error: --since value is not a valid date\n');
    process.exit(1);
  }

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

  function aggregateReport(entries) {
    const agg = {};
    for (const entry of entries) {
      const key = entry.event;
      if (!agg[key]) agg[key] = { count: 0, tokensAffected: 0, tokensSaved: 0, notes: new Set() };
      agg[key].count++;
      if (typeof entry.tokens_affected === 'number') agg[key].tokensAffected += entry.tokens_affected;
      if (typeof entry.tokens_saved === 'number' && entry.event !== 'rtk-rewrite') {
        agg[key].tokensSaved += entry.tokens_saved;
      }
      if (entry.note) agg[key].notes.add(entry.note);
    }
    return agg;
  }

  const TAG_RE = /^v?\d+\.\d+\.\d+$/;

  function resolveEndpoint(value) {
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

  if (compareFrom && compareTo) {
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

    const aggNew = aggregateReport(inNew);
    const aggRef = aggregateReport(inRef);
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

  // ── Default mode ────────────────────────────────────────────────────────────
  if (!fs.existsSync(METRICS_DIR)) {
    console.log('No metrics data yet');
    process.exit(0);
  }

  const files = fs.readdirSync(METRICS_DIR).filter(f => f.endsWith('.jsonl'));
  if (files.length === 0) {
    console.log('No metrics data yet');
    process.exit(0);
  }

  const agg = {};
  for (const file of files) {
    const filePath = path.join(METRICS_DIR, file);
    let content;
    try { content = fs.readFileSync(filePath, 'utf8'); } catch (_) { continue; }
    for (const raw of content.split('\n')) {
      const line = raw.trim();
      if (!line) continue;
      let entry;
      try { entry = JSON.parse(line); } catch (_) { continue; }
      if (!entry.event) continue;
      if (sinceFilter && entry.ts && new Date(entry.ts) < sinceFilter) continue;
      if (eventFilter && entry.event !== eventFilter) continue;
      const key = entry.event;
      if (!agg[key]) agg[key] = { count: 0, tokensAffected: 0, tokensSaved: 0, notes: new Set() };
      agg[key].count++;
      if (typeof entry.tokens_affected === 'number') agg[key].tokensAffected += entry.tokens_affected;
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

  const header = '| Event | Count | Tokens Affected | Tokens Saved | Notes |';
  const sep    = '|-------|-------|-----------------|--------------|-------|';
  console.log(header);
  console.log(sep);
  let totalSaved = 0, totalAffected = 0, totalCount = 0;
  for (const evt of events.sort()) {
    const { count, tokensAffected, tokensSaved, notes } = agg[evt];
    const noteStr = [...notes].slice(0, 2).join('; ') || '-';
    const affectedCell = tokensAffected > 0 ? tokensAffected : '-';
    const savedCell = tokensSaved > 0 ? tokensSaved : '-';
    console.log(`| ${evt} | ${count} | ${affectedCell} | ${savedCell} | ${noteStr} |`);
    totalSaved += tokensSaved;
    totalAffected += tokensAffected;
    totalCount += count;
  }
  console.log(sep);
  console.log(`| **TOTAL** | ${totalCount} | ${totalAffected || '-'} | ${totalSaved || '-'} | - |`);

  const rtk = (() => {
    try { return getRtkGain({ timeout: 5000 }); } catch (_) { return null; }
  })();

  if (rtk && rtk.saved > 0) {
    console.log('');
    console.log('## RTK Token Savings');
    console.log('');
    console.log(`| Metric | Value |`);
    console.log(`|--------|-------|`);
    console.log(`| Total tokens saved | ${rtk.saved.toLocaleString()} |`);
    if (rtk.originalTotal > 0) {
      console.log(`| Total original tokens | ${rtk.originalTotal.toLocaleString()} |`);
    }
    console.log(`| Savings rate | ${Math.round(rtk.pct)}% |`);
    if (rtk.commands > 0) {
      console.log(`| Commands rewritten | ${rtk.commands} |`);
    }

    if (rtk.byCommand && typeof rtk.byCommand === 'object') {
      const cmds = Object.entries(rtk.byCommand);
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

  if (agg['rtk-rewrite']) {
    const hookRewrites = agg['rtk-rewrite'].count;
    console.log('');
    console.log('## RTK Hook Activity');
    console.log(`| Metric | Value |`);
    console.log(`|--------|-------|`);
    console.log(`| Commands rewritten by hook | ${hookRewrites} |`);
  }

  process.exit(0);
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

if (subcommand === 'collect') {
  runCollect(argv.slice(1));
} else if (subcommand === 'report') {
  runReport(argv.slice(1));
} else {
  console.log('Usage:');
  console.log('  metrics.js collect [--hooks-only]');
  console.log('  metrics.js report [--since <ISO>] [--event <type>] [--compare <from> <to>]');
  process.exit(0);
}
