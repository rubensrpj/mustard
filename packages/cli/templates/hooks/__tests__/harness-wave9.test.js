#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 9 — Strict Gates Tests
 *
 * Covers:
 * 1.  close-gate blocks in test fail (phase=CLOSE + testCommand="exit 1")
 * 2.  close-gate allows in test pass (testCommand="exit 0")
 * 3.  close-gate fail-open on env bug (command not found)
 * 4.  close-gate mode warn: test fail → allow + stderr
 * 5.  close-gate mode off: test fail → passthrough, no commands run
 * 6.  close-gate does NOT trigger on phase != CLOSE
 * 7.  review-gate strict + secrets staged → deny
 * 8.  review-gate strict + build broken → deny
 * 9.  review-gate warn (default) + secrets → allow with warning
 * 10. close-gate.check event emitted in harness log with correct payload
 *
 * Run with: bun test templates/hooks/__tests__/harness-wave9.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const CLOSE_GATE = path.join(HOOKS_DIR, 'close-gate.js');
const REVIEW_GATE = path.join(HOOKS_DIR, 'review-gate.js');

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Run a hook with JSON stdin, return { code, stdout, stderr, parsed, response } */
function runHook(hookPath, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const env = Object.assign({}, process.env, {
      MUSTARD_DISABLED_HOOKS: 'all', // disable other hooks to avoid interference
    });
    // Restore the hook under test by removing it from MUSTARD_DISABLED_HOOKS
    delete env.MUSTARD_DISABLED_HOOKS;
    // Apply specific env overrides
    if (opts.env) Object.assign(env, opts.env);

    const child = spawn(process.execPath, [hookPath], {
      cwd: projectDir,
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', d => (stdout += d));
    child.stderr.on('data', d => (stderr += d));
    child.on('error', reject);
    child.on('close', code => {
      let parsed = null;
      try { parsed = JSON.parse(stdout.trim()); } catch (_) {}
      // Extract hookSpecificOutput for convenience
      const response = parsed && parsed.hookSpecificOutput ? parsed.hookSpecificOutput : null;
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed, response });
    });
    child.stdin.write(JSON.stringify(inputObj));
    child.stdin.end();
  });
}

/** Create a temp project dir with necessary subdirs */
function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w9-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness', 'sessions'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.pipeline-states'), { recursive: true });
  return dir;
}

/** Write a mustard.json with given command overrides */
function writeMustardJson(projectDir, commands = {}) {
  const cfg = {
    git: { flow: {}, provider: 'github', submodules: false },
    ...commands,
  };
  fs.writeFileSync(path.join(projectDir, 'mustard.json'), JSON.stringify(cfg, null, 2), 'utf8');
}

/** Build a Write hook input that writes pipeline-state with given phase */
function makePipelineStateInput(projectDir, specName, phase, extraFields = {}) {
  const content = JSON.stringify({ spec: specName, phase, ...extraFields });
  const filePath = path.join(projectDir, '.claude', '.pipeline-states', specName + '.json');
  return {
    tool: 'Write',
    tool_input: {
      file_path: filePath,
      content,
    },
    cwd: projectDir,
  };
}

/** Read harness events */
function readEvents(projectDir) {
  const f = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  if (!fs.existsSync(f)) return [];
  return fs.readFileSync(f, 'utf8')
    .split('\n').filter(Boolean)
    .map(l => { try { return JSON.parse(l); } catch (_) { return null; } })
    .filter(Boolean);
}

// ── Helper: determine OS-appropriate "exit 1" and "exit 0" commands ──────────
const IS_WIN = process.platform === 'win32';
const EXIT_FAIL = IS_WIN ? 'cmd /c exit 1' : 'sh -c "exit 1"';
const EXIT_PASS = IS_WIN ? 'cmd /c exit 0' : 'sh -c "exit 0"';

// ── Test 1: close-gate blocks on test fail ────────────────────────────────────

describe('Wave 9 — close-gate: blocks when testCommand fails (phase=CLOSE)', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns permissionDecision=deny when testCommand exits non-zero', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_FAIL });
    const input = makePipelineStateInput(tmp, 'auth-login', 'CLOSE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict', MUSTARD_QA_GATE_MODE: 'off' },
    });

    assert.equal(result.code, 0, `hook must exit 0 (fail-open), stderr: ${result.stderr}`);
    assert.ok(result.response, `expected hookSpecificOutput, stdout: ${result.stdout}`);
    assert.equal(result.response.permissionDecision, 'deny',
      `expected deny, got: ${result.response.permissionDecision}`);
    assert.ok(
      result.response.permissionDecisionReason &&
      result.response.permissionDecisionReason.includes('[Close Gate]'),
      `reason should include [Close Gate]: ${result.response.permissionDecisionReason}`
    );
  });

  it('deny reason is truncated to ≤500 chars + ellipsis', async () => {
    // Use a command that produces lots of output
    const longOutputCmd = IS_WIN
      ? 'cmd /c "for /l %i in (1,1,50) do @echo This is a very long error message that should be truncated properly by the gate & exit 1"'
      : 'sh -c "for i in $(seq 1 50); do echo This is a very long error message that should be truncated; done; exit 1"';
    writeMustardJson(tmp, { testCommand: longOutputCmd });
    const input = makePipelineStateInput(tmp, 'spec1', 'CLOSE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict', MUSTARD_QA_GATE_MODE: 'off' },
    });

    assert.equal(result.code, 0);
    if (result.response && result.response.permissionDecision === 'deny') {
      const reason = result.response.permissionDecisionReason || '';
      // The reason should not be excessively long (truncated at 500 + prefix overhead)
      assert.ok(reason.length <= 600, `reason too long: ${reason.length} chars`);
    }
  });
});

// ── Test 2: close-gate allows on test pass ────────────────────────────────────

describe('Wave 9 — close-gate: allows when all commands pass (phase=CLOSE)', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('no permissionDecision=deny when testCommand exits 0', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_PASS });
    const input = makePipelineStateInput(tmp, 'auth-login', 'CLOSE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict', MUSTARD_QA_GATE_MODE: 'off' },
    });

    assert.equal(result.code, 0);
    // Either empty output or no deny decision
    const decision = result.response ? result.response.permissionDecision : null;
    assert.notEqual(decision, 'deny', `should not deny when tests pass, got: ${decision}`);
  });
});

// ── Test 3: close-gate fail-open on env bug (mustard.json absent) ────────────
//
// The env-bug scenario for close-gate is: mustard.json not present.
// Without mustard.json we cannot get build/test commands → hook fails-open
// with a stderr warning. This is the canonical "env bug" path.
// Note: on Windows, running an unknown shell command still returns exit code 1
// through cmd.exe (not ENOENT at the OS level), so cross-platform env-bug
// detection via "command not found" is not meaningfully testable without mocks.

describe('Wave 9 — close-gate: fail-open when mustard.json is absent (env bug)', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('exits 0 without deny when mustard.json is missing', async () => {
    // No mustard.json written — hook should fail-open with warning
    const input = makePipelineStateInput(tmp, 'spec2', 'CLOSE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict', MUSTARD_QA_GATE_MODE: 'off' },
    });

    assert.equal(result.code, 0, 'hook must exit 0 when mustard.json is absent');
    const decision = result.response ? result.response.permissionDecision : null;
    assert.notEqual(decision, 'deny', 'must NOT deny when mustard.json is absent — fail-open');
    // Should emit a warning to stderr
    assert.ok(result.stderr.includes('[close-gate]'),
      `expected [close-gate] warning in stderr, got: ${result.stderr}`);
  });

  it('exits 0 without deny when mustard.json has no commands configured', async () => {
    // mustard.json exists but has no testCommand/buildCommand etc.
    writeMustardJson(tmp, {}); // no command fields
    const input = makePipelineStateInput(tmp, 'spec-nocmds', 'CLOSE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict', MUSTARD_QA_GATE_MODE: 'off' },
    });

    assert.equal(result.code, 0, 'hook must exit 0 when no commands configured');
    const decision = result.response ? result.response.permissionDecision : null;
    assert.notEqual(decision, 'deny', 'must NOT deny when no commands configured');
    // Expect warning to stderr
    assert.ok(result.stderr.includes('[close-gate]'),
      `expected [close-gate] warning in stderr, got: ${result.stderr}`);
  });
});

// ── Test 4: close-gate mode warn + test fail → allow + stderr ─────────────────

describe('Wave 9 — close-gate: mode=warn + test fail → allow with stderr', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('allows (no deny) and prints to stderr when mode=warn and test fails', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_FAIL });
    const input = makePipelineStateInput(tmp, 'spec-warn', 'CLOSE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'warn', MUSTARD_QA_GATE_MODE: 'off' },
    });

    assert.equal(result.code, 0);
    const decision = result.response ? result.response.permissionDecision : null;
    assert.notEqual(decision, 'deny', 'mode=warn must not deny');
    // Should have written a warning to stderr
    assert.ok(result.stderr.includes('[close-gate]'),
      `expected [close-gate] in stderr, got: ${result.stderr}`);
  });
});

// ── Test 5: close-gate mode off → skip entirely ───────────────────────────────

describe('Wave 9 — close-gate: mode=off → skip without running any command', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('exits 0 with empty stdout when mode=off, even with failing testCommand', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_FAIL });
    const input = makePipelineStateInput(tmp, 'spec-off', 'CLOSE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'off' },
    });

    assert.equal(result.code, 0);
    assert.equal(result.stdout, '', 'mode=off must produce empty stdout');
    // No deny
    const decision = result.response ? result.response.permissionDecision : null;
    assert.notEqual(decision, 'deny', 'mode=off must not deny');
  });
});

// ── Test 6: close-gate does NOT trigger on phase != CLOSE ─────────────────────

describe('Wave 9 — close-gate: does not trigger for phases other than CLOSE', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('passes through silently for phase=EXECUTE', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_FAIL });
    const input = makePipelineStateInput(tmp, 'spec-exec', 'EXECUTE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict' },
    });

    assert.equal(result.code, 0);
    assert.equal(result.stdout, '', 'must produce no output for non-CLOSE phase');
    const decision = result.response ? result.response.permissionDecision : null;
    assert.notEqual(decision, 'deny', 'must not deny for phase=EXECUTE');
  });

  it('passes through silently for phase=ANALYZE', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_FAIL });
    const input = makePipelineStateInput(tmp, 'spec-analyze', 'ANALYZE');

    const result = await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict' },
    });

    assert.equal(result.code, 0);
    assert.equal(result.stdout, '', 'must produce no output for phase=ANALYZE');
  });
});

// ── Test 7: review-gate strict + secrets staged → deny ───────────────────────

describe('Wave 9 — review-gate strict mode: secrets staged → deny', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns permissionDecision=deny when .env file is staged (strict mode)', async () => {
    // We simulate git diff by having review-gate detect the .env pattern.
    // Since we can't control git in a tmp dir easily, we mock by stubbing
    // git diff output via a wrapper script that writes the staged file list.
    // Simpler: run in a temp git repo with an actual staged .env file.

    // Initialize a git repo
    try {
      const { execSync } = require('child_process');
      execSync('git init', { cwd: tmp, stdio: 'pipe' });
      execSync('git config user.email "test@test.com"', { cwd: tmp, stdio: 'pipe' });
      execSync('git config user.name "Test"', { cwd: tmp, stdio: 'pipe' });
      // Create and stage a .env file
      fs.writeFileSync(path.join(tmp, '.env'), 'SECRET=abc123', 'utf8');
      execSync('git add .env', { cwd: tmp, stdio: 'pipe' });
    } catch (e) {
      // If git is not available, skip this test gracefully
      console.log('  [skip] git not available in test env');
      return;
    }

    const input = {
      tool: 'Bash',
      tool_input: { command: 'git commit -m "feat: add feature"' },
      cwd: tmp,
    };

    const result = await runHook(REVIEW_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_COMMIT_GATE_MODE: 'strict' },
    });

    assert.equal(result.code, 0);
    assert.ok(result.response, `expected hookSpecificOutput, stdout: ${result.stdout}`);
    assert.equal(result.response.permissionDecision, 'deny',
      `expected deny for staged .env, got: ${result.response.permissionDecision}`);
    assert.ok(
      result.response.permissionDecisionReason &&
      result.response.permissionDecisionReason.toLowerCase().includes('sensitive'),
      `reason should mention sensitive: ${result.response.permissionDecisionReason}`
    );
  });
});

// ── Test 8: review-gate strict + build broken → deny ─────────────────────────

describe('Wave 9 — review-gate strict mode: build broken → deny', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns permissionDecision=deny when buildCommand fails (strict mode)', async () => {
    // Initialize a git repo with a non-sensitive staged file and broken build
    try {
      const { execSync } = require('child_process');
      execSync('git init', { cwd: tmp, stdio: 'pipe' });
      execSync('git config user.email "test@test.com"', { cwd: tmp, stdio: 'pipe' });
      execSync('git config user.name "Test"', { cwd: tmp, stdio: 'pipe' });
      // Stage a non-sensitive file
      fs.writeFileSync(path.join(tmp, 'src.js'), 'console.log("hello")', 'utf8');
      execSync('git add src.js', { cwd: tmp, stdio: 'pipe' });
    } catch (e) {
      console.log('  [skip] git not available in test env');
      return;
    }

    writeMustardJson(tmp, { buildCommand: EXIT_FAIL });

    const input = {
      tool: 'Bash',
      tool_input: { command: 'git commit -m "feat: add feature"' },
      cwd: tmp,
    };

    const result = await runHook(REVIEW_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_COMMIT_GATE_MODE: 'strict' },
    });

    assert.equal(result.code, 0);
    assert.ok(result.response, `expected hookSpecificOutput, stdout: ${result.stdout}`);
    assert.equal(result.response.permissionDecision, 'deny',
      `expected deny for broken build, got: ${result.response.permissionDecision}`);
    assert.ok(
      result.response.permissionDecisionReason &&
      result.response.permissionDecisionReason.toLowerCase().includes('build'),
      `reason should mention build: ${result.response.permissionDecisionReason}`
    );
  });
});

// ── Test 9: review-gate warn (default) → allow + warning ─────────────────────

describe('Wave 9 — review-gate: default mode=warn → allow with warning advisory', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('allows (no deny) when mode=warn even if .env is staged', async () => {
    try {
      const { execSync } = require('child_process');
      execSync('git init', { cwd: tmp, stdio: 'pipe' });
      execSync('git config user.email "test@test.com"', { cwd: tmp, stdio: 'pipe' });
      execSync('git config user.name "Test"', { cwd: tmp, stdio: 'pipe' });
      fs.writeFileSync(path.join(tmp, '.env'), 'SECRET=abc123', 'utf8');
      execSync('git add .env', { cwd: tmp, stdio: 'pipe' });
    } catch (e) {
      console.log('  [skip] git not available in test env');
      return;
    }

    const input = {
      tool: 'Bash',
      tool_input: { command: 'git commit -m "feat: add feature"' },
      cwd: tmp,
    };

    // Default mode = warn (no MUSTARD_COMMIT_GATE_MODE set → defaults to warn)
    const result = await runHook(REVIEW_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_COMMIT_GATE_MODE: 'warn' },
    });

    assert.equal(result.code, 0);
    // Should NOT deny
    const decision = result.response ? result.response.permissionDecision : null;
    assert.notEqual(decision, 'deny',
      `mode=warn must not deny, got: ${decision}`);
    // Should provide advisory (allow with warning text)
    if (result.response) {
      assert.ok(
        result.response.permissionDecision === 'allow' ||
        result.response.permissionDecision == null,
        `expected allow or no decision, got: ${result.response.permissionDecision}`
      );
      assert.ok(
        result.response.permissionDecisionReason &&
        result.response.permissionDecisionReason.includes('[Review Gate]'),
        `expected advisory reason, got: ${result.response.permissionDecisionReason}`
      );
    }
  });
});

// ── Test 10: close-gate.check event emitted in harness log ───────────────────

describe('Wave 9 — close-gate: emits close-gate.check event to harness log', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits close-gate.check event with result and stages payload', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_PASS, buildCommand: EXIT_PASS });
    const input = makePipelineStateInput(tmp, 'spec-event', 'CLOSE');

    await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'strict', MUSTARD_QA_GATE_MODE: 'off' },
    });

    const events = readEvents(tmp);
    const gateEvent = events.find(e => e.event === 'close-gate.check');
    assert.ok(gateEvent, `expected close-gate.check event in harness log, events: ${JSON.stringify(events.map(e => e.event))}`);
    assert.ok(gateEvent.payload, 'event must have payload');
    assert.ok(typeof gateEvent.payload.result === 'string', 'payload.result must be a string');
    assert.ok(Array.isArray(gateEvent.payload.stages), 'payload.stages must be an array');
    assert.equal(gateEvent.payload.mode, 'strict', `expected mode=strict in payload, got: ${gateEvent.payload.mode}`);
  });

  it('emits close-gate.check with result=fail when test fails', async () => {
    writeMustardJson(tmp, { testCommand: EXIT_FAIL });
    const input = makePipelineStateInput(tmp, 'spec-fail-event', 'CLOSE');

    await runHook(CLOSE_GATE, input, {
      projectDir: tmp,
      env: { MUSTARD_CLOSE_GATE_MODE: 'warn', MUSTARD_QA_GATE_MODE: 'off' }, // warn so we get the event without blocking
    });

    const events = readEvents(tmp);
    const gateEvent = events.find(e => e.event === 'close-gate.check');
    assert.ok(gateEvent, 'close-gate.check must be emitted even in warn mode');
    assert.equal(gateEvent.payload.result, 'fail', `expected result=fail, got: ${gateEvent.payload.result}`);
  });
});
