#!/usr/bin/env node
'use strict';

/**
 * costUsd — pricing table + computation.
 *
 * Run: node --test tests/unit/token-tracker/pricing.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');

const { costUsd, PRICING } = require('../../../dist/telemetry/pricing.js');

test('PRICING table exposes known models', () => {
  for (const m of ['claude-opus-4-7', 'claude-sonnet-4-6', 'claude-haiku-4-5']) {
    assert.ok(PRICING[m], `missing ${m}`);
    assert.ok(PRICING[m].input > 0);
    assert.ok(PRICING[m].output > 0);
  }
});

test('costUsd computes per-MTok rate correctly', () => {
  // 1M input + 1M output on opus-4-7 at $15/$75 = $90.
  const c = costUsd('claude-opus-4-7', 1_000_000, 1_000_000);
  assert.equal(c, 90);
});

test('costUsd scales linearly with tokens', () => {
  const c1 = costUsd('claude-sonnet-4-6', 1000, 0);
  const c2 = costUsd('claude-sonnet-4-6', 2000, 0);
  assert.ok(Math.abs(c2 - c1 * 2) < 1e-9, `expected linear: ${c1} vs ${c2}`);
});

test('costUsd returns 0 for unknown model', () => {
  assert.equal(costUsd('unknown-model', 1000, 1000), 0);
});

test('costUsd returns 0 for zero tokens', () => {
  assert.equal(costUsd('claude-opus-4-7', 0, 0), 0);
});

test('costUsd handles output-only tokens', () => {
  const c = costUsd('claude-haiku-4-5', 0, 1_000_000);
  // haiku-4-5 output = $5/MTok
  assert.equal(c, 5);
});
