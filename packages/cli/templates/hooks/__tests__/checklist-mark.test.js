#!/usr/bin/env bun
'use strict';
/**
 * Tests for mark-checklist-item.js.
 *
 * The close-gate checklist-consistency gate and the checklist-auto-mark hook
 * were ported to the Rust `close_gate` / `post_edit` modules (b3 Wave 4);
 * their parity tests now live in `packages/rt/src/hooks/{close_gate,post_edit}.rs`.
 *
 * Run with: bun test templates/hooks/__tests__/checklist-mark.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const MARK_SCRIPT = path.join(SCRIPTS_DIR, 'mark-checklist-item.js');

function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-cl-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.pipeline-states'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', 'spec', 'active'), { recursive: true });
  return dir;
}

function writeSpec(projectDir, specName, content) {
  const dir = path.join(projectDir, '.claude', 'spec', 'active', specName);
  fs.mkdirSync(dir, { recursive: true });
  const specFile = path.join(dir, 'spec.md');
  fs.writeFileSync(specFile, content, 'utf8');
  return specFile;
}

function runNode(file, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const env = Object.assign({}, process.env, { MUSTARD_DISABLED_HOOKS: 'all' });
    if (opts.env) Object.assign(env, opts.env);
    const child = spawn(process.execPath, [file, ...args], {
      cwd: opts.cwd || os.tmpdir(),
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', d => (stdout += d));
    child.stderr.on('data', d => (stderr += d));
    child.on('error', reject);
    child.on('close', code => resolve({ code, stdout: stdout.trim(), stderr: stderr.trim() }));
    if (opts.stdin) child.stdin.write(opts.stdin);
    child.stdin.end();
  });
}

const SPEC_WITH_OPEN = [
  '# Test Spec',
  '',
  '### Status: implementing | Phase: EXECUTE',
  '',
  '## Checklist',
  '',
  '- [x] first item done',
  '- [ ] second item still open',
  '- [ ] third item also open',
  '',
  '## Notes',
  '',
  'arbitrary',
  '',
].join('\n');

const SPEC_NO_CHECKLIST = [
  '# Test Spec',
  '',
  '## Summary',
  '',
  'no checklist here',
  '',
].join('\n');

// ── mark-checklist-item.js ────────────────────────────────────────────────────

describe('mark-checklist-item — by --item substring', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('marks a [ ] item matched by substring', async () => {
    const specFile = writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    const r = await runNode(MARK_SCRIPT, ['--spec', 'demo', '--item', 'second item', '--cwd', tmp]);
    assert.equal(r.code, 0, `stderr: ${r.stderr}`);
    assert.equal(r.stdout, 'marked');
    const out = fs.readFileSync(specFile, 'utf8');
    assert.match(out, /- \[x\] second item still open/);
    assert.match(out, /- \[ \] third item also open/, 'third should still be unmarked');
  });

  it('returns already-marked when item is already [x]', async () => {
    writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    const r = await runNode(MARK_SCRIPT, ['--spec', 'demo', '--item', 'first item', '--cwd', tmp]);
    assert.equal(r.code, 0);
    assert.equal(r.stdout, 'already-marked');
  });

  it('errors when no item matches', async () => {
    writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    const r = await runNode(MARK_SCRIPT, ['--spec', 'demo', '--item', 'nonexistent zzz', '--cwd', tmp]);
    assert.equal(r.code, 1);
    assert.match(r.stdout, /^error:/);
  });

  it('errors when spec does not exist', async () => {
    const r = await runNode(MARK_SCRIPT, ['--spec', 'ghost', '--item', 'whatever', '--cwd', tmp]);
    assert.equal(r.code, 1);
    assert.match(r.stdout, /spec not found/);
  });

  it('errors when spec has no Checklist section', async () => {
    writeSpec(tmp, 'demo', SPEC_NO_CHECKLIST);
    const r = await runNode(MARK_SCRIPT, ['--spec', 'demo', '--item', 'anything', '--cwd', tmp]);
    assert.equal(r.code, 1);
    assert.match(r.stdout, /no `## Checklist`/);
  });

  it('errors when --spec is missing', async () => {
    const r = await runNode(MARK_SCRIPT, ['--item', 'something', '--cwd', tmp]);
    assert.equal(r.code, 2);
  });
});

describe('mark-checklist-item — by --line', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('marks the checkbox at the given 1-based line', async () => {
    const specFile = writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    // SPEC_WITH_OPEN: line 8 is `- [ ] second item still open` (1-based)
    const r = await runNode(MARK_SCRIPT, ['--spec', 'demo', '--line', '8', '--cwd', tmp]);
    assert.equal(r.code, 0, `stderr: ${r.stderr}, stdout: ${r.stdout}`);
    assert.equal(r.stdout, 'marked');
    const out = fs.readFileSync(specFile, 'utf8');
    assert.match(out, /- \[x\] second item still open/);
  });

  it('errors when --line points outside the Checklist section', async () => {
    writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    const r = await runNode(MARK_SCRIPT, ['--spec', 'demo', '--line', '1', '--cwd', tmp]);
    assert.equal(r.code, 1);
    assert.match(r.stdout, /outside the Checklist section/);
  });
});
