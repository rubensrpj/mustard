#!/usr/bin/env bun
'use strict';
// <!-- mustard:generated -->
/**
 * DIAGNOSE-OTEL: End-to-end health check for the Mustard ↔ Claude Code OTEL pipeline.
 *
 * Sections (printed unless --json):
 *   env          — required CLAUDE_CODE_ENABLE_TELEMETRY / OTEL_* / MUSTARD_HARNESS_DUAL_EMIT
 *   collector    — PID file present + process alive
 *   health       — /healthz returns 200
 *   data         — claude_code_otel row count + max(ts_bucket) + sample
 *   subtractions — events.mustard.subtraction.applied count in last 24h
 *
 * Flags:
 *   --json                     Machine-readable output
 *   --expect-rows-after Xs     Wait X seconds, then assert row count grew. Exit 1 on fail.
 *                              Used by AC-8 to validate a real Claude Code session emitted.
 *
 * Exit codes:
 *   0  All checks reported (or --expect-rows-after passed).
 *   1  --expect-rows-after failed.
 *
 * Fail-open: missing OTEL config or dead collector do NOT exit non-zero (it's a
 * diagnose tool, not a gate). Only --expect-rows-after can fail.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

const PROJECT_DIR = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const CLAUDE_DIR = path.join(PROJECT_DIR, '.claude');
const HARNESS_DIR = path.join(CLAUDE_DIR, '.harness');
const PID_FILE = path.join(HARNESS_DIR, '.otel-collector.pid');
const PORT = parseInt(process.env.MUSTARD_OTEL_PORT || '4318', 10);

// ── Args ────────────────────────────────────────────────────────────────────

function parseArgs(argv) {
  const opts = { json: false, expectRowsAfterMs: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--json') opts.json = true;
    else if (a === '--expect-rows-after') {
      const v = argv[++i] || '';
      const m = String(v).match(/^(\d+)\s*(s|ms)?$/);
      if (m) {
        const n = parseInt(m[1], 10);
        opts.expectRowsAfterMs = m[2] === 'ms' ? n : n * 1000;
      }
    }
  }
  return opts;
}

// ── Checks ──────────────────────────────────────────────────────────────────

function checkEnv() {
  const required = [
    'CLAUDE_CODE_ENABLE_TELEMETRY',
    'OTEL_METRICS_EXPORTER',
    'OTEL_EXPORTER_OTLP_ENDPOINT',
    'MUSTARD_HARNESS_DUAL_EMIT',
  ];
  const status = {};
  for (const k of required) {
    status[k] = process.env[k] || null;
  }
  const ok = required.every((k) => !!status[k]);
  return { ok, status };
}

function checkCollector() {
  if (!fs.existsSync(PID_FILE)) {
    return { ok: false, reason: 'no pid file', pid: null };
  }
  let pid;
  try {
    pid = parseInt(fs.readFileSync(PID_FILE, 'utf8').trim(), 10);
  } catch (err) {
    return { ok: false, reason: 'pid read failed: ' + err.message, pid: null };
  }
  if (!Number.isFinite(pid) || pid <= 0) {
    return { ok: false, reason: 'invalid pid', pid };
  }
  try {
    process.kill(pid, 0); // signal 0 = liveness probe
    return { ok: true, pid };
  } catch (err) {
    return { ok: false, reason: 'process dead: ' + err.code, pid };
  }
}

async function checkHealth() {
  try {
    const res = await fetch(`http://127.0.0.1:${PORT}/healthz`);
    return { ok: res.status === 200, status: res.status };
  } catch (err) {
    return { ok: false, status: null, reason: err.message };
  }
}

function openStore() {
  try {
    const { getStore } = require(path.join(CLAUDE_DIR, 'hooks', '_lib', 'event-store.js'));
    return getStore(CLAUDE_DIR);
  } catch (_) {
    return null;
  }
}

function checkData(store) {
  if (!store || !store.db) {
    return { ok: false, reason: 'event-store unavailable', rows: 0, sample: [] };
  }
  try {
    const count = store.db.prepare('SELECT COUNT(*) AS n FROM claude_code_otel').get();
    const maxRow = store.db.prepare('SELECT MAX(ts_bucket) AS m FROM claude_code_otel').get();
    const sample = store.db.prepare(
      'SELECT ts_bucket, metric, session_id, model, token_type, sum, count FROM claude_code_otel ORDER BY ts_bucket DESC LIMIT 5'
    ).all();
    const maxIso = maxRow && maxRow.m ? new Date(maxRow.m).toISOString() : null;
    return { ok: true, rows: count.n, lastBucketIso: maxIso, sample };
  } catch (err) {
    return { ok: false, reason: err.message, rows: 0, sample: [] };
  }
}

function checkSubtractions(store) {
  if (!store || !store.db) {
    return { ok: false, reason: 'event-store unavailable', count: 0 };
  }
  try {
    const since = new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString();
    const row = store.db.prepare(
      "SELECT COUNT(*) AS n FROM events WHERE event='mustard.subtraction.applied' AND ts > ?"
    ).get(since);
    return { ok: true, count: row.n };
  } catch (err) {
    return { ok: false, reason: err.message, count: 0 };
  }
}

function getRowCount(store) {
  if (!store || !store.db) return null;
  try {
    const row = store.db.prepare('SELECT COUNT(*) AS n FROM claude_code_otel').get();
    return row.n;
  } catch (_) {
    return null;
  }
}

// ── Output ──────────────────────────────────────────────────────────────────

function renderHuman(report) {
  const lines = [];
  lines.push('=== Mustard OTEL Diagnose ===');
  lines.push('');
  lines.push('[env]');
  for (const [k, v] of Object.entries(report.env.status)) {
    lines.push(`  ${k} = ${v === null ? '(unset)' : v}`);
  }
  lines.push(`  status: ${report.env.ok ? 'OK' : 'INCOMPLETE'}`);
  lines.push('');
  lines.push('[collector]');
  lines.push(`  pid: ${report.collector.pid ?? '(none)'}`);
  lines.push(`  alive: ${report.collector.ok}`);
  if (!report.collector.ok) lines.push(`  reason: ${report.collector.reason}`);
  lines.push('');
  lines.push('[health]');
  lines.push(`  status: ${report.health.status ?? '(unreachable)'}`);
  lines.push(`  ok: ${report.health.ok}`);
  if (!report.health.ok && report.health.reason) lines.push(`  reason: ${report.health.reason}`);
  lines.push('');
  lines.push('[data]');
  if (!report.data.ok) {
    lines.push(`  reason: ${report.data.reason}`);
  } else {
    lines.push(`  rows: ${report.data.rows}`);
    lines.push(`  last bucket: ${report.data.lastBucketIso ?? '(none)'}`);
    if (report.data.sample.length > 0) {
      lines.push('  sample (latest 5):');
      for (const r of report.data.sample) {
        lines.push(`    - ${new Date(r.ts_bucket).toISOString()} ${r.metric} session=${r.session_id ?? '-'} model=${r.model ?? '-'} type=${r.token_type ?? '-'} sum=${r.sum} count=${r.count}`);
      }
    }
  }
  lines.push('');
  lines.push('[subtractions]');
  if (!report.subtractions.ok) {
    lines.push(`  reason: ${report.subtractions.reason}`);
  } else {
    lines.push(`  applied (last 24h): ${report.subtractions.count}`);
  }
  return lines.join('\n');
}

// ── Main ────────────────────────────────────────────────────────────────────

async function run() {
  const opts = parseArgs(process.argv.slice(2));
  const store = openStore();

  // Snapshot row count BEFORE wait (for --expect-rows-after).
  const rowsBefore = getRowCount(store);

  if (opts.expectRowsAfterMs && opts.expectRowsAfterMs > 0) {
    await new Promise((r) => setTimeout(r, opts.expectRowsAfterMs));
  }

  const env = checkEnv();
  const collector = checkCollector();
  const health = await checkHealth();
  const data = checkData(store);
  const subtractions = checkSubtractions(store);
  const report = { env, collector, health, data, subtractions };

  // --expect-rows-after assertion
  if (opts.expectRowsAfterMs && opts.expectRowsAfterMs > 0) {
    const rowsAfter = data.ok ? data.rows : null;
    const passed = rowsBefore !== null && rowsAfter !== null && rowsAfter > rowsBefore;
    report.expectRowsAfter = {
      waitMs: opts.expectRowsAfterMs,
      before: rowsBefore,
      after: rowsAfter,
      passed,
    };
    if (opts.json) {
      process.stdout.write(JSON.stringify(report, null, 2) + '\n');
    } else {
      process.stdout.write(renderHuman(report) + '\n');
      process.stdout.write('\n[expect-rows-after]\n');
      process.stdout.write(`  before: ${rowsBefore}\n`);
      process.stdout.write(`  after:  ${rowsAfter}\n`);
      process.stdout.write(`  passed: ${passed}\n`);
    }
    process.exit(passed ? 0 : 1);
  }

  if (opts.json) {
    process.stdout.write(JSON.stringify(report, null, 2) + '\n');
  } else {
    process.stdout.write(renderHuman(report) + '\n');
  }
  process.exit(0);
}

run().catch((err) => {
  // Fail-open: diagnose should never crash. Print and exit 0.
  try { process.stderr.write('[diagnose-otel] unexpected: ' + err.message + '\n'); } catch (_) {}
  process.exit(0);
});
