#!/usr/bin/env node
'use strict';
/**
 * Tests for mark-checklist-item.js + close-gate.js checklist consistency gate.
 *
 * Run with: node --test templates/hooks/__tests__/checklist-mark.test.js
 */

const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const CLOSE_GATE = path.join(HOOKS_DIR, 'close-gate.js');
const AUTO_MARK_HOOK = path.join(HOOKS_DIR, 'checklist-auto-mark.js');
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

function runHook(input, opts = {}) {
  return new Promise((resolve, reject) => {
    const env = Object.assign({}, process.env);
    if (opts.env) Object.assign(env, opts.env);
    const child = spawn(process.execPath, [CLOSE_GATE], {
      cwd: opts.cwd || os.tmpdir(),
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', d => (stdout += d));
    child.stderr.on('data', d => (stderr += d));
    child.on('error', reject);
    child.on('close', code => {
      let parsed = null;
      try { parsed = JSON.parse(stdout.trim()); } catch (_) {}
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });
    child.stdin.write(JSON.stringify(input));
    child.stdin.end();
  });
}

function makeCloseInput(projectDir, specName) {
  return {
    tool: 'Write',
    tool_input: {
      file_path: path.join(projectDir, '.claude', '.pipeline-states', specName + '.json'),
      content: JSON.stringify({ spec: specName, phase: 'CLOSE' }),
    },
    cwd: projectDir,
  };
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

const SPEC_ALL_DONE = [
  '# Test Spec',
  '',
  '### Status: implementing | Phase: EXECUTE',
  '',
  '## Checklist',
  '',
  '- [x] first',
  '- [x] second',
  '',
  '## Notes',
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

// ── close-gate checklist consistency gate ─────────────────────────────────────

describe('close-gate — checklist consistency gate', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  // Disable QA gate so we isolate the new checklist behavior.
  const ENV_NO_QA = { MUSTARD_QA_GATE_MODE: 'off' };

  it('strict (default): denies CLOSE when Checklist has unmarked items', async () => {
    writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    const r = await runHook(makeCloseInput(tmp, 'demo'), { cwd: tmp, env: ENV_NO_QA });
    assert.equal(r.code, 0);
    assert.ok(r.parsed, `expected JSON, got: ${r.stdout}`);
    assert.equal(r.parsed.hookSpecificOutput.permissionDecision, 'deny');
    assert.match(r.parsed.hookSpecificOutput.permissionDecisionReason, /Checklist has 2 unmarked/);
  });

  it('strict: passes through when Checklist is fully [x]', async () => {
    writeSpec(tmp, 'demo', SPEC_ALL_DONE);
    // No mustard.json → close-gate skips build/test stage and exits 0
    const r = await runHook(makeCloseInput(tmp, 'demo'), { cwd: tmp, env: ENV_NO_QA });
    assert.equal(r.code, 0);
    assert.equal(r.stdout, '', `expected no deny output, got: ${r.stdout}`);
  });

  it('warn mode: emits stderr but does not deny', async () => {
    writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    const r = await runHook(makeCloseInput(tmp, 'demo'), {
      cwd: tmp,
      env: { ...ENV_NO_QA, MUSTARD_CHECKLIST_GATE_MODE: 'warn' },
    });
    assert.equal(r.code, 0);
    assert.equal(r.stdout, '', 'warn must not produce deny output');
    assert.match(r.stderr, /unmarked item/);
  });

  it('off mode: skips checklist gate entirely', async () => {
    writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    const r = await runHook(makeCloseInput(tmp, 'demo'), {
      cwd: tmp,
      env: { ...ENV_NO_QA, MUSTARD_CHECKLIST_GATE_MODE: 'off' },
    });
    assert.equal(r.code, 0);
    assert.equal(r.stdout, '', 'off must not produce any deny output');
    // close-gate may still log unrelated stderr (e.g. missing mustard.json) —
    // the contract here is only that the checklist warning is absent.
    assert.ok(!/unmarked item/.test(r.stderr), `off must not warn about checklist, stderr: ${r.stderr}`);
  });

  it('skips when spec has no Checklist section', async () => {
    writeSpec(tmp, 'demo', SPEC_NO_CHECKLIST);
    const r = await runHook(makeCloseInput(tmp, 'demo'), { cwd: tmp, env: ENV_NO_QA });
    assert.equal(r.code, 0);
    assert.equal(r.stdout, '', 'should pass through silently');
  });
});

// ── checklist-auto-mark.js (PostToolUse:Edit|Write) ──────────────────────────

function writePipelineState(projectDir, specName) {
  const fp = path.join(projectDir, '.claude', '.pipeline-states', specName + '.json');
  fs.writeFileSync(fp, JSON.stringify({ spec: specName, phase: 'EXECUTE' }), 'utf8');
}

function runAutoMark(input, opts = {}) {
  return new Promise((resolve, reject) => {
    const env = Object.assign({}, process.env);
    if (opts.env) Object.assign(env, opts.env);
    const child = spawn(process.execPath, [AUTO_MARK_HOOK], {
      cwd: opts.cwd || os.tmpdir(),
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', d => (stdout += d));
    child.stderr.on('data', d => (stderr += d));
    child.on('error', reject);
    child.on('close', code => resolve({ code, stdout: stdout.trim(), stderr: stderr.trim() }));
    child.stdin.write(JSON.stringify(input));
    child.stdin.end();
  });
}

function makeEditInput(projectDir, filePath) {
  return {
    tool_name: 'Edit',
    tool_input: { file_path: filePath, new_string: 'whatever' },
    cwd: projectDir,
  };
}

describe('checklist-auto-mark — basename pista', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('marks an item that mentions the basename when that file is edited', async () => {
    const specSrc = [
      '# Spec', '',
      '## Checklist', '',
      '- [ ] Update UserService.cs to add validation',
      '- [ ] Write docs', // no pista
      '',
    ].join('\n');
    const specPath = writeSpec(tmp, 'demo', specSrc);
    writePipelineState(tmp, 'demo');

    const editedFile = path.join(tmp, 'src', 'Services', 'UserService.cs');
    const r = await runAutoMark(makeEditInput(tmp, editedFile), { cwd: tmp });
    assert.equal(r.code, 0);
    assert.equal(r.stdout, '', 'auto-mark must be silent on stdout');

    const updated = fs.readFileSync(specPath, 'utf8');
    assert.match(updated, /- \[x\] Update UserService\.cs/);
    assert.match(updated, /- \[ \] Write docs/, 'item without pista must remain unmarked');
  });

  it('does not mark when no item mentions the file', async () => {
    const specSrc = [
      '# Spec', '',
      '## Checklist', '',
      '- [ ] Refactor OtherFile.ts',
      '',
    ].join('\n');
    const specPath = writeSpec(tmp, 'demo', specSrc);
    writePipelineState(tmp, 'demo');

    const editedFile = path.join(tmp, 'src', 'Unrelated.cs');
    const r = await runAutoMark(makeEditInput(tmp, editedFile), { cwd: tmp });
    assert.equal(r.code, 0);
    const updated = fs.readFileSync(specPath, 'utf8');
    assert.match(updated, /- \[ \] Refactor OtherFile\.ts/);
  });
});

describe('checklist-auto-mark — arrow target', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('marks an item with an explicit `→ path` target', async () => {
    const specSrc = [
      '# Spec', '',
      '## Checklist', '',
      '- [ ] Add validation → src/Services/UserService.cs',
      '',
    ].join('\n');
    const specPath = writeSpec(tmp, 'demo', specSrc);
    writePipelineState(tmp, 'demo');

    const editedFile = path.join(tmp, 'src', 'Services', 'UserService.cs');
    const r = await runAutoMark(makeEditInput(tmp, editedFile), { cwd: tmp });
    assert.equal(r.code, 0);
    const updated = fs.readFileSync(specPath, 'utf8');
    assert.match(updated, /- \[x\] Add validation → src\/Services\/UserService\.cs/);
  });

  it('marks an item with arrow target by basename match', async () => {
    const specSrc = [
      '# Spec', '',
      '## Checklist', '',
      '- [ ] Refactor → UserService.cs',
      '',
    ].join('\n');
    const specPath = writeSpec(tmp, 'demo', specSrc);
    writePipelineState(tmp, 'demo');

    const editedFile = path.join(tmp, 'deep', 'nested', 'UserService.cs');
    const r = await runAutoMark(makeEditInput(tmp, editedFile), { cwd: tmp });
    assert.equal(r.code, 0);
    const updated = fs.readFileSync(specPath, 'utf8');
    assert.match(updated, /- \[x\] Refactor → UserService\.cs/);
  });
});

describe('checklist-auto-mark — safety', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('does not loop when the spec.md itself is being edited', async () => {
    const specSrc = [
      '# Spec', '',
      '## Checklist', '',
      '- [ ] Edit spec.md notes',
      '',
    ].join('\n');
    const specPath = writeSpec(tmp, 'demo', specSrc);
    writePipelineState(tmp, 'demo');

    const r = await runAutoMark(makeEditInput(tmp, specPath), { cwd: tmp });
    assert.equal(r.code, 0);
    const updated = fs.readFileSync(specPath, 'utf8');
    assert.match(updated, /- \[ \] Edit spec\.md notes/, 'must not auto-mark when editing the spec itself');
  });

  it('exits silently when there is no active spec', async () => {
    const editedFile = path.join(tmp, 'src', 'Anything.cs');
    const r = await runAutoMark(makeEditInput(tmp, editedFile), { cwd: tmp });
    assert.equal(r.code, 0);
    assert.equal(r.stdout, '');
    assert.equal(r.stderr, '');
  });

  it('exits silently for non-Edit/Write tools', async () => {
    writeSpec(tmp, 'demo', SPEC_WITH_OPEN);
    writePipelineState(tmp, 'demo');
    const r = await runAutoMark({
      tool_name: 'Bash',
      tool_input: { command: 'echo hello' },
      cwd: tmp,
    }, { cwd: tmp });
    assert.equal(r.code, 0);
    assert.equal(r.stdout, '');
  });
});
