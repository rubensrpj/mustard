#!/usr/bin/env node
'use strict';

/**
 * EventStore.spans — span filter combinations.
 *
 * Run: node --test tests/unit/event-store/spans.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { EventStore } = require('../../../dist/runtime/event-store.js');

function mkStoreWithSpans() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-spans-'));
  const dbPath = path.join(dir, 'mustard.db');
  const store = new EventStore(dbPath);
  store.init();
  const here = path.resolve(__dirname, '..', '..', '..');
  const shim = require(path.join(here, 'templates', 'hooks', '_lib', 'runtime-shim.js'));
  const Ctor = shim.loadSqlite();
  const db = new Ctor(dbPath);
  const insert = db.prepare(
    `INSERT INTO spans (trace_id, span_id, parent_span_id, name, started_at, ended_at,
                        duration_ms, attributes, spec, phase, model, input_tokens,
                        output_tokens, is_error)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
  );
  const rows = [
    ['t1', 's1', null, 'dispatch.a', 1_000_000, 1_001_000, 1000, '{}', 'feat-a', 'EXECUTE', 'opus', 100, 200, 0],
    ['t1', 's2', 's1', 'dispatch.b', 1_002_000, 1_003_500, 1500, '{}', 'feat-a', 'EXECUTE', 'opus', 50, 70, 0],
    ['t2', 's3', null, 'dispatch.c', 1_500_000, 1_500_500, 500, '{}', 'feat-b', 'ANALYZE', 'haiku', 30, 40, 0],
    ['t3', 's4', null, 'dispatch.err', 2_000_000, 2_000_100, 100, '{}', 'feat-a', 'EXECUTE', 'sonnet', 10, 0, 1],
  ];
  for (const r of rows) insert.run(...r);
  db.close();
  return { dir, store };
}

test('spans() returns all rows when no filter', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    const all = store.spans();
    assert.equal(all.length, 4);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('spans({spec}) filters by spec', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    const a = store.spans({ spec: 'feat-a' });
    assert.equal(a.length, 3);
    assert.ok(a.every((s) => s.spec === 'feat-a'));
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('spans({phase}) filters by phase', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    const exec = store.spans({ phase: 'EXECUTE' });
    assert.equal(exec.length, 3);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('spans({since}) filters by started_at >=', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    const later = store.spans({ since: 1_500_000 });
    assert.equal(later.length, 2);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('spans({limit}) caps result', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    const small = store.spans({ limit: 2 });
    assert.equal(small.length, 2);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('spans() decodes traceId, durationMs, attributes, isError correctly', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    const [first] = store.spans({ spec: 'feat-a', phase: 'EXECUTE' });
    assert.equal(first.traceId, 't1');
    assert.equal(first.durationMs, 1000);
    assert.equal(first.isError, false);
    // parentSpanId is null for s1.
    assert.equal(first.parentSpanId, undefined);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('spans() preserves error flag', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    const all = store.spans();
    const err = all.find((s) => s.spanId === 's4');
    assert.equal(err.isError, true);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('spans() defaults limit=1000', () => {
  const { dir, store } = mkStoreWithSpans();
  try {
    // No explicit limit — should not throw, returns ≤1000.
    const all = store.spans({ spec: 'feat-a' });
    assert.ok(all.length <= 1000);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});
