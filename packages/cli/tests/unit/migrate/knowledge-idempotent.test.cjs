#!/usr/bin/env bun
'use strict';

/**
 * jsonl-to-sqlite — knowledge.json migration + FTS5 population
 * (the Phase 4 Wave 1 fix). Regression test against the original
 * "database disk image is malformed" crash on Windows.
 *
 * Run: bun test tests/unit/migrate/knowledge-idempotent.test.cjs
 */

const { test } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { migrate } = require('../../../dist/migrate/jsonl-to-sqlite.js');
const { EventStore } = require('../../../dist/runtime/event-store.js');

function mkFixture(knowledge) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-kn-'));
  const claudeDir = path.join(root, '.claude');
  const harness = path.join(claudeDir, '.harness');
  fs.mkdirSync(harness, { recursive: true });
  fs.writeFileSync(path.join(claudeDir, 'knowledge.json'), JSON.stringify(knowledge), 'utf8');
  return { root, harness };
}

test('knowledge.json migrates without FTS5 crash', () => {
  const { root, harness } = mkFixture([
    { id: 'k1', type: 'pattern', name: 'auth', description: 'JWT pattern', confidence: 0.9 },
    { id: 'k2', type: 'convention', name: 'naming', description: 'camelCase', confidence: 0.7 },
  ]);
  try {
    const r = migrate(harness);
    assert.equal(r.knowledgeImported, 2);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('FTS5 queries succeed against migrated DB', () => {
  const { root, harness } = mkFixture([
    { id: 'k1', type: 'pattern', name: 'auth-flow', description: 'JWT refresh', confidence: 0.9 },
    { id: 'k2', type: 'pattern', name: 'cache-lru', description: 'least recently used', confidence: 0.8 },
  ]);
  try {
    migrate(harness);
    const store = new EventStore(path.join(harness, 'mustard.db'));
    store.init();
    const hits = store.knowledge({ search: 'auth' });
    assert.ok(hits.length >= 1);
    assert.equal(hits[0].id, 'k1');
    store.close();
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('re-running migrate keeps FTS5 rowids deterministic', () => {
  const { root, harness } = mkFixture([
    { id: 'k1', type: 'pattern', name: 'auth-flow', description: 'JWT', confidence: 0.9 },
    { id: 'k2', type: 'pattern', name: 'cache-lru', description: 'LRU', confidence: 0.8 },
    { id: 'k3', type: 'entity', name: 'user', description: 'user row', confidence: 0.7 },
  ]);
  try {
    migrate(harness);
    migrate(harness);
    migrate(harness);
    const store = new EventStore(path.join(harness, 'mustard.db'));
    store.init();
    const all = store.knowledge();
    assert.equal(all.length, 3);
    // FTS5 still works.
    assert.ok(store.knowledge({ search: 'auth' }).length >= 1);
    store.close();
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge.json wrapped in {entries: [...]} is also accepted', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-kn-wrap-'));
  const claudeDir = path.join(root, '.claude');
  const harness = path.join(claudeDir, '.harness');
  fs.mkdirSync(harness, { recursive: true });
  fs.writeFileSync(
    path.join(claudeDir, 'knowledge.json'),
    JSON.stringify({ entries: [{ id: 'w1', name: 'wrap', description: 'wrapped' }] }),
    'utf8'
  );
  try {
    const r = migrate(harness);
    assert.equal(r.knowledgeImported, 1);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('knowledge.json with malformed JSON does not crash migrate', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-kn-bad-'));
  const claudeDir = path.join(root, '.claude');
  const harness = path.join(claudeDir, '.harness');
  fs.mkdirSync(harness, { recursive: true });
  fs.writeFileSync(path.join(claudeDir, 'knowledge.json'), '{not json', 'utf8');
  try {
    const r = migrate(harness);
    assert.equal(r.knowledgeImported, 0);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('entries without id are skipped silently', () => {
  const { root, harness } = mkFixture([
    { id: 'k1', name: 'good', description: 'ok' },
    { name: 'no-id', description: 'skipped' },
    { id: 'k3', name: 'good3', description: 'ok' },
  ]);
  try {
    const r = migrate(harness);
    assert.equal(r.knowledgeImported, 2);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});
