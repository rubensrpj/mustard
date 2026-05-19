#!/usr/bin/env node
/**
 * Cross-runtime tests for runtime-shim. Runs under:
 *   node templates/hooks/_lib/__tests__/runtime-shim.test.js
 *   bun  templates/hooks/_lib/__tests__/runtime-shim.test.js
 *
 * NOTE: avoids `node:test` because `bun <file>` rejects it ("Cannot use test
 * outside of the test runner"). Uses a tiny inline harness with
 * `node:assert/strict`, which Bun supports.
 */
'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');

const SHIM_PATH = path.resolve(__dirname, '..', 'runtime-shim.js');

let passed = 0;
let failed = 0;
const failures = [];

function test(name, fn) {
  try {
    fn();
    passed++;
    process.stdout.write('  ok  ' + name + '\n');
  } catch (e) {
    failed++;
    failures.push({ name: name, err: e });
    process.stdout.write('  FAIL ' + name + '\n');
  }
}

function freshShim(envPatch) {
  // Clear require cache + reset memoization so each test re-detects.
  delete require.cache[SHIM_PATH];
  const saved = {};
  if (envPatch) {
    for (const k of Object.keys(envPatch)) {
      saved[k] = process.env[k];
      if (envPatch[k] === undefined) delete process.env[k];
      else process.env[k] = envPatch[k];
    }
  }
  const shim = require(SHIM_PATH);
  shim._resetCache();
  return { shim: shim, restore: function () {
    for (const k of Object.keys(saved)) {
      if (saved[k] === undefined) delete process.env[k];
      else process.env[k] = saved[k];
    }
    delete require.cache[SHIM_PATH];
  } };
}

// ── 1. pickRuntime() returns correct shape ─────────────────────────────────
test('pickRuntime returns { kind, version, bunSqliteAvailable }', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: undefined });
  try {
    const r = shim.pickRuntime();
    assert.ok(r && typeof r === 'object', 'returns object');
    assert.ok(r.kind === 'bun' || r.kind === 'node', 'kind is bun|node');
    assert.equal(typeof r.version, 'string');
    assert.equal(typeof r.bunSqliteAvailable, 'boolean');
  } finally { restore(); }
});

// ── 2. isBun / isNode are mutually exclusive ───────────────────────────────
test('isBun and isNode are mutually exclusive', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: undefined });
  try {
    const b = shim.isBun();
    const n = shim.isNode();
    assert.equal(typeof b, 'boolean');
    assert.equal(typeof n, 'boolean');
    assert.notEqual(b, n, 'exactly one of isBun/isNode must be true');
  } finally { restore(); }
});

// ── 3. MUSTARD_RUNTIME=node forces node identity ───────────────────────────
test('MUSTARD_RUNTIME=node forces kind=node', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: 'node' });
  try {
    const r = shim.pickRuntime();
    assert.equal(r.kind, 'node');
    assert.equal(shim.isNode(), true);
    assert.equal(shim.isBun(), false);
  } finally { restore(); }
});

// ── 4. MUSTARD_RUNTIME=bun forces bun identity ─────────────────────────────
test('MUSTARD_RUNTIME=bun forces kind=bun', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: 'bun' });
  try {
    const r = shim.pickRuntime();
    assert.equal(r.kind, 'bun');
    assert.equal(shim.isBun(), true);
    assert.equal(shim.isNode(), false);
  } finally { restore(); }
});

// ── 5. Invalid MUSTARD_RUNTIME falls back to auto-detect ───────────────────
test('invalid MUSTARD_RUNTIME does not crash and auto-detects', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: 'banana' });
  try {
    const r = shim.pickRuntime();
    assert.ok(r.kind === 'bun' || r.kind === 'node');
  } finally { restore(); }
});

// ── 6. pickRuntime is memoized ─────────────────────────────────────────────
test('pickRuntime returns same reference on repeated calls', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: undefined });
  try {
    const a = shim.pickRuntime();
    const b = shim.pickRuntime();
    assert.strictEqual(a, b, 'memoized result should be referentially equal');
  } finally { restore(); }
});

// ── 7. loadSqlite() never throws — returns Database ctor or null ───────────
test('loadSqlite returns Database constructor under Bun, null otherwise', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: undefined });
  try {
    let result;
    assert.doesNotThrow(() => { result = shim.loadSqlite(); });
    if (shim.isBun()) {
      assert.ok(result, 'under Bun, loadSqlite must return non-null');
    } else {
      assert.equal(result, null, 'under Node Phase 0, loadSqlite must return null');
    }
  } finally { restore(); }
});

// ── 8. version is a non-empty string ───────────────────────────────────────
test('pickRuntime version is non-empty string', () => {
  const { shim, restore } = freshShim({ MUSTARD_RUNTIME: undefined });
  try {
    const r = shim.pickRuntime();
    assert.ok(r.version.length > 0, 'version should be non-empty');
  } finally { restore(); }
});

// ── Summary ─────────────────────────────────────────────────────────────────
process.stdout.write('\n' + passed + ' passed, ' + failed + ' failed\n');
if (failed > 0) {
  for (const f of failures) {
    process.stderr.write('\nFAIL: ' + f.name + '\n');
    process.stderr.write((f.err && f.err.stack) ? f.err.stack + '\n' : String(f.err) + '\n');
  }
  process.exit(1);
}
process.exit(0);
