#!/usr/bin/env bun
'use strict';

/**
 * EventStore.query — filter combinations.
 *
 * Run: bun test tests/unit/event-store/query.test.cjs
 */

const { test } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { EventStore } = require('../../../dist/runtime/event-store.js');

function seed() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-query-'));
  const store = new EventStore(path.join(dir, 'mustard.db'));
  store.init();
  const rows = [
    { ts: '2026-05-12T00:00:00Z', spec: 'feat-a', event: 'spec.start' },
    { ts: '2026-05-12T00:00:01Z', spec: 'feat-a', event: 'tool.use', payload: { tool: 'Edit' } },
    { ts: '2026-05-12T00:00:02Z', spec: 'feat-b', event: 'tool.use', payload: { tool: 'Read' } },
    { ts: '2026-05-12T00:00:03Z', spec: 'feat-a', event: 'spec.complete' },
    { ts: '2026-05-12T01:00:00Z', spec: 'feat-b', event: 'spec.complete' },
  ];
  for (const r of rows) store.append(r);
  return { dir, store };
}

test('query() with no filter returns all rows in insertion order', () => {
  const { dir, store } = seed();
  try {
    const all = store.query();
    assert.equal(all.length, 5);
    assert.equal(all[0].event, 'spec.start');
    assert.equal(all[4].event, 'spec.complete');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('query({spec}) filters by spec', () => {
  const { dir, store } = seed();
  try {
    const aRows = store.query({ spec: 'feat-a' });
    assert.equal(aRows.length, 3);
    assert.ok(aRows.every((r) => r.spec === 'feat-a'));
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('query({event}) filters by event name', () => {
  const { dir, store } = seed();
  try {
    const tools = store.query({ event: 'tool.use' });
    assert.equal(tools.length, 2);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('query({since}) filters by ts >=', () => {
  const { dir, store } = seed();
  try {
    const later = store.query({ since: '2026-05-12T00:30:00Z' });
    assert.equal(later.length, 1);
    assert.equal(later[0].spec, 'feat-b');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('query combines spec + event filters', () => {
  const { dir, store } = seed();
  try {
    const r = store.query({ spec: 'feat-a', event: 'tool.use' });
    assert.equal(r.length, 1);
    assert.equal(r[0].payload.tool, 'Edit');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('query returns empty array when no matches', () => {
  const { dir, store } = seed();
  try {
    const r = store.query({ spec: 'does-not-exist' });
    assert.deepEqual(r, []);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('specs() returns the projected list, sorted by name', () => {
  const { dir, store } = seed();
  try {
    store.rebuild();
    const specs = store.specs();
    const names = specs.map((s) => s.name);
    assert.deepEqual(names, ['feat-a', 'feat-b']);
    const a = specs.find((s) => s.name === 'feat-a');
    assert.equal(a.status, 'completed');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('metrics(spec) returns null when missing', () => {
  const { dir, store } = seed();
  try {
    store.rebuild();
    assert.equal(store.metrics('does-not-exist'), null);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});
