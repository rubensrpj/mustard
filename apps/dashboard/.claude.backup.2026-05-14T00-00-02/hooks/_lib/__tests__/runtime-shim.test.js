#!/usr/bin/env bun
/**
 * Bun-only tests for runtime-shim (Mustard 2.0+).
 *
 * Runs under:
 *   bun  templates/hooks/_lib/__tests__/runtime-shim.test.js
 *
 * Uses an inline harness with `node:assert/strict` (Bun-compatible).
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

function freshShim() {
  delete require.cache[SHIM_PATH];
  const shim = require(SHIM_PATH);
  shim._resetCache();
  return shim;
}

test('pickRuntime returns { kind: bun, version, bunSqliteAvailable: true }', () => {
  const shim = freshShim();
  const r = shim.pickRuntime();
  assert.ok(r && typeof r === 'object', 'returns object');
  assert.equal(r.kind, 'bun');
  assert.equal(typeof r.version, 'string');
  assert.equal(r.bunSqliteAvailable, true);
});

test('isBun() returns true', () => {
  const shim = freshShim();
  assert.equal(shim.isBun(), true);
});

test('pickRuntime is memoized', () => {
  const shim = freshShim();
  const a = shim.pickRuntime();
  const b = shim.pickRuntime();
  assert.strictEqual(a, b, 'memoized result should be referentially equal');
});

test('loadSqlite returns Database constructor', () => {
  const shim = freshShim();
  let result;
  assert.doesNotThrow(() => { result = shim.loadSqlite(); });
  assert.ok(result, 'loadSqlite must return non-null under Bun');
});

test('pickRuntime version is non-empty', () => {
  const shim = freshShim();
  const r = shim.pickRuntime();
  assert.ok(r.version.length > 0, 'version should be non-empty');
});

process.stdout.write('\n' + passed + ' passed, ' + failed + ' failed\n');
if (failed > 0) {
  for (const f of failures) {
    process.stderr.write('\nFAIL: ' + f.name + '\n');
    process.stderr.write((f.err && f.err.stack) ? f.err.stack + '\n' : String(f.err) + '\n');
  }
  process.exit(1);
}
process.exit(0);
