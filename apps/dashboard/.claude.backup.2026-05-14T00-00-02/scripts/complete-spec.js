#!/usr/bin/env bun
'use strict';
/**
 * complete-spec — finalize a pipeline spec in two stages.
 *
 * Stage 1 (default): mark `closed-followup`
 *   - Snapshot affectedFiles from harness events or `git diff --name-only`
 *   - Update pipeline-state.json: { status: 'closed-followup', closedAt, affectedFiles }
 *   - Leave spec under spec/active/ so metrics-tracker can still link follow-up edits
 *     whose filepath matches affectedFiles.
 *
 * Stage 2 (--archive): finalize archival
 *   - Move spec/active/<name> → spec/completed/<name>
 *   - Write archived metrics to .claude/metrics/<name>.json (if metrics exist)
 *   - Delete .pipeline-states/<name>.json
 *
 * Cancellation triggers:
 *   - Called with --archive by another command (e.g. /mustard:feature about to open a new spec).
 *   - session-cleanup hook auto-archives closed-followup specs older than 24h.
 *
 * Usage:
 *   node complete-spec.js <spec-name>            # mark closed-followup
 *   node complete-spec.js <spec-name> --archive  # finalize archival
 *   node complete-spec.js --archive-stale        # archive every closed-followup > 24h
 *
 * Exits non-zero only for argument errors. All I/O is fail-soft.
 */

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const FOLLOWUP_TTL_MS = 24 * 60 * 60 * 1000;

function readJson(p) {
  try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch (_) { return null; }
}

function writeJson(p, obj) {
  try {
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, JSON.stringify(obj, null, 2) + '\n', 'utf8');
    return true;
  } catch (_) { return false; }
}

function tryExec(cmd, args, cwd) {
  try {
    return execFileSync(cmd, args, {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: 5000,
      windowsHide: true,
    }).trim();
  } catch (_) { return ''; }
}

function readMustardJson(cwd) {
  return readJson(path.join(cwd, 'mustard.json')) || {};
}

function parentBranchFor(cwd, currentBranch) {
  const m = readMustardJson(cwd);
  const flow = m && m.gitFlow;
  if (flow && flow.parentOf && flow.parentOf[currentBranch]) return flow.parentOf[currentBranch];
  if (flow && flow.mainBranch) return flow.mainBranch;
  return 'main';
}

function currentBranch(cwd) {
  return tryExec('git', ['rev-parse', '--abbrev-ref', 'HEAD'], cwd) || '';
}

function collectAffectedFiles(cwd, specName, state) {
  const files = new Set();

  // 1. From harness events: read tool.use events tagged with this spec, collect target.file
  try {
    const eventsPath = path.join(cwd, '.claude', '.harness', 'events.jsonl');
    if (fs.existsSync(eventsPath)) {
      const content = fs.readFileSync(eventsPath, 'utf8');
      const lines = content.split(/\r?\n/);
      for (const line of lines) {
        if (!line) continue;
        try {
          const ev = JSON.parse(line);
          if (ev.spec !== specName) continue;
          const f = ev.payload && ev.payload.target && ev.payload.target.file;
          if (typeof f === 'string' && f.length > 0) files.add(f);
        } catch (_) {}
      }
    }
  } catch (_) {}

  // 2. From git diff against parent branch
  try {
    const branch = currentBranch(cwd);
    const parent = parentBranchFor(cwd, branch);
    if (branch && parent && branch !== parent) {
      const diff = tryExec('git', ['diff', '--name-only', `${parent}...HEAD`], cwd);
      if (diff) {
        for (const f of diff.split(/\r?\n/)) {
          const trimmed = f.trim();
          if (trimmed) files.add(trimmed);
        }
      }
    }
  } catch (_) {}

  // 3. From state.metrics.toolBreakdown if it tracks files
  try {
    const tb = state && state.metrics && state.metrics.toolBreakdown;
    if (tb && typeof tb === 'object') {
      for (const k of Object.keys(tb)) {
        if (k.includes('/') || k.includes('\\')) files.add(k);
      }
    }
  } catch (_) {}

  return Array.from(files);
}

function activeSpecDir(cwd, specName) {
  return path.join(cwd, '.claude', 'spec', 'active', specName);
}

function completedSpecDir(cwd, specName) {
  return path.join(cwd, '.claude', 'spec', 'completed', specName);
}

function pipelineStatePath(cwd, specName) {
  return path.join(cwd, '.claude', '.pipeline-states', `${specName}.json`);
}

function moveDir(src, dst) {
  try {
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.renameSync(src, dst);
    return true;
  } catch (e) {
    // Cross-device fallback: copy + remove
    try {
      copyRecursive(src, dst);
      removeRecursive(src);
      return true;
    } catch (_) { return false; }
  }
}

function copyRecursive(src, dst) {
  const stat = fs.statSync(src);
  if (stat.isDirectory()) {
    fs.mkdirSync(dst, { recursive: true });
    for (const entry of fs.readdirSync(src)) {
      copyRecursive(path.join(src, entry), path.join(dst, entry));
    }
  } else {
    fs.copyFileSync(src, dst);
  }
}

function removeRecursive(p) {
  try {
    if (fs.rmSync) fs.rmSync(p, { recursive: true, force: true });
    else fs.rmdirSync(p, { recursive: true });
  } catch (_) {}
}

function markFollowup(cwd, specName) {
  const statePath = pipelineStatePath(cwd, specName);
  const state = readJson(statePath) || { specName };
  const affected = collectAffectedFiles(cwd, specName, state);
  const now = new Date().toISOString();
  state.status = 'closed-followup';
  state.closedAt = now;
  state.affectedFiles = affected;
  state.specName = state.specName || specName;
  const ok = writeJson(statePath, state);
  return { ok, affectedCount: affected.length, statePath };
}

// Wave 4 moved metrics from `state.metrics` sidecar to events.jsonl. We derive
// via harness-views.buildPipelineState first, then fall back to legacy
// state.metrics for pipelines that ran before Wave 4 and still have the field.
function deriveMetricsFromEvents(cwd, specName) {
  try {
    const eventsPath = path.join(cwd, '.claude', '.harness', 'events.jsonl');
    if (!fs.existsSync(eventsPath)) return null;
    const views = require('./harness-views.js');
    const events = views.readEventsSync
      ? views.readEventsSync(eventsPath)
      : fs.readFileSync(eventsPath, 'utf8').trim().split('\n').filter(Boolean).map(l => { try { return JSON.parse(l); } catch (_) { return null; } }).filter(Boolean);
    const r = views.buildPipelineState(events, { spec: specName });
    if (!r || !r.metrics || !r.metrics.apiCalls) return null;
    return r.metrics;
  } catch (_) { return null; }
}

function archiveMetricsFromState(cwd, specName, state) {
  try {
    const derived = deriveMetricsFromEvents(cwd, specName);
    const m = derived || (state && state.metrics);
    if (!m) return false;
    const metricsDir = path.join(cwd, '.claude', 'metrics');
    fs.mkdirSync(metricsDir, { recursive: true });
    const out = {
      name: specName,
      completedAt: (state && state.completedAt) || new Date().toISOString(),
      durationMs: m.startedAt && m.updatedAt
        ? Math.max(0, new Date(m.updatedAt).getTime() - new Date(m.startedAt).getTime())
        : null,
      apiCalls: m.apiCalls || 0,
      retries: m.retries || 0,
      pass1: (m.retries || 0) === 0,
      toolBreakdown: m.toolBreakdown || {},
      agentCount: m.agentCount || undefined,
      dispatchFailuresByPhase: m.dispatchFailuresByPhase || undefined,
      source: derived ? 'harness-events' : 'legacy-state',
    };
    fs.writeFileSync(path.join(metricsDir, `${specName}.json`), JSON.stringify(out, null, 2) + '\n', 'utf8');
    return true;
  } catch (_) { return false; }
}

function archive(cwd, specName) {
  const activeDir = activeSpecDir(cwd, specName);
  const completedDir = completedSpecDir(cwd, specName);
  const statePath = pipelineStatePath(cwd, specName);
  const state = readJson(statePath);
  let movedSpec = false;
  if (fs.existsSync(activeDir) && !fs.existsSync(completedDir)) {
    movedSpec = moveDir(activeDir, completedDir);
  } else if (!fs.existsSync(activeDir) && fs.existsSync(completedDir)) {
    movedSpec = true;
  }
  archiveMetricsFromState(cwd, specName, state || {});
  try { if (fs.existsSync(statePath)) fs.unlinkSync(statePath); } catch (_) {}
  return { movedSpec, hadState: !!state };
}

function archiveFollowups(cwd, opts) {
  const requireTTL = !!(opts && opts.requireTTL);
  const statesDir = path.join(cwd, '.claude', '.pipeline-states');
  if (!fs.existsSync(statesDir)) return { scanned: 0, archived: 0 };
  let scanned = 0, archived = 0;
  for (const f of fs.readdirSync(statesDir)) {
    if (!f.endsWith('.json') || f.endsWith('.metrics.json')) continue;
    scanned++;
    const state = readJson(path.join(statesDir, f));
    if (!state || state.status !== 'closed-followup') continue;
    if (requireTTL) {
      const closedAt = Date.parse(state.closedAt || '');
      if (!Number.isFinite(closedAt)) continue;
      if (Date.now() - closedAt < FOLLOWUP_TTL_MS) continue;
    }
    const name = state.specName || f.replace(/\.json$/, '');
    const r = archive(cwd, name);
    if (r.movedSpec || r.hadState) archived++;
  }
  return { scanned, archived };
}

function main() {
  const args = process.argv.slice(2);
  const cwd = process.cwd();
  const archiveFlag = args.includes('--archive');
  const archiveStaleFlag = args.includes('--archive-stale');
  const archiveFollowupsFlag = args.includes('--archive-followups');
  const specName = args.find((a, i) => !a.startsWith('--') && i === 0);

  if (archiveStaleFlag) {
    const r = archiveFollowups(cwd, { requireTTL: true });
    console.log(JSON.stringify({ ok: true, mode: 'archive-stale', ...r }));
    return;
  }

  if (archiveFollowupsFlag) {
    const r = archiveFollowups(cwd, { requireTTL: false });
    console.log(JSON.stringify({ ok: true, mode: 'archive-followups', ...r }));
    return;
  }

  if (!specName) {
    console.error('usage: complete-spec.js <spec-name> [--archive] | --archive-stale | --archive-followups');
    process.exit(2);
  }

  if (archiveFlag) {
    const r = archive(cwd, specName);
    console.log(JSON.stringify({ ok: true, mode: 'archive', spec: specName, ...r }));
    return;
  }

  const r = markFollowup(cwd, specName);
  console.log(JSON.stringify({ ok: r.ok, mode: 'followup', spec: specName, affectedFiles: r.affectedCount, statePath: r.statePath }));
}

main();
