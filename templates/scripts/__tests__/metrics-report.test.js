'use strict';
// Tests for templates/scripts/metrics-report.js --compare mode.
// Uses node:test + node:assert (Node built-ins only).
//
// Strategy:
// - Copy fixture .jsonl into a fresh tmp dir per test.
// - Point the script at that dir via MUSTARD_METRICS_DIR env var.
// - Assert on stdout/stderr/exit via spawnSync.

const { test } = require('bun:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const SCRIPT = path.resolve(__dirname, '..', 'metrics.js');
const FIXTURE = path.resolve(__dirname, 'fixtures', 'metrics-sample.jsonl');

function mkTmpMetricsDir(opts = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-metrics-'));
  if (opts.empty) return dir;
  const dest = path.join(dir, 'sample.jsonl');
  fs.copyFileSync(FIXTURE, dest);
  if (opts.extraLines && opts.extraLines.length) {
    fs.appendFileSync(dest, opts.extraLines.map(l => JSON.stringify(l)).join('\n') + '\n');
  }
  return dir;
}

function run(args, metricsDir) {
  return spawnSync(process.execPath, [SCRIPT, 'report', ...args], {
    encoding: 'utf8',
    env: { ...process.env, MUSTARD_METRICS_DIR: metricsDir },
  });
}

test('--compare with ISO dates produces a Compare report with delta columns', () => {
  const dir = mkTmpMetricsDir();
  const res = run(['--compare', '2026-04-09', '2026-04-20'], dir);
  assert.equal(res.status, 0, `stderr: ${res.stderr}`);
  assert.match(res.stdout, /## Compare/);
  assert.match(res.stdout, /Reference window:/);
  assert.match(res.stdout, /New window:/);
  // Header of delta table
  assert.match(res.stdout, /Count \(ref→new, Δ%\)/);
  // rtk-rewrite present in both windows -> should appear
  assert.match(res.stdout, /rtk-rewrite/);
  // TOTAL row
  assert.match(res.stdout, /\*\*TOTAL\*\*/);
});

test('--compare delta values are arithmetically correct for a known event', () => {
  // Fixture design:
  //  Pre-2026-04-09 window (ref): rtk-rewrite events on 04-01, 04-02, 04-03, 04-08  -> count=4
  //  2026-04-09 → 2026-04-20 (new): rtk-rewrite events on 04-10, 04-11, 04-12, 04-19 -> count=4
  // Ref window mirrors the 11-day new window starting from 2026-03-29 → 2026-04-09,
  // which captures all 4 pre-events above.
  const dir = mkTmpMetricsDir();
  const res = run(['--compare', '2026-04-09', '2026-04-20'], dir);
  assert.equal(res.status, 0, res.stderr);
  // Expect a row like "| rtk-rewrite | 4→4 (0.0%) | ..."
  const m = res.stdout.match(/\|\s*rtk-rewrite\s*\|\s*(\d+)→(\d+)\s*\(/);
  assert.ok(m, `rtk-rewrite row not found in:\n${res.stdout}`);
  assert.equal(m[1], '4', 'ref count should be 4');
  assert.equal(m[2], '4', 'new count should be 4');
});

test('sparse reference window emits stderr warning but still prints report and exits 0', () => {
  // Narrow new window 2026-04-18 → 2026-04-20 (duration = 2 days).
  // Ref window = 2026-04-16 → 2026-04-18, fixture has 2 events there (bash-native-redirect
  // on 04-16 and 04-17) — below the 5-event threshold.
  const dir = mkTmpMetricsDir();
  const res = run(['--compare', '2026-04-18', '2026-04-20'], dir);
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stderr, /Warning:.*reference window.*<5/);
  assert.match(res.stdout, /## Compare/);
  assert.match(res.stdout, /reference history sparse/);
});

test('both windows empty -> "No metrics data in the given windows" + exit 0', () => {
  const dir = mkTmpMetricsDir();
  // Pick a window far in the future where fixture has nothing, and ref window
  // is also outside fixture range.
  const res = run(['--compare', '2030-01-01', '2030-02-01'], dir);
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /No metrics data in the given windows/);
});

test('--compare with invalid endpoint exits 1 with clear error', () => {
  const dir = mkTmpMetricsDir();
  const res = run(['--compare', 'not-a-date', '2026-04-20'], dir);
  assert.equal(res.status, 1);
  assert.match(res.stderr, /not a valid git tag.*or ISO date/);
});

test('--compare with from >= to exits 1 with clear error', () => {
  const dir = mkTmpMetricsDir();
  const res = run(['--compare', '2026-04-20', '2026-04-10'], dir);
  assert.equal(res.status, 1);
  assert.match(res.stderr, /must be earlier than/);
});

test('--compare missing second arg exits 1', () => {
  const dir = mkTmpMetricsDir();
  const res = run(['--compare', '2026-04-09'], dir);
  assert.equal(res.status, 1);
  assert.match(res.stderr, /--compare requires two arguments/);
});

test('--compare honors --event filter (restricts both windows)', () => {
  const dir = mkTmpMetricsDir();
  const res = run(['--compare', '2026-04-09', '2026-04-20', '--event', 'rtk-rewrite'], dir);
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /rtk-rewrite/);
  // budget-check should NOT appear because of event filter
  assert.doesNotMatch(res.stdout, /\|\s*budget-check\s*\|/);
});

test('regression: --since alone (no --compare) still prints the classic table', () => {
  const dir = mkTmpMetricsDir();
  const res = run(['--since', '2026-04-10'], dir);
  assert.equal(res.status, 0, res.stderr);
  // Classic header, no Compare section
  assert.match(res.stdout, /\| Event \| Count \| Tokens Affected \| Tokens Saved \| Notes \|/);
  assert.doesNotMatch(res.stdout, /## Compare/);
  // Should include post-04-10 events
  assert.match(res.stdout, /rtk-rewrite/);
});

test('regression: no args prints classic aggregate over full fixture', () => {
  const dir = mkTmpMetricsDir();
  const res = run([], dir);
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /\| Event \| Count \| Tokens Affected \| Tokens Saved \| Notes \|/);
  assert.match(res.stdout, /\*\*TOTAL\*\*/);
  assert.doesNotMatch(res.stdout, /## Compare/);
});

test('regression: empty metrics dir prints "No metrics data yet"', () => {
  const dir = mkTmpMetricsDir({ empty: true });
  const res = run([], dir);
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /No metrics data yet/);
});
