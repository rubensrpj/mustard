#!/usr/bin/env bun
'use strict';

/**
 * jsonl-to-sqlite — spans.jsonl → spans table projection.
 *
 * Run: bun test tests/unit/migrate/spans-projection.test.cjs
 */

const { test } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { migrate } = require('../../../dist/migrate/jsonl-to-sqlite.js');
const { EventStore } = require('../../../dist/runtime/event-store.js');

function makeOtlpSpan(opts = {}) {
  const startMs = opts.startMs ?? 1_700_000_000_000;
  const endMs = opts.endMs ?? startMs + 1000;
  return {
    resourceSpans: [
      {
        resource: {
          attributes: [
            { key: 'service.name', value: { stringValue: 'mustard' } },
          ],
        },
        scopeSpans: [
          {
            scope: { name: 'mustard.telemetry' },
            spans: [
              {
                traceId: opts.traceId ?? 'a'.repeat(32),
                spanId: opts.spanId ?? 'b'.repeat(16),
                parentSpanId: opts.parentSpanId,
                kind: 3,
                name: opts.name ?? 'task.dispatch',
                startTimeUnixNano: String(BigInt(startMs) * 1_000_000n),
                endTimeUnixNano: String(BigInt(endMs) * 1_000_000n),
                attributes: [
                  { key: 'gen_ai.system', value: { stringValue: 'anthropic' } },
                  { key: 'gen_ai.request.model', value: { stringValue: opts.model ?? 'claude-opus-4-7' } },
                  { key: 'gen_ai.usage.input_tokens', value: { intValue: String(opts.inputTokens ?? 100) } },
                  { key: 'gen_ai.usage.output_tokens', value: { intValue: String(opts.outputTokens ?? 200) } },
                  { key: 'mustard.spec', value: { stringValue: opts.spec ?? 'feat-x' } },
                  { key: 'mustard.phase', value: { stringValue: opts.phase ?? 'EXECUTE' } },
                ],
                status: { code: opts.isError ? 2 : 1 },
              },
            ],
          },
        ],
      },
    ],
  };
}

function mkFixture(spans) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-spans-'));
  const harness = path.join(root, '.claude', '.harness');
  fs.mkdirSync(harness, { recursive: true });
  fs.writeFileSync(
    path.join(harness, 'spans.jsonl'),
    spans.map((s) => JSON.stringify(s)).join('\n') + '\n',
    'utf8'
  );
  return { root, harness };
}

test('spans.jsonl migrates to spans table', () => {
  const { root, harness } = mkFixture([
    makeOtlpSpan({ spanId: '1'.repeat(16) }),
    makeOtlpSpan({ spanId: '2'.repeat(16), name: 'task.b' }),
  ]);
  try {
    const r = migrate(harness);
    assert.equal(r.spansImported, 2);
    const store = new EventStore(path.join(harness, 'mustard.db'));
    store.init();
    const spans = store.spans();
    assert.equal(spans.length, 2);
    store.close();
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('spans migration extracts gen_ai + mustard attributes', () => {
  const { root, harness } = mkFixture([
    makeOtlpSpan({
      spanId: '1'.repeat(16),
      model: 'claude-sonnet-4-6',
      inputTokens: 500,
      outputTokens: 300,
      spec: 'auth-feature',
      phase: 'PLAN',
    }),
  ]);
  try {
    migrate(harness);
    const store = new EventStore(path.join(harness, 'mustard.db'));
    store.init();
    const [s] = store.spans();
    assert.equal(s.model, 'claude-sonnet-4-6');
    assert.equal(s.inputTokens, 500);
    assert.equal(s.outputTokens, 300);
    assert.equal(s.spec, 'auth-feature');
    assert.equal(s.phase, 'PLAN');
    store.close();
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('spans migration computes durationMs', () => {
  const { root, harness } = mkFixture([
    makeOtlpSpan({
      spanId: '1'.repeat(16),
      startMs: 1_700_000_000_000,
      endMs: 1_700_000_000_500,
    }),
  ]);
  try {
    migrate(harness);
    const store = new EventStore(path.join(harness, 'mustard.db'));
    store.init();
    const [s] = store.spans();
    assert.equal(s.durationMs, 500);
    store.close();
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('spans migration flags isError when status.code=2', () => {
  const { root, harness } = mkFixture([
    makeOtlpSpan({ spanId: '1'.repeat(16), isError: true }),
  ]);
  try {
    migrate(harness);
    const store = new EventStore(path.join(harness, 'mustard.db'));
    store.init();
    const [s] = store.spans();
    assert.equal(s.isError, true);
    store.close();
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('spans migration is idempotent (spanId PK)', () => {
  const { root, harness } = mkFixture([
    makeOtlpSpan({ spanId: '1'.repeat(16) }),
  ]);
  try {
    const r1 = migrate(harness);
    const r2 = migrate(harness);
    assert.equal(r1.spansImported, 1);
    assert.equal(r2.spansImported, 0);
    assert.equal(r2.spansSkipped, 1);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('malformed spans lines counted as skipped', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-spans-bad-'));
  const harness = path.join(root, '.claude', '.harness');
  fs.mkdirSync(harness, { recursive: true });
  fs.writeFileSync(
    path.join(harness, 'spans.jsonl'),
    JSON.stringify(makeOtlpSpan({ spanId: '1'.repeat(16) })) +
      '\n{"resourceSpans":[]}\n' +
      'not-json\n',
    'utf8'
  );
  try {
    const r = migrate(harness);
    assert.equal(r.spansImported, 1);
    assert.equal(r.spansSkipped, 2);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});

test('missing spans.jsonl is silent', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-ut-mig-spans-empty-'));
  const harness = path.join(root, '.claude', '.harness');
  fs.mkdirSync(harness, { recursive: true });
  try {
    const r = migrate(harness);
    assert.equal(r.spansImported, 0);
    assert.equal(r.spansSkipped, 0);
  } finally {
    try { fs.rmSync(root, { recursive: true, force: true }); } catch {}
  }
});
