#!/usr/bin/env node
'use strict';
/**
 * METRICS-COLLECT: Unified pipeline + hook + RTK metrics view.
 *
 * Sources:
 * - .claude/.harness/events.jsonl                 (harness log; metrics aggregated via buildPipelineState)
 * - .claude/.pipeline-states/{spec}.json          (main state; phase + orphan detection)
 * - .claude/metrics/{spec}.json                   (archived pipelines, written by /complete)
 * - .claude/.metrics/*.jsonl                      (hook enforcement events)
 * - `rtk gain --all --format json`                (token economy, via _rtk-gain helper)
 *
 * Flags:
 *   --hooks-only   Emit only Summary + Enforcement Events + RTK (skip per-spec sections)
 *
 * Output: Markdown to stdout. Summary block first (5–8 lines with
 * ✓/⚠/→ prefixes), then drill-down sections.
 */

const fs = require('fs');
const path = require('path');
const { getRtkGain } = require('./_rtk-gain.js');

const args = process.argv.slice(2);
const HOOKS_ONLY = args.includes('--hooks-only');

function main() {
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

  // ── Summary (always first) ───────────────────────────────────────────
  const summaryLines = buildSummary({ specs, archives, hookEvents, rtk });
  if (summaryLines.length > 0) {
    parts.push('## Summary');
    for (const l of summaryLines) parts.push(l);
    parts.push('');
  }

  // ── Per-spec drill-down ──────────────────────────────────────────────
  if (!HOOKS_ONLY) {
    renderSpecs(parts, specs.active, 'Active');
    renderSpecs(parts, specs.orphaned, 'Orphaned');
    if (specs.orphaned.length > 0) {
      parts.push(`> ${specs.orphaned.length} orphaned pipeline state(s) detected. Run \`/mustard:complete {spec-name}\` or \`/mustard:maint\` to reconcile.`);
      parts.push('');
    }
    renderArchives(parts, archives);
  }

  // ── Last 7 Days (temporal dimension) ─────────────────────────────────
  if (weekly && weekly.hasData) {
    parts.push('## Last 7 Days');
    parts.push('');
    parts.push('| Day | Events |');
    parts.push('|-----|--------|');
    for (const [day, count] of weekly.days) {
      parts.push(`| ${day} | ${count} |`);
    }
    parts.push('');
    if (weekly.delta) {
      parts.push(`- Current week: ${weekly.currentCount} events`);
      parts.push(`- Previous week: ${weekly.prevCount} events`);
      parts.push(`- Delta: ${weekly.delta}`);
      parts.push('');
    }
  }

  // ── Enforcement Events (hooks) ───────────────────────────────────────
  if (hookEvents.total > 0) {
    parts.push('## Enforcement Events (hooks)');
    parts.push('');
    parts.push('| Event | Count | Tokens Affected | Tokens Saved |');
    parts.push('|-------|-------|-----------------|--------------|');
    let tc = 0, ta = 0, ts = 0;
    for (const evt of Object.keys(hookEvents.byEvent).sort()) {
      const e = hookEvents.byEvent[evt];
      const aff = e.tokensAffected > 0 ? e.tokensAffected : '-';
      const sav = e.tokensSaved > 0 ? e.tokensSaved : '-';
      parts.push(`| ${evt} | ${e.count} | ${aff} | ${sav} |`);
      tc += e.count;
      ta += e.tokensAffected;
      ts += e.tokensSaved;
    }
    parts.push('|-------|-------|-----------------|--------------|');
    parts.push(`| **TOTAL** | ${tc} | ${ta || '-'} | ${ts || '-'} |`);
    parts.push('');
  }

  // ── RTK Token Economy ────────────────────────────────────────────────
  if (rtk && rtk.saved > 0) {
    parts.push('## RTK Token Economy');
    parts.push(`- Total saved: ${Math.round(rtk.saved / 1000)}k tokens`);
    parts.push(`- Savings rate: ${Math.round(rtk.pct)}%`);
    if (rtk.commands > 0) parts.push(`- Commands rewritten: ${rtk.commands}`);
    parts.push('');
  }

  if (parts.length <= 2) {
    parts.push('No metrics data found. Run a pipeline first.');
  }

  console.log(parts.join('\n'));
  process.exit(0);
}

// ── Data collection ────────────────────────────────────────────────────

function collectSpecs(claudeDir) {
  const statesDir = path.join(claudeDir, '.pipeline-states');
  const activeSpecDir = path.join(claudeDir, 'spec', 'active');
  const out = { active: [], orphaned: [] };
  if (!fs.existsSync(statesDir)) return out;

  // Wave 4: metrics come from harness log via buildPipelineState (no .metrics.json sidecar).
  // Fall back to main state .metrics field if harness-views is unavailable.
  let harnessViews = null;
  try { harnessViews = require('./harness-views.js'); } catch (_) {}

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

    // Derive metrics from harness log, falling back to inline metrics field
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

function aggregateHookEvents(metricsDir) {
  const result = { byEvent: {}, byDay: {}, total: 0 };
  if (!fs.existsSync(metricsDir)) return result;
  const files = fs.readdirSync(metricsDir).filter(f => f.endsWith('.jsonl'));
  for (const file of files) {
    let content;
    try { content = fs.readFileSync(path.join(metricsDir, file), 'utf8'); }
    catch { continue; }
    for (const raw of content.split('\n')) {
      const line = raw.trim();
      if (!line) continue;
      let entry;
      try { entry = JSON.parse(line); } catch { continue; }
      if (!entry.event) continue;
      const k = entry.event;
      if (!result.byEvent[k]) result.byEvent[k] = { count: 0, tokensAffected: 0, tokensSaved: 0 };
      result.byEvent[k].count++;
      result.total++;
      if (typeof entry.tokens_affected === 'number') result.byEvent[k].tokensAffected += entry.tokens_affected;
      // PR1: rtk-rewrite tokens_saved is heuristic; real numbers come from rtk-gain.
      if (typeof entry.tokens_saved === 'number' && entry.event !== 'rtk-rewrite') {
        result.byEvent[k].tokensSaved += entry.tokens_saved;
      }
      if (entry.ts) {
        const day = String(entry.ts).slice(0, 10);
        result.byDay[day] = (result.byDay[day] || 0) + 1;
      }
    }
  }
  return result;
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
  // Current week vs prior week (14-day window split in half, ending today).
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

// ── Summary ────────────────────────────────────────────────────────────

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

  // Pass@1: percentage of tracked specs with zero hook retries.
  if (totalSpecs > 0) {
    let pass = 0;
    for (const group of [specs.active, specs.orphaned]) {
      for (const s of group) if ((s.metrics.retries || 0) === 0) pass++;
    }
    const pct = Math.round((pass / totalSpecs) * 100);
    const prefix = pct >= 80 ? '✓' : pct >= 50 ? '→' : '⚠';
    lines.push(`${prefix} Pass@1 (hook-level): ${pct}% (${pass}/${totalSpecs} without hook retries)`);
  }

  if (rtk && rtk.saved > 0) {
    lines.push(`✓ RTK savings: ~${Math.round(rtk.saved / 1000)}k tokens (${Math.round(rtk.pct)}%)`);
  }

  // Top alert: spec with highest retry count on any single phase ≥ 3.
  const alert = findTopAlert([...specs.active, ...specs.orphaned]);
  if (alert) lines.push(`⚠ ${alert}`);

  return lines;
}

function findTopAlert(allSpecs) {
  let worst = null;
  for (const s of allSpecs) {
    const attempts = s.metrics.agentAttempts || {};
    for (const [phase, n] of Object.entries(attempts)) {
      if (n >= 3 && (!worst || n > worst.n)) worst = { name: s.name, phase, n };
    }
  }
  if (!worst) return null;
  return `1 pipeline with ${worst.n} retries in ${worst.phase} (${worst.name})`;
}

// ── Rendering ──────────────────────────────────────────────────────────

function renderSpecs(parts, list, label) {
  // Sort for stable output: newest date prefix first (names begin with YYYY-MM-DD).
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

    if (m.agentAttempts && Object.keys(m.agentAttempts).length > 0) {
      const entries = Object.entries(m.agentAttempts).map(([phase, n]) => {
        const mark = n >= 3 ? ' ⚠' : '';
        return `${phase}:${n}${mark}`;
      });
      parts.push(`- Retries by phase: ${entries.join(', ')}`);
    }

    if (m.gate_saves !== undefined) parts.push(`- Gate saves: ${m.gate_saves}`);
    if (m.wave_reentry !== undefined) parts.push(`- Wave reentries: ${m.wave_reentry}`);

    if (m.skillHits && Object.keys(m.skillHits).length > 0) {
      parts.push('- Skill hits:');
      for (const [agent, hits] of Object.entries(m.skillHits).sort()) {
        const pct = hits.loaded > 0 ? Math.round((hits.read / hits.loaded) * 100) + '%' : '—';
        parts.push(`  - ${agent}: ${hits.read}/${hits.loaded} (${pct})`);
      }
    }

    // Pass@1 per agent (heuristic): cross subagent-registry with agentAttempts.
    const pass1 = agentPass1(s);
    if (pass1 && pass1.length > 0) {
      parts.push('- Pass@1 by agent (heuristic):');
      for (const row of pass1) parts.push(`  - ${row}`);
    }

    if (s.isOrphaned) {
      parts.push('- Spec: not in spec/active/ (likely completed without /mustard:complete)');
    }
    parts.push('');
  }
}

function agentPass1(spec) {
  const registryPath = path.join(process.cwd(), '.claude', '.subagent-registry.json');
  const registry = readJson(registryPath);
  if (!registry) return null;
  const attempts = spec.metrics.agentAttempts || {};
  const anyRetry = Object.values(attempts).some(n => n > 0);
  const agents = new Set();
  for (const entry of Object.values(registry)) {
    if (entry && entry.agentType) agents.add(entry.agentType);
  }
  if (agents.size === 0) return null;
  const rows = [];
  for (const agent of [...agents].sort()) {
    rows.push(`${agent}: ${anyRetry ? 'advisory (retries present)' : '100%'}`);
  }
  return rows.slice(0, 5);
}

function renderArchives(parts, archives) {
  if (archives.length === 0) return;
  parts.push('## Completed Pipelines');
  parts.push('');

  let totalCalls = 0;
  let totalRetries = 0;
  let totalDurationMs = 0;
  let count = 0;
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

  // Pass@1 across all archives.
  let pass1Count = 0;
  let retrySum = 0;
  for (const a of archives) {
    if ((a.metrics.retries || 0) === 0) pass1Count++;
    retrySum += a.metrics.retries || 0;
  }
  const pct = Math.round((pass1Count / archives.length) * 100);
  const avg = (retrySum / archives.length).toFixed(1);
  parts.push('## Pass@1 Metrics (archived)');
  parts.push(`- Pass@1 (hook-level): ${pct}% (${pass1Count}/${archives.length} completed with zero hook retries)`);
  parts.push(`- Avg hook retries per pipeline: ${avg}`);
  parts.push('');
}

// ── Small helpers ──────────────────────────────────────────────────────

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
  const s = d > 0 ? '+' : '';
  return `${s}${d.toFixed(1)}%`;
}

function cell(ref, cur) {
  return `${ref}→${cur} (${pct(ref, cur)})`;
}

function formatDuration(start, end) {
  return formatMs(end.getTime() - start.getTime());
}

function formatMs(ms) {
  if (ms < 60000) return `${Math.round(ms / 1000)}s`;
  const m = Math.floor(ms / 60000);
  const s = Math.round((ms % 60000) / 1000);
  if (m < 60) return `${m}m${s}s`;
  const h = Math.floor(m / 60);
  return `${h}h${m % 60}m`;
}

main();
