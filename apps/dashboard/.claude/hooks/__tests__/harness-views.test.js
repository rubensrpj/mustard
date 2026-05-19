#!/usr/bin/env node
'use strict';
/**
 * Tests for templates/scripts/harness-views.js
 * Run with: node --test templates/hooks/__tests__/harness-views.test.js
 */

const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const views = require('../../scripts/harness-views.js');

let tmpDir;

beforeEach(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-views-'));
});

afterEach(() => {
  try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch (_) {}
});

function ev(overrides) {
  return Object.assign({
    v: 1,
    ts: new Date().toISOString(),
    sessionId: 's-test',
    wave: 0,
    actor: { kind: 'hook' },
    event: 'tool.use',
    payload: {},
  }, overrides);
}

describe('buildAgentVisibility', () => {
  it('filters events by wave', () => {
    const events = [
      ev({ wave: 1, event: 'agent.start' }),
      ev({ wave: 2, event: 'agent.start' }),
      ev({ wave: 2, event: 'tool.use' }),
      ev({ wave: 3, event: 'agent.stop' }),
    ];
    const out = views.buildAgentVisibility(events, { wave: 2 });
    assert.equal(out.wave, 2);
    assert.equal(out.events.length, 2);
    for (const e of out.events) assert.equal(e.wave, 2);
  });

  it('defaults wave to the max seen when not provided', () => {
    const events = [
      ev({ wave: 1 }),
      ev({ wave: 5 }),
      ev({ wave: 3 }),
    ];
    const out = views.buildAgentVisibility(events, {});
    assert.equal(out.wave, 5);
    assert.equal(out.events.length, 1);
  });

  it('returns findings with confidence >= threshold regardless of wave', () => {
    const events = [
      ev({ wave: 1, event: 'finding', payload: { confidence: 0.9, content: 'high' } }),
      ev({ wave: 2, event: 'finding', payload: { confidence: 0.3, content: 'low' } }),
      ev({ wave: 3, event: 'finding', payload: { confidence: 0.75, content: 'mid' } }),
    ];
    const out = views.buildAgentVisibility(events, { wave: 99, minConfidence: 0.7 });
    const contents = out.findings.map(f => f.payload.content).sort();
    assert.deepEqual(contents, ['high', 'mid']);
  });

  it('truncates agent.stop summary to maxChars', () => {
    const longSummary = 'x'.repeat(2000);
    const events = [
      ev({ wave: 1, event: 'agent.stop', payload: { summary: longSummary, confidence: 0.8 } }),
    ];
    const out = views.buildAgentVisibility(events, { wave: 1, maxChars: 800 });
    assert.equal(out.events.length, 1);
    const truncated = out.events[0].payload.summary;
    assert.ok(truncated.length <= 810, 'summary should be truncated near 800');
    assert.ok(truncated.startsWith('xxx'));
    // Original event is not mutated.
    assert.equal(events[0].payload.summary.length, 2000);
  });

  it('caps total events via eventLimit', () => {
    const events = [];
    for (let i = 0; i < 100; i++) events.push(ev({ wave: 1, event: 'tool.use', payload: { i } }));
    const out = views.buildAgentVisibility(events, { wave: 1, eventLimit: 10 });
    assert.equal(out.events.length, 10);
    // Most recent ones kept.
    assert.equal(out.events[9].payload.i, 99);
  });
});

describe('buildPipelineState', () => {
  it('derives final phase from last pipeline.phase event for the spec', () => {
    const events = [
      ev({ spec: 'add-login', event: 'pipeline.phase', payload: { from: null, to: 'ANALYZE' } }),
      ev({ spec: 'add-login', event: 'pipeline.phase', payload: { from: 'ANALYZE', to: 'PLAN' } }),
      ev({ spec: 'other', event: 'pipeline.phase', payload: { from: 'ANALYZE', to: 'EXECUTE' } }),
      ev({ spec: 'add-login', event: 'pipeline.phase', payload: { from: 'PLAN', to: 'EXECUTE' } }),
    ];
    const out = views.buildPipelineState(events, { spec: 'add-login' });
    assert.equal(out.phase, 'EXECUTE');
    assert.equal(out.spec, 'add-login');
  });

  it('collects dispatch failures, decisions, lessons per spec', () => {
    const events = [
      ev({ spec: 'x', event: 'dispatch.failure', payload: { reason: 'overload' } }),
      ev({ spec: 'x', event: 'decision', payload: { title: 'Use Drizzle' } }),
      ev({ spec: 'x', event: 'lesson', payload: { takeaway: 'cache ids' } }),
      ev({ spec: 'y', event: 'decision', payload: { title: 'ignore me' } }),
    ];
    const out = views.buildPipelineState(events, { spec: 'x' });
    assert.equal(out.dispatchFailures.length, 1);
    assert.equal(out.decisions.length, 1);
    assert.equal(out.lessons.length, 1);
  });

  it('handles empty input gracefully', () => {
    const out = views.buildPipelineState([], { spec: 'foo' });
    assert.equal(out.phase, null);
    assert.deepEqual(out.dispatchFailures, []);
  });
});

describe('buildSessionSummary', () => {
  it('counts agents, tools, and collects findings/decisions/lessons', () => {
    const events = [
      ev({ event: 'agent.start' }),
      ev({ event: 'agent.start' }),
      ev({ event: 'tool.use' }),
      ev({ event: 'tool.use' }),
      ev({ event: 'tool.use' }),
      ev({ event: 'finding', payload: { content: 'f1', confidence: 0.8 } }),
      ev({ event: 'decision', payload: { title: 'd1' } }),
      ev({ event: 'lesson', payload: { takeaway: 'l1' } }),
      ev({ spec: 'abc', event: 'tool.use' }),
    ];
    const out = views.buildSessionSummary(events);
    assert.equal(out.agentCount, 2);
    assert.equal(out.toolCount, 4);
    assert.equal(out.findings.length, 1);
    assert.equal(out.decisions.length, 1);
    assert.equal(out.lessons.length, 1);
    assert.deepEqual(out.specs, ['abc']);
  });
});

describe('buildCrossSessionTimeline', () => {
  it('reads sessions ordered by mtime (most recent first)', async () => {
    const sessionsDir = path.join(tmpDir, 'sessions');
    fs.mkdirSync(sessionsDir, { recursive: true });

    const mk = (name, events, mtime) => {
      const fp = path.join(sessionsDir, name);
      const lines = events.map(e => JSON.stringify(e)).join('\n') + '\n';
      fs.writeFileSync(fp, lines);
      fs.utimesSync(fp, mtime / 1000, mtime / 1000);
    };

    mk('old.jsonl', [ev({ sessionId: 's-old', event: 'agent.start' })], Date.now() - 10000);
    mk('mid.jsonl', [ev({ sessionId: 's-mid', event: 'agent.start' })], Date.now() - 5000);
    mk('new.jsonl', [ev({ sessionId: 's-new', event: 'agent.start' })], Date.now());

    const out = await views.buildCrossSessionTimeline(sessionsDir, { limit: 3 });
    assert.equal(out.length, 3);
    assert.equal(out[0].sessionId, 's-new');
    assert.equal(out[1].sessionId, 's-mid');
    assert.equal(out[2].sessionId, 's-old');
  });

  it('honours limit option', async () => {
    const sessionsDir = path.join(tmpDir, 'sessions');
    fs.mkdirSync(sessionsDir, { recursive: true });
    for (let i = 0; i < 5; i++) {
      const fp = path.join(sessionsDir, `s-${i}.jsonl`);
      fs.writeFileSync(fp, JSON.stringify(ev({ sessionId: `s-${i}` })) + '\n');
      fs.utimesSync(fp, (Date.now() + i * 1000) / 1000, (Date.now() + i * 1000) / 1000);
    }
    const out = await views.buildCrossSessionTimeline(sessionsDir, { limit: 2 });
    assert.equal(out.length, 2);
  });

  it('returns [] when dir missing', async () => {
    const out = await views.buildCrossSessionTimeline(path.join(tmpDir, 'nope'), { limit: 3 });
    assert.deepEqual(out, []);
  });

  it('skips malformed lines without throwing', async () => {
    const sessionsDir = path.join(tmpDir, 'sessions');
    fs.mkdirSync(sessionsDir, { recursive: true });
    const fp = path.join(sessionsDir, 'broken.jsonl');
    fs.writeFileSync(fp, 'not json\n' + JSON.stringify(ev({ event: 'agent.start' })) + '\n');
    const out = await views.buildCrossSessionTimeline(sessionsDir, { limit: 1 });
    assert.equal(out.length, 1);
    assert.equal(out[0].agentCount, 1);
  });
});

describe('readEventsSync', () => {
  it('parses NDJSON file', () => {
    const fp = path.join(tmpDir, 'e.jsonl');
    fs.writeFileSync(fp, JSON.stringify(ev()) + '\n' + JSON.stringify(ev()) + '\n');
    const events = views.readEventsSync(fp);
    assert.equal(events.length, 2);
  });

  it('returns [] for missing file', () => {
    assert.deepEqual(views.readEventsSync(path.join(tmpDir, 'missing.jsonl')), []);
  });
});
