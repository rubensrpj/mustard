#!/usr/bin/env bun
'use strict';

/**
 * Hook cold-start benchmark (Phase 4 Wave 2).
 *
 * Spawns `bun templates/hooks/_lib/runtime-shim.js` as a child process and
 * measures spawn→exit wall time. Loops 50 iterations, computes p50/p95/p99.
 *
 * runtime-shim.js exports module symbols; we wrap it with a tiny exit-immediate
 * harness via -e so we measure spawn + require + exit (the floor a real hook hits).
 *
 * Run: bun tests/bench/hook-cold-start.bench.cjs
 */

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const REPO_ROOT = path.resolve(__dirname, '..', '..');
const SHIM = path.join(REPO_ROOT, 'templates', 'hooks', '_lib', 'runtime-shim.js');
const ITERATIONS = Number(process.env.MUSTARD_BENCH_ITER || 50);

function p(arr, q) {
  const sorted = [...arr].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.floor(sorted.length * q));
  return sorted[idx];
}

function main() {
  // Inline harness: require shim, call pickRuntime, exit. Mirrors what a hook
  // does in its prologue (require + cheap module work).
  const inline = `const s=require(${JSON.stringify(SHIM)});s.pickRuntime();process.exit(0);`;

  // Warmup
  for (let i = 0; i < 3; i++) spawnSync('bun', ['-e', inline], { stdio: 'ignore' });

  const samples = [];
  for (let i = 0; i < ITERATIONS; i++) {
    const t0 = performance.now();
    const r = spawnSync('bun', ['-e', inline], { stdio: 'ignore' });
    const dur = performance.now() - t0;
    if (r.status !== 0) {
      console.error(`[hook-cold-start.bench] iteration ${i} failed: status=${r.status}`);
      process.exit(2);
    }
    samples.push(dur);
  }

  const out = {
    iterations: ITERATIONS,
    p50: p(samples, 0.5),
    p95: p(samples, 0.95),
    p99: p(samples, 0.99),
    min: Math.min(...samples),
    max: Math.max(...samples),
    avg: samples.reduce((a, b) => a + b, 0) / samples.length,
    ts: new Date().toISOString(),
  };

  const outDir = path.join(__dirname, '.results');
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, 'hook-cold-start.json'), JSON.stringify(out, null, 2));
  console.log(`[hook-cold-start.bench] p50=${out.p50.toFixed(2)}ms p95=${out.p95.toFixed(2)}ms p99=${out.p99.toFixed(2)}ms`);
}

main();
