#!/usr/bin/env bun
'use strict';

/**
 * MCP roundtrip benchmark (Phase 4 Wave 2).
 *
 * Spawns the mustard-memory MCP server via the Phase 3 test helper,
 * issues 100 `search_knowledge` calls, computes p50/p95/p99.
 *
 * Run: bun tests/bench/mcp-roundtrip.bench.cjs
 */

const fs = require('node:fs');
const path = require('node:path');

const REPO_ROOT = path.resolve(__dirname, '..', '..');
const helpers = require(path.join(REPO_ROOT, 'tests', 'integration', 'mcp-helpers.cjs'));

const ITERATIONS = Number(process.env.MUSTARD_BENCH_ITER || 100);

function p(arr, q) {
  const sorted = [...arr].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.floor(sorted.length * q));
  return sorted[idx];
}

async function main() {
  const fixture = helpers.makeFixture('bench-roundtrip');
  const knowledge = [];
  for (let i = 0; i < 200; i++) {
    knowledge.push({
      id: `k${i}`,
      type: 'pattern',
      name: `entry-${i}`,
      description: `bench seed entry number ${i} with auth and cache tokens`,
      confidence: 0.5 + ((i % 50) / 100),
      created_at: '2026-01-01',
      updated_at: '2026-01-01',
      source: 'spec',
    });
  }
  helpers.writeKnowledge(fixture, knowledge);
  helpers.runMigration(fixture);

  const client = new helpers.McpClient(fixture.dbPath);
  try {
    await client.initialize();
    // Warmup
    for (let i = 0; i < 5; i++) await client.callTool('search_knowledge', { query: 'auth', limit: 3 });

    const samples = [];
    const queries = ['auth', 'cache', 'pattern', 'entry', 'bench'];
    for (let i = 0; i < ITERATIONS; i++) {
      const t0 = performance.now();
      await client.callTool('search_knowledge', { query: queries[i % queries.length], limit: 3 });
      samples.push(performance.now() - t0);
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
    fs.writeFileSync(path.join(outDir, 'mcp-roundtrip.json'), JSON.stringify(out, null, 2));
    console.log(`[mcp-roundtrip.bench] p50=${out.p50.toFixed(2)}ms p95=${out.p95.toFixed(2)}ms p99=${out.p99.toFixed(2)}ms`);
  } finally {
    client.close();
    helpers.cleanup(fixture);
  }
}

main().catch((err) => { console.error(err); process.exit(1); });
