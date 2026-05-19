#!/usr/bin/env bun
/**
 * Tests for scripts/wave-tree.js.
 * Run with: bun test templates/hooks/__tests__/wave-tree.test.js
 */

const { describe, it } = require('bun:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const SCRIPT = path.resolve(__dirname, '..', '..', 'scripts', 'wave-tree.js');

function run(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' });
}

function mkSpecDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'wave-tree-'));
}

function writeSpec(dir, status) {
  fs.writeFileSync(
    path.join(dir, 'spec.md'),
    `# Wave\n### Status: ${status} | Phase: EXECUTE | Scope: full\n\nbody\n`,
  );
}

describe('wave-tree', () => {
  it('renders ascii tree for wave-plan with 3 waves', () => {
    const root = mkSpecDir();
    const wavePlan = [
      '| Wave | Role | Status | Pasta |',
      '|------|------|--------|-------|',
      '| 1 | backend | queued | wave-1-foo |',
      '| 2 | frontend | queued | wave-2-bar |',
      '| 3 | billing | queued | wave-3-baz |',
    ].join('\n');
    fs.writeFileSync(path.join(root, 'wave-plan.md'), wavePlan);
    for (const [folder, status] of [
      ['wave-1-foo', 'completed'],
      ['wave-2-bar', 'implementing'],
      ['wave-3-baz', 'draft'],
    ]) {
      fs.mkdirSync(path.join(root, folder));
      writeSpec(path.join(root, folder), status);
    }
    const r = run(['--spec-dir', root]);
    assert.equal(r.status, 0);
    assert.ok(r.stdout.includes('Roadmap:'));
    assert.ok(r.stdout.includes('[v]'));
    assert.ok(r.stdout.includes('[>]'));
    assert.ok(r.stdout.includes('[ ]'));
    assert.ok(r.stdout.includes('wave-1-foo'));
    assert.ok(r.stdout.includes('wave-2-bar'));
    assert.ok(r.stdout.includes('wave-3-baz'));
  });

  it('single-spec without wave-plan renders Spec: line', () => {
    const root = mkSpecDir();
    writeSpec(root, 'completed');
    const r = run(['--spec-dir', root]);
    assert.equal(r.status, 0);
    assert.ok(r.stdout.startsWith('Spec:'));
    assert.ok(r.stdout.includes('[v]'));
  });

  it('non-existent dir → exit 0 + (no spec', () => {
    const nope = path.join(os.tmpdir(), 'wave-tree-does-not-exist-' + Date.now());
    const r = run(['--spec-dir', nope]);
    assert.equal(r.status, 0);
    assert.ok(r.stdout.includes('(no spec'));
  });

  it('--format json returns valid wave-plan shape', () => {
    const root = mkSpecDir();
    const wavePlan = [
      '| Wave | Role | Status | Pasta |',
      '|------|------|--------|-------|',
      '| 1 | backend | queued | wave-1-foo |',
      '| 2 | frontend | queued | wave-2-bar |',
      '| 3 | billing | queued | wave-3-baz |',
    ].join('\n');
    fs.writeFileSync(path.join(root, 'wave-plan.md'), wavePlan);
    for (const [folder, status] of [
      ['wave-1-foo', 'completed'],
      ['wave-2-bar', 'implementing'],
      ['wave-3-baz', 'draft'],
    ]) {
      fs.mkdirSync(path.join(root, folder));
      writeSpec(path.join(root, folder), status);
    }
    const r = run(['--spec-dir', root, '--format', 'json']);
    assert.equal(r.status, 0);
    const parsed = JSON.parse(r.stdout.trim());
    assert.equal(parsed.kind, 'wave-plan');
    assert.ok(Array.isArray(parsed.waves));
    assert.equal(parsed.waves.length, 3);
  });
});
