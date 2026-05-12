#!/usr/bin/env node
'use strict';

/**
 * EventStore.append — unit coverage.
 *
 * Tests:
 *  - append writes a row readable via eventCount + query
 *  - all optional fields (sessionId, wave, spec, actor) round-trip
 *  - payload serializes/deserializes as JSON
 *  - requireDb throws when init() not called
 *
 * Run: node --test tests/unit/event-store/append.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { EventStore } = require('../../../dist/runtime/event-store.js');

function mkTmpDb() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-append-'));
  return { dir, path: path.join(dir, 'mustard.db') };
}

test('append → eventCount reflects insertion', () => {
  const { dir, path: dbPath } = mkTmpDb();
  try {
    const store = new EventStore(dbPath);
    store.init();
    assert.equal(store.eventCount(), 0);
    store.append({ ts: '2026-05-12T00:00:00Z', event: 'spec.start' });
    assert.equal(store.eventCount(), 1);
    store.append({ ts: '2026-05-12T00:00:01Z', event: 'spec.complete' });
    assert.equal(store.eventCount(), 2);
    store.close();
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('append round-trips all optional fields', () => {
  const { dir, path: dbPath } = mkTmpDb();
  try {
    const store = new EventStore(dbPath);
    store.init();
    const ev = {
      ts: '2026-05-12T00:00:00Z',
      sessionId: 'sess-1',
      wave: 4,
      spec: 'feat-x',
      event: 'tool.use',
      actor: { kind: 'agent', id: 'general-purpose' },
      payload: { tool: 'Read', file: 'x.ts', count: 3 },
    };
    store.append(ev);
    const [row] = store.query();
    assert.equal(row.ts, ev.ts);
    assert.equal(row.sessionId, 'sess-1');
    assert.equal(row.wave, 4);
    assert.equal(row.spec, 'feat-x');
    assert.equal(row.event, 'tool.use');
    assert.deepEqual(row.actor, { kind: 'agent', id: 'general-purpose' });
    assert.deepEqual(row.payload, ev.payload);
    store.close();
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('append works without optional fields', () => {
  const { dir, path: dbPath } = mkTmpDb();
  try {
    const store = new EventStore(dbPath);
    store.init();
    store.append({ ts: '2026-05-12T00:00:00Z', event: 'minimal' });
    const [row] = store.query();
    assert.equal(row.event, 'minimal');
    assert.equal(row.sessionId, undefined);
    assert.equal(row.spec, undefined);
    assert.equal(row.actor, undefined);
    assert.deepEqual(row.payload, {});
    store.close();
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('append before init throws', () => {
  const { dir, path: dbPath } = mkTmpDb();
  try {
    const store = new EventStore(dbPath);
    assert.throws(
      () => store.append({ ts: '2026-05-12T00:00:00Z', event: 'x' }),
      /call init/
    );
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('init is idempotent (memoized)', () => {
  const { dir, path: dbPath } = mkTmpDb();
  try {
    const store = new EventStore(dbPath);
    store.init();
    store.init(); // no-op
    store.append({ ts: '2026-05-12T00:00:00Z', event: 'a' });
    assert.equal(store.eventCount(), 1);
    store.close();
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('close resets state and allows re-init', () => {
  const { dir, path: dbPath } = mkTmpDb();
  try {
    const store = new EventStore(dbPath);
    store.init();
    store.append({ ts: '2026-05-12T00:00:00Z', event: 'a' });
    store.close();
    store.close(); // double close is a no-op
    store.init();
    assert.equal(store.eventCount(), 1);
    store.close();
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('tables() reports the core schema', () => {
  const { dir, path: dbPath } = mkTmpDb();
  try {
    const store = new EventStore(dbPath);
    store.init();
    const t = store.tables();
    for (const expected of ['events', 'specs', 'metrics_projection', 'knowledge', 'spans']) {
      assert.ok(t.includes(expected), `expected table ${expected} in ${t.join(',')}`);
    }
    store.close();
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});
