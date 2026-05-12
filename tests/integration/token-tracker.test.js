#!/usr/bin/env node
/**
 * AC #2, #3, #4 for Mustard 2.0 Phase 2 (OpenTelemetry tokens).
 *
 * Exercises the compiled TokenTracker end-to-end against a temp directory:
 *   1. startSpan writes a sidecar keyed by tool_use_id.
 *   2. endSpan reads sidecar, emits one OTLP/JSON line to spans.jsonl,
 *      removes the sidecar.
 *   3. Emitted line conforms to OTLP v1 JSON shape:
 *        - resourceSpans[].scopeSpans[].spans[]
 *        - traceId = 32 lowercase hex; spanId = 16 lowercase hex
 *        - startTimeUnixNano < endTimeUnixNano (string nanoseconds)
 *      Plus all required `gen_ai.*` and `mustard.*` attributes,
 *      with the right value-type wrapper (stringValue / intValue / doubleValue).
 *   4. cost_usd from the emitted attribute equals pricing.ts costUsd().
 *
 * NOTE on @opentelemetry/otlp-transformer: the package only exposes
 * *serializers* (JsonTraceSerializer.serializeRequest takes a ReadableSpan
 * SDK object — not a raw OTLP JSON blob — and outputs OTLP). It has no
 * public parser/validator for arbitrary OTLP JSON. Validating shape via
 * the transformer would require constructing a fake ReadableSpan, which is
 * a circular check. We instead do strict structural assertions on the
 * emitted JSON; the transformer dep is kept in devDeps as documented in
 * the spec for future hard-validation work (e.g. round-tripping via
 * ProtobufTraceSerializer).
 *
 * Run: node --test tests/integration/token-tracker.test.js
 */

import test from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '..', '..');

// Eager import the compiled ESM module (TokenTracker compiles to dist/).
const trackerMod = await import(
  pathToFileURL(path.join(REPO_ROOT, 'dist', 'telemetry', 'token-tracker.js')).href
);
const pricingMod = await import(
  pathToFileURL(path.join(REPO_ROOT, 'dist', 'telemetry', 'pricing.js')).href
);
const { TokenTracker } = trackerMod;
const { costUsd } = pricingMod;

function tmpDir() {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-token-tracker-'));
  return d;
}

function findAttr(attrs, key) {
  return attrs.find((a) => a && a.key === key);
}

test('startSpan persists sidecar; endSpan emits OTLP JSON and cleans up', () => {
  const dir = tmpDir();
  const spansJsonl = path.join(dir, 'spans.jsonl');
  const tracker = new TokenTracker(spansJsonl);

  const toolUseId = 'toolu_test_001';
  const promptBytes = 1250 * 4; // 1250 tokens worth.
  const ctx = tracker.startSpan({
    name: 'task.dispatch',
    toolUseId,
    model: 'claude-opus-4-7',
    agentType: 'general-purpose',
    spec: '2026-05-12-test-spec',
    phase: 'EXECUTE',
    wave: 1,
    promptBytes,
  });

  // Sidecar should exist after startSpan.
  const sidecarPath = path.join(dir, '.active-spans', `${toolUseId}.json`);
  assert.equal(fs.existsSync(sidecarPath), true, 'sidecar should exist');
  assert.match(ctx.traceId, /^[0-9a-f]{32}$/, 'traceId is 32 lowercase hex');
  assert.match(ctx.spanId, /^[0-9a-f]{16}$/, 'spanId is 16 lowercase hex');

  // Small delay so endTime > startTime measurably.
  const start = Date.now();
  while (Date.now() - start < 3) { /* spin briefly */ }

  const responseBytes = 340 * 4; // 340 output tokens worth.
  tracker.endSpan({ toolUseId, responseBytes });

  // Sidecar cleaned up.
  assert.equal(fs.existsSync(sidecarPath), false, 'sidecar removed after endSpan');

  // spans.jsonl should have one line.
  const raw = fs.readFileSync(spansJsonl, 'utf8').trim();
  const lines = raw.split('\n').filter(Boolean);
  assert.equal(lines.length, 1, 'one line emitted');

  const wrapper = JSON.parse(lines[0]);

  // Structural: resourceSpans → scopeSpans → spans.
  assert.ok(Array.isArray(wrapper.resourceSpans), 'has resourceSpans array');
  assert.equal(wrapper.resourceSpans.length, 1);
  const rs = wrapper.resourceSpans[0];
  assert.ok(rs.resource && Array.isArray(rs.resource.attributes));
  const svcName = findAttr(rs.resource.attributes, 'service.name');
  assert.ok(svcName && svcName.value.stringValue === 'mustard');

  assert.ok(Array.isArray(rs.scopeSpans));
  assert.equal(rs.scopeSpans.length, 1);
  const ss = rs.scopeSpans[0];
  assert.ok(ss.scope && ss.scope.name === 'mustard.telemetry');
  assert.ok(Array.isArray(ss.spans));
  assert.equal(ss.spans.length, 1);
  const span = ss.spans[0];

  // Span fields.
  assert.match(span.traceId, /^[0-9a-f]{32}$/);
  assert.match(span.spanId, /^[0-9a-f]{16}$/);
  assert.equal(span.name, 'task.dispatch');
  assert.equal(span.kind, 3, 'SPAN_KIND_CLIENT');
  assert.equal(typeof span.startTimeUnixNano, 'string');
  assert.equal(typeof span.endTimeUnixNano, 'string');
  assert.ok(
    BigInt(span.endTimeUnixNano) > BigInt(span.startTimeUnixNano),
    'end > start in nanoseconds'
  );
  assert.equal(span.status.code, 1, 'status OK');

  // Attributes: required gen_ai.*.
  const a = span.attributes;
  const gsys = findAttr(a, 'gen_ai.system');
  assert.ok(gsys && gsys.value.stringValue === 'anthropic');
  const gmodel = findAttr(a, 'gen_ai.request.model');
  assert.ok(gmodel && gmodel.value.stringValue === 'claude-opus-4-7');
  const gin = findAttr(a, 'gen_ai.usage.input_tokens');
  assert.ok(gin && typeof gin.value.intValue === 'string');
  assert.equal(gin.value.intValue, '1250');
  const gout = findAttr(a, 'gen_ai.usage.output_tokens');
  assert.ok(gout && gout.value.intValue === '340');
  const gop = findAttr(a, 'gen_ai.operation.name');
  assert.ok(gop && gop.value.stringValue === 'task.dispatch');

  // Attributes: required mustard.*.
  const mspec = findAttr(a, 'mustard.spec');
  assert.ok(mspec && mspec.value.stringValue === '2026-05-12-test-spec');
  const mphase = findAttr(a, 'mustard.phase');
  assert.ok(mphase && mphase.value.stringValue === 'EXECUTE');
  const mwave = findAttr(a, 'mustard.wave');
  assert.ok(mwave && mwave.value.intValue === '1');
  const magent = findAttr(a, 'mustard.agent_type');
  assert.ok(magent && magent.value.stringValue === 'general-purpose');

  // cost_usd matches pricing.ts.
  const mcost = findAttr(a, 'mustard.cost_usd');
  assert.ok(mcost && typeof mcost.value.doubleValue === 'number');
  const expectedCost = costUsd('claude-opus-4-7', 1250, 340);
  assert.equal(mcost.value.doubleValue, expectedCost);
});

test('endSpan with missing sidecar is a no-op (fail-open)', () => {
  const dir = tmpDir();
  const tracker = new TokenTracker(path.join(dir, 'spans.jsonl'));
  // Should not throw. No file should be created.
  tracker.endSpan({ toolUseId: 'orphan_id', responseBytes: 100 });
  assert.equal(
    fs.existsSync(path.join(dir, 'spans.jsonl')),
    false,
    'no jsonl written on orphan endSpan'
  );
});

test('isError=true sets status.code=2 and error.type attribute', () => {
  const dir = tmpDir();
  const tracker = new TokenTracker(path.join(dir, 'spans.jsonl'));
  tracker.startSpan({
    name: 'task.dispatch',
    toolUseId: 'err_001',
    model: 'claude-sonnet-4-6',
    agentType: 'Explore',
    promptBytes: 400,
  });
  tracker.endSpan({
    toolUseId: 'err_001',
    responseBytes: 0,
    isError: true,
    errorType: 'api_overload',
  });
  const wrapper = JSON.parse(
    fs.readFileSync(path.join(dir, 'spans.jsonl'), 'utf8').trim()
  );
  const span = wrapper.resourceSpans[0].scopeSpans[0].spans[0];
  assert.equal(span.status.code, 2, 'status ERROR');
  const errAttr = findAttr(span.attributes, 'error.type');
  assert.ok(errAttr && errAttr.value.stringValue === 'api_overload');
});
