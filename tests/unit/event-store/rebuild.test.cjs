#!/usr/bin/env node
'use strict';

/**
 * EventStore.rebuild — projections derived from events.
 *
 * Run: node --test tests/unit/event-store/rebuild.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { EventStore } = require('../../../dist/runtime/event-store.js');

function mkTmpStore() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-rebuild-'));
  const store = new EventStore(path.join(dir, 'mustard.db'));
  store.init();
  return { dir, store };
}

test('rebuild creates a spec row from any spec-tagged event', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'feat-x', event: 'tool.use', payload: { tool: 'Edit' } });
    store.rebuild();
    const specs = store.specs();
    assert.equal(specs.length, 1);
    assert.equal(specs[0].name, 'feat-x');
    assert.equal(specs[0].status, 'active');
    assert.equal(specs[0].startedAt, '2026-05-12T00:00:00Z');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild marks spec.complete events as completed', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'feat-x', event: 'spec.start' });
    store.append({ ts: '2026-05-12T00:00:10Z', spec: 'feat-x', event: 'spec.complete' });
    store.rebuild();
    const [s] = store.specs();
    assert.equal(s.status, 'completed');
    assert.equal(s.completedAt, '2026-05-12T00:00:10Z');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild marks spec.cancel events as cancelled', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'feat-x', event: 'spec.start' });
    store.append({ ts: '2026-05-12T00:00:10Z', spec: 'feat-x', event: 'spec.cancel' });
    store.rebuild();
    const [s] = store.specs();
    assert.equal(s.status, 'cancelled');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild derives phase from pipeline.phase events', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'feat-x', event: 'spec.start' });
    store.append({ ts: '2026-05-12T00:00:01Z', spec: 'feat-x', event: 'pipeline.phase', payload: { from: 'ANALYZE', to: 'PLAN' } });
    store.rebuild();
    const [s] = store.specs();
    assert.equal(s.phase, 'PLAN');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild derives phase from phase.enter events', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'feat-x', event: 'phase.enter', payload: { phase: 'EXECUTE' } });
    store.rebuild();
    const [s] = store.specs();
    assert.equal(s.phase, 'EXECUTE');
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild accumulates metrics: apiCalls excludes Read', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'm-spec', event: 'spec.start' });
    store.append({ ts: '2026-05-12T00:00:01Z', spec: 'm-spec', event: 'tool.use', payload: { tool: 'Read' } });
    store.append({ ts: '2026-05-12T00:00:02Z', spec: 'm-spec', event: 'tool.use', payload: { tool: 'Edit' } });
    store.append({ ts: '2026-05-12T00:00:03Z', spec: 'm-spec', event: 'tool.use', payload: { tool: 'Edit' } });
    store.rebuild();
    const m = store.metrics('m-spec');
    assert.equal(m.apiCalls, 2);
    assert.equal(m.toolBreakdown.Edit, 2);
    assert.equal(m.toolBreakdown.Read, undefined);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild counts retries from dispatch.failure events', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'r-spec', event: 'spec.start' });
    store.append({ ts: '2026-05-12T00:00:01Z', spec: 'r-spec', event: 'dispatch.failure', payload: { phase: 'EXECUTE' } });
    store.append({ ts: '2026-05-12T00:00:02Z', spec: 'r-spec', event: 'dispatch.failure', payload: { phase: 'EXECUTE' } });
    store.rebuild();
    const m = store.metrics('r-spec');
    assert.equal(m.retries, 2);
    assert.equal(m.dispatchFailuresByPhase.EXECUTE, 2);
    assert.equal(m.pass1, false);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild pass1 = true when no retries', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'p1', event: 'spec.start' });
    store.append({ ts: '2026-05-12T00:00:01Z', spec: 'p1', event: 'tool.use', payload: { tool: 'Edit' } });
    store.rebuild();
    const m = store.metrics('p1');
    assert.equal(m.pass1, true);
    assert.equal(m.retries, 0);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild agent.start counts go into agentCount', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'a', event: 'spec.start' });
    store.append({ ts: '2026-05-12T00:00:01Z', spec: 'a', event: 'agent.start' });
    store.append({ ts: '2026-05-12T00:00:02Z', spec: 'a', event: 'agent.start' });
    store.append({ ts: '2026-05-12T00:00:03Z', spec: 'a', event: 'agent.start' });
    store.rebuild();
    const m = store.metrics('a');
    assert.equal(m.agentCount, 3);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild is idempotent (re-run yields same projections)', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', spec: 'idem', event: 'tool.use', payload: { tool: 'Edit' } });
    store.append({ ts: '2026-05-12T00:00:01Z', spec: 'idem', event: 'spec.complete' });
    store.rebuild();
    const before = JSON.stringify(store.metrics('idem'));
    store.rebuild();
    const after = JSON.stringify(store.metrics('idem'));
    assert.equal(before, after);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('rebuild ignores events without a spec', () => {
  const { dir, store } = mkTmpStore();
  try {
    store.append({ ts: '2026-05-12T00:00:00Z', event: 'global.event' });
    store.rebuild();
    assert.equal(store.specs().length, 0);
  } finally {
    store.close();
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});
