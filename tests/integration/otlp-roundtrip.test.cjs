#!/usr/bin/env node
'use strict';

/**
 * OTLP round-trip validation — Phase 4 Wave 1.
 *
 * Goal: validate Mustard's hand-rolled OTLP/JSON spans are structurally
 * sound by attempting to round-trip them through
 * `@opentelemetry/otlp-transformer`'s `ProtobufTraceSerializer`.
 *
 * Outcome (Wave 1): the transformer's `serializeRequest` API accepts
 * `ReadableSpan[]` from `@opentelemetry/sdk-trace-base`, NOT raw OTLP/JSON
 * resourceSpans wrappers. Mustard emits OTLP/JSON directly without the SDK
 * (zero-deps hook contract — adding sdk-trace-base would force bundling
 * the OTel runtime into every hook). A clean round-trip is therefore
 * impossible without abandoning the zero-deps invariant.
 *
 * Decision: remove the `@opentelemetry/otlp-transformer` devDep. Span shape
 * is covered by structural assertions in
 * `tests/integration/token-tracker.test.js` and
 * `tests/integration/subagent-tracker-spans.test.js`. This file remains as
 * documentation of the investigation so future contributors don't re-add
 * the dep without reading the rationale.
 *
 * Run: node tests/integration/otlp-roundtrip.test.cjs
 */

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert');
const test = require('node:test');

// Resolve TokenTracker via the compiled output (matches what hooks load).
const { TokenTracker } = require('../../dist/telemetry/token-tracker.js');

test('OTLP/JSON spans emitted by TokenTracker are structurally valid', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-otlp-rt-'));
  const spansPath = path.join(tmp, 'spans.jsonl');
  try {
    const tracker = new TokenTracker(spansPath);
    // Generate 3 spans (start+end pairs) to exercise the emit path.
    for (let i = 0; i < 3; i += 1) {
      const toolUseId = `tu-${i}-${Date.now()}`;
      tracker.startSpan({
        name: 'task.dispatch',
        toolUseId,
        model: 'claude-opus-4-7',
        agentType: 'general-purpose',
        spec: 'roundtrip-test',
        phase: 'EXECUTE',
        wave: 1,
        promptBytes: 1024 * (i + 1),
      });
      tracker.endSpan({ toolUseId, responseBytes: 512 * (i + 1) });
    }

    const lines = fs
      .readFileSync(spansPath, 'utf8')
      .split('\n')
      .filter((l) => l.trim());
    assert.strictEqual(lines.length, 3, 'expected 3 spans emitted');

    // Structural OTLP/JSON contract assertions. The transformer would
    // accept a `ReadableSpan[]` (SDK), not these JSON wrappers — see file
    // header for context.
    for (const line of lines) {
      const wrapper = JSON.parse(line);
      assert.ok(Array.isArray(wrapper.resourceSpans), 'resourceSpans is array');
      const rs = wrapper.resourceSpans[0];
      assert.ok(Array.isArray(rs.scopeSpans), 'scopeSpans is array');
      const ss = rs.scopeSpans[0];
      assert.ok(Array.isArray(ss.spans) && ss.spans.length === 1, '1 span per line');
      const span = ss.spans[0];
      assert.match(span.traceId, /^[0-9a-f]{32}$/, 'traceId 32 hex chars');
      assert.match(span.spanId, /^[0-9a-f]{16}$/, 'spanId 16 hex chars');
      assert.ok(typeof span.startTimeUnixNano === 'string', 'startNs is decimal string');
      assert.ok(typeof span.endTimeUnixNano === 'string', 'endNs is decimal string');
      assert.ok(BigInt(span.endTimeUnixNano) >= BigInt(span.startTimeUnixNano));
      assert.ok(Array.isArray(span.attributes));
      const attrKeys = span.attributes.map((kv) => kv.key);
      assert.ok(attrKeys.includes('gen_ai.system'));
      assert.ok(attrKeys.includes('gen_ai.request.model'));
      assert.ok(attrKeys.includes('gen_ai.usage.input_tokens'));
      assert.ok(attrKeys.includes('gen_ai.usage.output_tokens'));
    }

    // Round-trip attempt: only run if the dep is somehow still installed
    // (CI envs may cache node_modules). The dep was removed from
    // devDependencies in Phase 4 Wave 1; this branch documents the failure
    // mode for posterity.
    let transformer = null;
    try {
      transformer = require('@opentelemetry/otlp-transformer');
    } catch {
      // Expected path after Wave 1: dep removed.
    }
    if (transformer && transformer.ProtobufTraceSerializer) {
      const wrapper = JSON.parse(lines[0]);
      assert.throws(
        () => transformer.ProtobufTraceSerializer.serializeRequest(wrapper),
        // API mismatch: serializer expects ReadableSpan[] from
        // @opentelemetry/sdk-trace-base. Feeding OTLP JSON throws because
        // the internal createResourceMap iterates `.spans` on a SDK span.
        /not iterable|readableSpan|toJsonEncoder|getAttributes|spanContext|undefined/i,
        'documents API mismatch: serializer expects ReadableSpan[], not OTLP JSON wrapper'
      );
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
