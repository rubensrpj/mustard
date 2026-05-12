#!/usr/bin/env node
'use strict';

/**
 * OTLP/JSON shape — strict structural assertions on TokenTracker output.
 *
 * Run: node --test tests/unit/token-tracker/otlp-shape.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { TokenTracker } = require('../../../dist/telemetry/token-tracker.js');
const { costUsd } = require('../../../dist/telemetry/pricing.js');

function emit(opts = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-otlp-'));
  const spans = path.join(dir, 'spans.jsonl');
  const t = new TokenTracker(spans);
  t.startSpan({
    name: opts.name ?? 'task.dispatch',
    toolUseId: opts.toolUseId ?? 'tu-shape',
    model: opts.model ?? 'claude-opus-4-7',
    agentType: opts.agentType ?? 'general-purpose',
    promptBytes: opts.promptBytes ?? 4000,
    spec: opts.spec,
    phase: opts.phase,
    wave: opts.wave,
  });
  t.endSpan({ toolUseId: opts.toolUseId ?? 'tu-shape', responseBytes: opts.responseBytes ?? 400 });
  const wrapper = JSON.parse(fs.readFileSync(spans, 'utf8').split('\n')[0]);
  fs.rmSync(dir, { recursive: true, force: true });
  return wrapper;
}

test('emitted wrapper has resourceSpans → scopeSpans → spans path', () => {
  const w = emit();
  assert.ok(Array.isArray(w.resourceSpans));
  assert.equal(w.resourceSpans.length, 1);
  const rs = w.resourceSpans[0];
  assert.ok(Array.isArray(rs.scopeSpans));
  const ss = rs.scopeSpans[0];
  assert.ok(Array.isArray(ss.spans) && ss.spans.length === 1);
});

test('resource has service.name and service.version attributes', () => {
  const w = emit();
  const res = w.resourceSpans[0].resource;
  const keys = res.attributes.map((kv) => kv.key);
  assert.ok(keys.includes('service.name'));
  assert.ok(keys.includes('service.version'));
});

test('scope name is mustard.telemetry', () => {
  const w = emit();
  assert.equal(w.resourceSpans[0].scopeSpans[0].scope.name, 'mustard.telemetry');
});

test('span ids conform to OTel hex spec', () => {
  const w = emit();
  const span = w.resourceSpans[0].scopeSpans[0].spans[0];
  assert.match(span.traceId, /^[0-9a-f]{32}$/);
  assert.match(span.spanId, /^[0-9a-f]{16}$/);
});

test('time fields are decimal strings (proto3 int64 mapping)', () => {
  const w = emit();
  const span = w.resourceSpans[0].scopeSpans[0].spans[0];
  assert.equal(typeof span.startTimeUnixNano, 'string');
  assert.equal(typeof span.endTimeUnixNano, 'string');
  assert.match(span.startTimeUnixNano, /^\d+$/);
  assert.ok(BigInt(span.endTimeUnixNano) >= BigInt(span.startTimeUnixNano));
});

test('kind=3 (SPAN_KIND_CLIENT)', () => {
  const w = emit();
  const span = w.resourceSpans[0].scopeSpans[0].spans[0];
  assert.equal(span.kind, 3);
});

test('gen_ai.* attributes are present with correct types', () => {
  const w = emit({ promptBytes: 4000, responseBytes: 400 });
  const span = w.resourceSpans[0].scopeSpans[0].spans[0];
  const byKey = Object.fromEntries(span.attributes.map((kv) => [kv.key, kv.value]));
  assert.equal(byKey['gen_ai.system'].stringValue, 'anthropic');
  assert.equal(byKey['gen_ai.request.model'].stringValue, 'claude-opus-4-7');
  // 4000 bytes / 4 = 1000 tokens estimate.
  assert.equal(byKey['gen_ai.usage.input_tokens'].intValue, '1000');
  assert.equal(byKey['gen_ai.usage.output_tokens'].intValue, '100');
});

test('mustard.cost_usd matches pricing.ts costUsd()', () => {
  const w = emit({ promptBytes: 4000, responseBytes: 400 });
  const span = w.resourceSpans[0].scopeSpans[0].spans[0];
  const costAttr = span.attributes.find((kv) => kv.key === 'mustard.cost_usd');
  const expected = costUsd('claude-opus-4-7', 1000, 100);
  assert.equal(costAttr.value.doubleValue, expected);
});

test('mustard.* optional attrs only emitted when supplied', () => {
  // Without spec/phase/wave.
  const w1 = emit();
  const keys1 = w1.resourceSpans[0].scopeSpans[0].spans[0].attributes.map((kv) => kv.key);
  assert.ok(!keys1.includes('mustard.spec'));
  assert.ok(!keys1.includes('mustard.phase'));
  assert.ok(!keys1.includes('mustard.wave'));
  // With them.
  const w2 = emit({ spec: 'feat-x', phase: 'PLAN', wave: 1 });
  const keys2 = w2.resourceSpans[0].scopeSpans[0].spans[0].attributes.map((kv) => kv.key);
  assert.ok(keys2.includes('mustard.spec'));
  assert.ok(keys2.includes('mustard.phase'));
  assert.ok(keys2.includes('mustard.wave'));
});
