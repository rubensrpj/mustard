#!/usr/bin/env node
'use strict';

/**
 * jsonl-to-sqlite — events migration idempotency.
 *
 * Run: node --test tests/unit/migrate/events-idempotent.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { migrate } = require('../../../dist/migrate/jsonl-to-sqlite.js');
const { EventStore } = require('../../../dist/runtime/event-store.js');

function mkFixture(events) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-'));
  const harness = path.join(root, '.claude', '.harness');
  fs.mkdirSync(harness, { recursive: true });
  const lines = events.map((e) => JSON.stringify(e)).join('\n') + '\n';
  fs.writeFileSync(path.join(harness, 'events.jsonl'), lines, 'utf8');
  return { root, harness };
}

test('first migrate imports all events', () => {
  const { root, harness } = mkFixture([
    { ts: '2026-05-12T00:00:00Z', event: 'spec.start', spec: 'a', actor: { kind: 'agent', id: 'g' } },
    { ts: '2026-05-12T00:00:01Z', event: 'tool.use', spec: 'a', actor: { kind: 'agent', id: 'g' }, payload: { tool: 'Edit' } },
  ]);
  try {
    const r = migrate(harness);
    assert.equal(r.eventsImported, 2);
    assert.equal(r.eventsSkipped, 0);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('second migrate skips all events (idempotency)', () => {
  const { root, harness } = mkFixture([
    { ts: '2026-05-12T00:00:00Z', event: 'spec.start', spec: 'a', actor: { kind: 'agent', id: 'g' } },
    { ts: '2026-05-12T00:00:01Z', event: 'tool.use', spec: 'a', actor: { kind: 'agent', id: 'g' }, payload: { tool: 'Edit' } },
  ]);
  try {
    migrate(harness);
    const r2 = migrate(harness);
    assert.equal(r2.eventsImported, 0);
    assert.equal(r2.eventsSkipped, 2);
    const store = new EventStore(path.join(harness, 'mustard.db'));
    store.init();
    assert.equal(store.eventCount(), 2);
    store.close();
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('events without ts or event are skipped', () => {
  const { root, harness } = mkFixture([
    { ts: '2026-05-12T00:00:00Z', event: 'good' },
    { event: 'no-ts' },
    { ts: '2026-05-12T00:00:01Z' },
  ]);
  try {
    const r = migrate(harness);
    assert.equal(r.eventsImported, 1);
    assert.equal(r.eventsSkipped, 2);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('malformed JSON lines do not crash migration', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-bad-'));
  const harness = path.join(root, '.claude', '.harness');
  fs.mkdirSync(harness, { recursive: true });
  fs.writeFileSync(
    path.join(harness, 'events.jsonl'),
    '{"ts":"2026-05-12T00:00:00Z","event":"good"}\nnot-json\n{"ts":"2026-05-12T00:00:01Z","event":"good2"}\n',
    'utf8'
  );
  try {
    const r = migrate(harness);
    assert.equal(r.eventsImported, 2);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('missing events.jsonl is handled gracefully', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-empty-'));
  const harness = path.join(root, '.claude', '.harness');
  fs.mkdirSync(harness, { recursive: true });
  try {
    const r = migrate(harness);
    assert.equal(r.eventsImported, 0);
    assert.equal(r.eventsSkipped, 0);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('migrate throws when harnessDir missing', () => {
  const nonExistent = path.join(os.tmpdir(), 'mustard-ut-mig-nonexistent-' + Date.now());
  assert.throws(() => migrate(nonExistent), /harness dir not found/);
});
