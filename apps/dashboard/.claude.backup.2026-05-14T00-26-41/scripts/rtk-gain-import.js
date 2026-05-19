#!/usr/bin/env bun
'use strict';
/**
 * rtk-gain-import — snapshot real RTK token-savings telemetry.
 *
 * Runs `rtk gain --format json -p` and writes the parsed result as ONE
 * `rtk-gain` event per invocation to `.claude/.metrics/rtk-gain.jsonl`.
 * The file is overwritten (truncate + write) so it always reflects the
 * current state of RTK's cumulative counters — idempotent snapshot.
 *
 * Why not append? `rtk gain` already returns cumulative totals; appending
 * would double-count on re-run. Snapshot-replace keeps the event honest.
 *
 * Fail-open: if `rtk` is missing, output is non-JSON, or anything throws,
 * this script exits 0 without writing anything (never crashes callers).
 *
 * Event schema (written line):
 *   { ts, event: 'rtk-gain', tokens_affected, tokens_saved, note, extras: {...} }
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const CWD = process.cwd();
const METRICS_DIR = path.join(CWD, '.claude', '.metrics');
const OUT_FILE = path.join(METRICS_DIR, 'rtk-gain.jsonl');

function rtkAvailable() {
  try {
    if (process.platform === 'win32') {
      execFileSync('where', ['rtk'], { stdio: 'ignore' });
    } else {
      execFileSync('which', ['rtk'], { stdio: 'ignore' });
    }
    return true;
  } catch (_) {
    return false;
  }
}

function fetchGainJson() {
  try {
    const raw = execFileSync('rtk', ['gain', '--format', 'json', '-p'], {
      encoding: 'utf8',
      timeout: 5000,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    const trimmed = (raw || '').trim();
    if (!trimmed) return null;
    return JSON.parse(trimmed);
  } catch (_) {
    return null;
  }
}

function main() {
  if (!rtkAvailable()) {
    process.stdout.write('rtk not available; skipping\n');
    return 0;
  }

  const data = fetchGainJson();
  if (!data || typeof data !== 'object') {
    process.stdout.write('rtk gain produced no parseable JSON; skipping\n');
    return 0;
  }

  // rtk gain --format json -p returns: { summary: { total_commands,
  //   total_input, total_output, total_saved, avg_savings_pct,
  //   total_time_ms, avg_time_ms } }
  const summary = (data && data.summary && typeof data.summary === 'object')
    ? data.summary
    : data; // some rtk versions may inline fields at the top level

  const totalSaved = Number(summary.total_saved) || 0;
  const totalInput = Number(summary.total_input) || 0;
  const totalOutput = Number(summary.total_output) || 0;
  const totalCommands = Number(summary.total_commands) || 0;
  const avgSavingsPct = Number(summary.avg_savings_pct) || 0;

  const line = {
    ts: new Date().toISOString(),
    event: 'rtk-gain',
    tokens_affected: totalInput,
    tokens_saved: totalSaved,
    note: 'imported from rtk gain',
    period: '-p (project scope)',
    commands_count: totalCommands,
    total_output_tokens: totalOutput,
    avg_savings_pct: avgSavingsPct,
  };

  let serialized;
  try {
    serialized = JSON.stringify(line);
  } catch (_) {
    process.stdout.write('rtk-gain snapshot skipped: could not serialize\n');
    return 0;
  }

  try {
    fs.mkdirSync(METRICS_DIR, { recursive: true });
    // Snapshot-replace (not append): rtk gain returns cumulative totals,
    // so writing one event per invocation keeps this file authoritative.
    fs.writeFileSync(OUT_FILE, serialized + '\n', 'utf8');
  } catch (err) {
    process.stdout.write(`rtk-gain snapshot failed to write: ${err.message}\n`);
    return 0;
  }

  process.stdout.write(
    `rtk-gain snapshot written: 1 events, total_saved=${totalSaved} tokens\n`
  );
  return 0;
}

try {
  process.exit(main());
} catch (err) {
  // Absolute fail-open: never surface a non-zero exit.
  process.stderr.write(`[rtk-gain-import] ${err && err.message}\n`);
  process.exit(0);
}
