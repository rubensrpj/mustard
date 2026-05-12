#!/usr/bin/env node
'use strict';

/**
 * TokenTracker.startSpan / endSpan lifecycle.
 *
 * Run: node --test tests/unit/token-tracker/start-end-span.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { TokenTracker } = require('../../../dist/telemetry/token-tracker.js');

function mkTmp() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-tt-'));
  return { dir, spans: path.join(dir, 'spans.jsonl') };
}

test('startSpan writes a sidecar keyed by toolUseId', () => {
  const { dir, spans } = mkTmp();
  try {
    const t = new TokenTracker(spans);
    const ctx = t.startSpan({
      name: 'task.dispatch',
      toolUseId: 'tu-1',
      model: 'claude-opus-4-7',
      agentType: 'general-purpose',
      promptBytes: 1024,
    });
    assert.match(ctx.traceId, /^[0-9a-f]{32}$/);
    assert.match(ctx.spanId, /^[0-9a-f]{16}$/);
    const sidecar = path.join(dir, '.active-spans', 'tu-1.json');
    assert.ok(fs.existsSync(sidecar), 'sidecar exists');
    const rec = JSON.parse(fs.readFileSync(sidecar, 'utf8'));
    assert.equal(rec.name, 'task.dispatch');
    assert.equal(rec.model, 'claude-opus-4-7');
    assert.equal(rec.promptBytes, 1024);
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('endSpan reads sidecar, emits OTLP line, removes sidecar', () => {
  const { dir, spans } = mkTmp();
  try {
    const t = new TokenTracker(spans);
    t.startSpan({
      name: 'task.dispatch',
      toolUseId: 'tu-2',
      model: 'claude-opus-4-7',
      agentType: 'general-purpose',
      promptBytes: 1024,
    });
    t.endSpan({ toolUseId: 'tu-2', responseBytes: 512 });
    const sidecar = path.join(dir, '.active-spans', 'tu-2.json');
    assert.ok(!fs.existsSync(sidecar), 'sidecar removed');
    const lines = fs.readFileSync(spans, 'utf8').split('\n').filter(Boolean);
    assert.equal(lines.length, 1);
    const wrapper = JSON.parse(lines[0]);
    const span = wrapper.resourceSpans[0].scopeSpans[0].spans[0];
    assert.equal(span.name, 'task.dispatch');
    assert.equal(span.status.code, 1); // OK
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('endSpan without prior startSpan is a no-op (no crash)', () => {
  const { dir, spans } = mkTmp();
  try {
    const t = new TokenTracker(spans);
    // Should warn to stderr but not throw.
    t.endSpan({ toolUseId: 'never-started', responseBytes: 100 });
    assert.ok(!fs.existsSync(spans), 'no spans file emitted');
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('startSpan propagates spec/phase/wave into sidecar + final span', () => {
  const { dir, spans } = mkTmp();
  try {
    const t = new TokenTracker(spans);
    t.startSpan({
      name: 'task.dispatch',
      toolUseId: 'tu-3',
      model: 'claude-sonnet-4-7',
      agentType: 'Explore',
      spec: 'feat-x',
      phase: 'EXECUTE',
      wave: 2,
      promptBytes: 2048,
    });
    t.endSpan({ toolUseId: 'tu-3', responseBytes: 256 });
    const lines = fs.readFileSync(spans, 'utf8').split('\n').filter(Boolean);
    const span = JSON.parse(lines[0]).resourceSpans[0].scopeSpans[0].spans[0];
    const attrs = Object.fromEntries(
      span.attributes.map((kv) => [kv.key, kv.value.stringValue ?? kv.value.intValue ?? kv.value.doubleValue])
    );
    assert.equal(attrs['mustard.spec'], 'feat-x');
    assert.equal(attrs['mustard.phase'], 'EXECUTE');
    assert.equal(attrs['mustard.wave'], '2');
    assert.equal(attrs['mustard.agent_type'], 'Explore');
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('endSpan with isError sets status.code=2 and error attribute', () => {
  const { dir, spans } = mkTmp();
  try {
    const t = new TokenTracker(spans);
    t.startSpan({
      name: 'task.dispatch',
      toolUseId: 'tu-4',
      model: 'claude-opus-4-7',
      agentType: 'general-purpose',
      promptBytes: 100,
    });
    t.endSpan({ toolUseId: 'tu-4', responseBytes: 0, isError: true, errorType: 'timeout' });
    const lines = fs.readFileSync(spans, 'utf8').split('\n').filter(Boolean);
    const span = JSON.parse(lines[0]).resourceSpans[0].scopeSpans[0].spans[0];
    assert.equal(span.status.code, 2);
    const errAttr = span.attributes.find((kv) => kv.key === 'error.type');
    assert.equal(errAttr?.value.stringValue, 'timeout');
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('parentSpanId is propagated when supplied', () => {
  const { dir, spans } = mkTmp();
  try {
    const t = new TokenTracker(spans);
    const parent = 'a'.repeat(16);
    t.startSpan({
      name: 'task.dispatch',
      toolUseId: 'tu-5',
      model: 'claude-opus-4-7',
      agentType: 'general-purpose',
      promptBytes: 10,
      parentSpanId: parent,
    });
    t.endSpan({ toolUseId: 'tu-5', responseBytes: 10 });
    const lines = fs.readFileSync(spans, 'utf8').split('\n').filter(Boolean);
    const span = JSON.parse(lines[0]).resourceSpans[0].scopeSpans[0].spans[0];
    assert.equal(span.parentSpanId, parent);
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});

test('tool_use_id is sanitized for filesystem-safe sidecar path', () => {
  const { dir, spans } = mkTmp();
  try {
    const t = new TokenTracker(spans);
    // toolUseId with slashes — sanitized to underscores.
    t.startSpan({
      name: 'x',
      toolUseId: 'a/b\\c:d',
      model: 'claude-opus-4-7',
      agentType: 'general-purpose',
      promptBytes: 1,
    });
    const dirEntries = fs.readdirSync(path.join(dir, '.active-spans'));
    assert.equal(dirEntries.length, 1);
    assert.ok(!dirEntries[0].includes('/'));
    assert.ok(!dirEntries[0].includes('\\'));
    assert.ok(!dirEntries[0].includes(':'));
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch {}
  }
});
