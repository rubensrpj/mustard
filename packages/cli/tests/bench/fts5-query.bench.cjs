#!/usr/bin/env bun
'use strict';

/**
 * FTS5 query benchmark (Phase 4 Wave 2).
 *
 * Seeds ~1000 knowledge rows, runs `store.knowledge({search})` in a loop,
 * computes p50/p95/p99 latency, emits `.results/fts5-query.json`.
 *
 * Run: bun tests/bench/fts5-query.bench.cjs
 */

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const REPO_ROOT = path.resolve(__dirname, '..', '..');
const { EventStore } = require(path.join(REPO_ROOT, 'dist', 'runtime', 'event-store.js'));
const shim = require(path.join(REPO_ROOT, 'templates', 'hooks', '_lib', 'runtime-shim.js'));

const ITERATIONS = Number(process.env.MUSTARD_BENCH_ITER || 200);
const SEED_ROWS = 1000;
const SEARCH_TERMS = ['pattern', 'auth', 'cache', 'retry', 'naming'];

function p(arr, q) {
  const sorted = [...arr].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.floor(sorted.length * q));
  return sorted[idx];
}

function main() {
  const Ctor = shim.loadSqlite();
  if (!Ctor) {
    console.error('[fts5-query.bench] bun:sqlite unavailable — run under bun');
    process.exit(2);
  }

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-bench-fts5-'));
  const dbPath = path.join(dir, 'mustard.db');
  const store = new EventStore(dbPath);
  store.init();

  // Seed knowledge rows directly via SQLite (bypasses migration path).
  const db = new Ctor(dbPath);
  const insertK = db.prepare(
    `INSERT INTO knowledge (id, type, name, description, confidence, created_at, updated_at, source)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
  );
  const types = ['pattern', 'convention', 'entity', 'decision'];
  const tokens = ['auth', 'cache', 'retry', 'pattern', 'naming', 'flow', 'policy', 'config', 'state', 'event'];
  db.exec('BEGIN');
  for (let i = 0; i < SEED_ROWS; i++) {
    const t = types[i % types.length];
    const name = `${tokens[i % tokens.length]}-${t}-${i}`;
    const desc = `${tokens[(i + 3) % tokens.length]} strategy with ${tokens[(i + 5) % tokens.length]} backoff #${i}`;
    insertK.run(`k${i}`, t, name, desc, 0.5 + ((i % 50) / 100), '2026-01-01', '2026-01-01', 'spec');
  }
  db.exec('COMMIT');
  db.exec('DELETE FROM knowledge_fts');
  db.exec(
    `INSERT INTO knowledge_fts(rowid, id, name, description)
     SELECT ROW_NUMBER() OVER (ORDER BY id), id, name, description FROM knowledge`
  );
  db.close();

  // Warmup
  for (let i = 0; i < 10; i++) store.knowledge({ search: 'pattern', limit: 5 });

  // Measure
  const samples = [];
  for (let i = 0; i < ITERATIONS; i++) {
    const term = SEARCH_TERMS[i % SEARCH_TERMS.length];
    const t0 = performance.now();
    const hits = store.knowledge({ search: term, limit: 5 });
    const dur = performance.now() - t0;
    samples.push(dur);
    if (i === 0 && hits.length === 0) {
      console.error('[fts5-query.bench] warning: zero hits for first query');
    }
  }

  store.close();
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch { /* ignore */ }

  const out = {
    iterations: ITERATIONS,
    seedRows: SEED_ROWS,
    p50: p(samples, 0.5),
    p95: p(samples, 0.95),
    p99: p(samples, 0.99),
    min: Math.min(...samples),
    max: Math.max(...samples),
    avg: samples.reduce((a, b) => a + b, 0) / samples.length,
    runtime: shim.pickRuntime(),
    ts: new Date().toISOString(),
  };

  const outDir = path.join(__dirname, '.results');
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, 'fts5-query.json'), JSON.stringify(out, null, 2));
  console.log(`[fts5-query.bench] p50=${out.p50.toFixed(2)}ms p95=${out.p95.toFixed(2)}ms p99=${out.p99.toFixed(2)}ms`);
}

main();
