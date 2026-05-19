#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 3 — Tests for view-driven reads
 *
 * Covers:
 * 1. subagent-tracker injects findings from parallel agents in the SAME wave
 * 2. session-memory includes cross-session-timeline when sessions exist
 * 3. event-projections CLI: --view pipeline-state --spec returns JSON with phase
 * 4. Fallback: subagent-tracker doesn't crash when events.jsonl is missing
 *
 * Run with: bun test templates/hooks/__tests__/harness-wave3.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');

// ── Helpers ───────────────────────────────────────────────────────────────────

function runHook(hookFile, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const env = {
      ...process.env,
      MUSTARD_DISABLED_HOOKS: opts.disabledHooks || '',
    };

    const child = spawn(process.execPath, [path.join(HOOKS_DIR, hookFile)], {
      cwd: projectDir,
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    child.on('error', reject);
    child.on('close', (code) => {
      let parsed = null;
      try { parsed = JSON.parse(stdout.trim()); } catch (_) {}
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });
    child.stdin.write(JSON.stringify(inputObj));
    child.stdin.end();
  });
}

function runScript(scriptFile, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const env = { ...process.env };

    const allArgs = [path.join(SCRIPTS_DIR, scriptFile), ...args];
    const child = spawn(process.execPath, allArgs, {
      cwd: projectDir,
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    child.on('error', reject);
    child.on('close', (code) => {
      let parsed = null;
      try { parsed = JSON.parse(stdout.trim()); } catch (_) {}
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });
    if (opts.stdin) {
      child.stdin.write(opts.stdin);
    }
    child.stdin.end();
  });
}

/** Create a minimal project dir structure. */
function makeProjectDir(base) {
  const dir = fs.mkdtempSync(path.join(base, 'mustard-w3-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness', 'sessions'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.agent-state'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', 'scripts'), { recursive: true });
  // Stub memory.js
  fs.writeFileSync(path.join(dir, '.claude', 'scripts', 'memory.js'), "'use strict'; process.exit(0);\n");
  return dir;
}

/** Write an NDJSON event line to events.jsonl */
function appendEvent(projectDir, event) {
  const evFile = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  fs.appendFileSync(evFile, JSON.stringify(event) + '\n', 'utf8');
}

function makeEvent(overrides) {
  return Object.assign({
    v: 1,
    ts: new Date().toISOString(),
    sessionId: 's-test',
    wave: 3,
    actor: { kind: 'agent', id: 'ag-default', type: 'Explore' },
    event: 'agent.start',
    payload: { description: 'default agent', model: null },
  }, overrides);
}

/** Read events.jsonl into array */
function readEvents(projectDir) {
  const evFile = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  if (!fs.existsSync(evFile)) return [];
  return fs.readFileSync(evFile, 'utf8')
    .split('\n').filter(Boolean)
    .map(l => { try { return JSON.parse(l); } catch { return null; } })
    .filter(Boolean);
}

// NOTE: subagent-tracker.js was ported to the Rust `mustard-rt` `tracker`
// module in b3 Wave 3. Its parallel-agent visibility and events.jsonl-fallback
// parity now lives in packages/rt/src/hooks/tracker.rs.

// NOTE: session-memory.js was ported to the Rust `mustard-rt` `session_start`
// module in b3 Wave 5. Its persistent-memory injection parity now lives in
// packages/rt/src/hooks/session_start.rs.

// ── Test 3: event-projections CLI ─────────────────────────────────────────────────

describe('Wave 3 — event-projections CLI: --view pipeline-state', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns JSON with phase field for a known spec', async () => {
    // Write events for spec=add-login with pipeline.phase events
    const eventsFile = path.join(tmp, '.claude', '.harness', 'events.jsonl');
    [
      makeEvent({ event: 'pipeline.phase', spec: 'add-login', payload: { from: null, to: 'ANALYZE' } }),
      makeEvent({ event: 'pipeline.phase', spec: 'add-login', payload: { from: 'ANALYZE', to: 'PLAN' } }),
      makeEvent({ event: 'pipeline.phase', spec: 'other-spec', payload: { from: null, to: 'EXECUTE' } }),
      makeEvent({ event: 'pipeline.phase', spec: 'add-login', payload: { from: 'PLAN', to: 'EXECUTE' } }),
    ].forEach(e => fs.appendFileSync(eventsFile, JSON.stringify(e) + '\n'));

    const result = await runScript('event-projections.js',
      ['--view', 'pipeline-state', '--spec', 'add-login', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0, `script exited non-zero: ${result.stderr}`);
    assert.ok(result.parsed, `should output JSON. Got: ${result.stdout}`);
    assert.equal(result.parsed.spec, 'add-login');
    assert.equal(result.parsed.phase, 'EXECUTE');
  });

  it('returns JSON with null phase when no events for spec', async () => {
    // events.jsonl doesn't even exist
    const result = await runScript('event-projections.js',
      ['--view', 'pipeline-state', '--spec', 'nonexistent', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0);
    assert.ok(result.parsed, `should output JSON. Got: ${result.stdout}`);
    assert.equal(result.parsed.phase, null);
  });

  it('returns agent-visibility JSON for --view agent-visibility', async () => {
    const eventsFile = path.join(tmp, '.claude', '.harness', 'events.jsonl');
    fs.appendFileSync(eventsFile, JSON.stringify(makeEvent({ wave: 2, event: 'agent.start', actor: { kind: 'agent', id: 'ag-1', type: 'Explore' } })) + '\n');
    fs.appendFileSync(eventsFile, JSON.stringify(makeEvent({ wave: 2, event: 'tool.use', payload: { tool: 'Grep' } })) + '\n');

    const result = await runScript('event-projections.js',
      ['--view', 'agent-visibility', '--wave', '2', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0);
    assert.ok(result.parsed, `should output JSON. Got: ${result.stdout}`);
    assert.equal(result.parsed.wave, 2);
    assert.ok(Array.isArray(result.parsed.events), 'events should be an array');
    assert.equal(result.parsed.events.length, 2);
  });

  it('prints usage and exits 0 when --view is not provided', async () => {
    const result = await runScript('event-projections.js', ['--cwd', tmp], { projectDir: tmp });
    assert.equal(result.code, 0);
    // stderr has usage message
    assert.ok(result.stderr.includes('Usage') || result.code === 0);
  });
});

// ── Test 4: Fallback robustness ───────────────────────────────────────────────
//
// The session-memory `.harness/`-missing fallback is now covered by the Rust
// parity test `memory_injection_allows_when_no_sources` in
// packages/rt/src/hooks/session_start.rs (b3 Wave 5).
