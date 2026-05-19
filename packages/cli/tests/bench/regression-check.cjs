#!/usr/bin/env bun
'use strict';

/**
 * Bench regression check (Phase 4 Wave 2, AC #4).
 *
 * Reads `.results/<bench>.json` written by each bench script, compares the
 * observed p95 against `baselines.json` with a 15% tolerance, exits non-zero
 * if any metric regresses.
 *
 * Run: node tests/bench/regression-check.cjs
 *
 * Skip a noisy bench (e.g. while we tune the floor on slow runners):
 *   MUSTARD_BENCH_SKIP=hook-cold-start node tests/bench/regression-check.cjs
 */

const fs = require('node:fs');
const path = require('node:path');

const baselines = require('./baselines.json');
const REGRESSION_TOLERANCE = 1.15; // 15%

const skip = new Set(
  (process.env.MUSTARD_BENCH_SKIP || '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
);

const targets = [
  { file: 'fts5-query', baselineKey: 'fts5_query_p95_ms', label: 'fts5_query_p95' },
  { file: 'mcp-roundtrip', baselineKey: 'mcp_roundtrip_p95_ms', label: 'mcp_roundtrip_p95' },
  { file: 'hook-cold-start', baselineKey: 'hook_cold_start_p95_ms', label: 'hook_cold_start_p95' },
];

let failed = false;
let missing = [];

for (const t of targets) {
  if (skip.has(t.file)) {
    console.log(`SKIP ${t.label} (MUSTARD_BENCH_SKIP)`);
    continue;
  }
  const resultPath = path.join(__dirname, '.results', `${t.file}.json`);
  if (!fs.existsSync(resultPath)) {
    missing.push(t.file);
    continue;
  }
  const result = JSON.parse(fs.readFileSync(resultPath, 'utf8'));
  const baseline = baselines[t.baselineKey];
  const threshold = baseline * REGRESSION_TOLERANCE;
  const observed = result.p95;
  const status = observed <= threshold ? 'OK' : 'REGRESS';
  console.log(
    `${status} ${t.label}: observed=${observed.toFixed(2)}ms baseline=${baseline}ms threshold=${threshold.toFixed(2)}ms`
  );
  if (observed > threshold) failed = true;
}

if (missing.length > 0) {
  console.error(`MISSING result files: ${missing.join(', ')} — run \`bun run bench\` first.`);
  process.exit(1);
}

if (failed) {
  console.error('Bench regression detected. Adjust code or update baseline (with justification).');
  process.exit(1);
}

console.log('All bench metrics within tolerance.');
