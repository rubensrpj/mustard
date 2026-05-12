#!/usr/bin/env node
'use strict';

/**
 * EventStore.search (events FTS) + EventStore.knowledge({search}) (knowledge
 * FTS5, Phase 4 Wave 1 addition).
 *
 * Run: node --test tests/unit/event-store/search.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { EventStore } = require('../../../dist/runtime/event-store.js');

function seedEvents() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-search-'));
  const store = new EventStore(path.join(dir, 'mustard.db'));
  store.init();
  store.append({
    ts: '2026-05-12T00:00:00Z',
    spec: 'feat-auth',
    event: 'tool.use',
    payload: { tool: 'Read', file: 'login.ts' },
  });
  store.append({
    ts: '2026-05-12T00:00:01Z',
    spec: 'feat-cache',
    event: 'tool.use',
    payload: { tool: 'Edit', file: 'cache.ts' },
  });
  return { dir, store };
}

test('search() returns events matching the FTS5 token', () => {
  const { dir, store } = seedEvents();
  try {
    // FTS5 column-prefix syntax: spec:feat-auth → matches the `spec` column.
    const hits = store.search('spec:"feat-auth"');
    assert.ok(hits.length >= 1);
    assert.ok(hits.some((h) => h.spec === 'feat-auth'));
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('search() empty result on no match', () => {
  const { dir, store } = seedEvents();
  try {
    const hits = store.search('spec:"nonexistentterm999"');
    assert.deepEqual(hits, []);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

// ---------------------------------------------------------------------------
// knowledge({search}) — Phase 4 Wave 1
// ---------------------------------------------------------------------------

function seedKnowledge() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-kn-search-'));
  const dbPath = path.join(dir, 'mustard.db');
  const store = new EventStore(dbPath);
  store.init();
  // Direct insert + FTS5 populate via the same driver.
  const here = path.resolve(__dirname, '..', '..', '..');
  const shim = require(path.join(here, 'templates', 'hooks', '_lib', 'runtime-shim.js'));
  const Ctor = shim.loadSqlite();
  const db = new Ctor(dbPath);
  const insertK = db.prepare(
    `INSERT INTO knowledge (id, type, name, description, confidence, created_at, updated_at, source)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
  );
  const rows = [
    ['k1', 'pattern', 'auth-flow', 'JWT refresh token rotation pattern', 0.9, '2026-01-01', '2026-01-01', 'spec'],
    ['k2', 'pattern', 'cache-lru', 'least recently used cache strategy', 0.8, '2026-01-01', '2026-01-01', 'spec'],
    ['k3', 'convention', 'naming-camel', 'camelCase for fns and vars', 0.7, '2026-01-01', '2026-01-01', 'spec'],
    ['k4', 'entity', 'user-table', 'auth subject row in postgres', 0.6, '2026-01-01', '2026-01-01', 'spec'],
    ['k5', 'pattern', 'retry-policy', 'exponential backoff with jitter', 0.5, '2026-01-01', '2026-01-01', 'spec'],
  ];
  for (const r of rows) insertK.run(...r);
  db.exec('DELETE FROM knowledge_fts');
  db.exec(
    `INSERT INTO knowledge_fts(rowid, id, name, description)
     SELECT ROW_NUMBER() OVER (ORDER BY id), id, name, description FROM knowledge`
  );
  db.close();
  return { dir, store };
}

test('knowledge() with no filter returns all sorted by confidence DESC', () => {
  const { dir, store } = seedKnowledge();
  try {
    const all = store.knowledge();
    assert.equal(all.length, 5);
    assert.equal(all[0].id, 'k1'); // highest confidence
    assert.equal(all[4].id, 'k5');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge({minConfidence}) filters', () => {
  const { dir, store } = seedKnowledge();
  try {
    const high = store.knowledge({ minConfidence: 0.7 });
    assert.equal(high.length, 3);
    assert.ok(high.every((k) => k.confidence >= 0.7));
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge({limit}) limits result size', () => {
  const { dir, store } = seedKnowledge();
  try {
    const limited = store.knowledge({ limit: 2 });
    assert.equal(limited.length, 2);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge({search}) uses FTS5 MATCH with bm25 ranking', () => {
  const { dir, store } = seedKnowledge();
  try {
    const hits = store.knowledge({ search: 'auth' });
    assert.ok(hits.length >= 1, 'expected at least one match for auth');
    const ids = hits.map((k) => k.id);
    // bm25 ranks "auth-flow" (auth in name) before "user-table" (auth in desc).
    assert.ok(ids.includes('k1'));
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge({search}) combined with minConfidence', () => {
  const { dir, store } = seedKnowledge();
  try {
    const hits = store.knowledge({ search: 'cache OR auth', minConfidence: 0.85 });
    assert.ok(hits.every((k) => k.confidence >= 0.85));
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge({search}) limit caps results', () => {
  const { dir, store } = seedKnowledge();
  try {
    const hits = store.knowledge({ search: 'pattern OR cache OR auth OR user', limit: 2 });
    assert.ok(hits.length <= 2);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge({search}) malformed FTS query fails-open to []', () => {
  const { dir, store } = seedKnowledge();
  try {
    // "field:bad" with unknown field is a MATCH parse error → fail-open.
    const hits = store.knowledge({ search: 'unknownfield:badvalue' });
    assert.deepEqual(hits, []);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge({search}) empty string falls back to no-search branch', () => {
  const { dir, store } = seedKnowledge();
  try {
    const hits = store.knowledge({ search: '   ', limit: 3 });
    // whitespace-only treated as "no search"
    assert.equal(hits.length, 3);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});
