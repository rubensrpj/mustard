#!/usr/bin/env bun
'use strict';

/**
 * AC #7: 100 search_knowledge calls in a loop; p95 must be <10ms.
 *
 * Measures end-to-end MCP round-trip (stdin write → stdout response parse).
 * Init handshake is paid once before the loop.
 *
 * Run:  node tests/integration/mcp-latency.js
 *
 * Env: MUSTARD_MCP_LATENCY_BUDGET_MS (default 10) — override p95 budget.
 */

const assert = require('node:assert');
const {
  McpClient, makeFixture, writeKnowledge, runMigration, cleanup,
} = require('./mcp-helpers.cjs');

const N = 100;
const BUDGET_MS = Number(process.env.MUSTARD_MCP_LATENCY_BUDGET_MS || 10);

function p95(samples) {
  const sorted = samples.slice().sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.floor(0.95 * sorted.length));
  return sorted[idx];
}

async function main() {
  const fix = makeFixture('latency');
  const entries = [];
  for (let i = 0; i < 100; i++) {
    entries.push({
      id: 'k' + i,
      type: 'pattern',
      name: 'pattern-' + i,
      description: 'sample pattern entry number ' + i,
      confidence: 0.5 + (i % 50) / 100,
      createdAt: '2026-01-01',
      updatedAt: '2026-01-01',
      source: 'spec',
    });
  }
  writeKnowledge(fix, entries);
  runMigration(fix);

  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    // warmup — first call carries lazy init cost on the server side.
    await client.callTool('search_knowledge', { query: 'pattern', limit: 5 });

    const durations = [];
    for (let i = 0; i < N; i++) {
      const t0 = process.hrtime.bigint();
      await client.callTool('search_knowledge', { query: 'pattern', limit: 5 });
      const t1 = process.hrtime.bigint();
      durations.push(Number(t1 - t0) / 1e6);
    }
    const p = p95(durations);
    const avg = durations.reduce((a, b) => a + b, 0) / durations.length;
    console.log('mcp-latency: n=' + N + ' avg=' + avg.toFixed(2) + 'ms p95=' + p.toFixed(2) + 'ms budget=' + BUDGET_MS + 'ms');
    assert.ok(p < BUDGET_MS, 'p95 (' + p.toFixed(2) + 'ms) exceeded budget (' + BUDGET_MS + 'ms)');
    console.log('PASS mcp-latency');
  } finally {
    client.close();
    cleanup(fix);
  }
}

main().catch((err) => { console.error('FAIL', err.message); process.exit(1); });
