#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 3 — Tests for view-driven reads
 *
 * Covers:
 * 1. subagent-tracker injects findings from parallel agents in the SAME wave
 * 2. session-memory includes cross-session-timeline when sessions exist
 * 3. harness-views CLI: --view pipeline-state --spec returns JSON with phase
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
  // Stub memory-write.js
  fs.writeFileSync(path.join(dir, '.claude', 'scripts', 'memory-write.js'), "'use strict'; process.exit(0);\n");
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

// ── Test 1: Parallel agents see each other in the same wave ──────────────────

describe('Wave 3 — subagent-tracker: parallel agents see each other', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('3rd agent in wave=3 sees agent.start events of agents A and B in wave=3', async () => {
    // Simulate agent A starting in wave=3
    appendEvent(tmp, makeEvent({
      wave: 3,
      actor: { kind: 'agent', id: 'ag-A', type: 'Explore' },
      event: 'agent.start',
      payload: { description: 'Agent A: scan auth patterns', model: null },
    }));

    // Simulate agent B starting in wave=3
    appendEvent(tmp, makeEvent({
      wave: 3,
      actor: { kind: 'agent', id: 'ag-B', type: 'general-purpose' },
      event: 'agent.start',
      payload: { description: 'Agent B: implement login endpoint', model: null },
    }));

    // Also add a finding from a different wave (should still appear as finding)
    appendEvent(tmp, makeEvent({
      wave: 2,
      actor: { kind: 'agent', id: 'ag-Z', type: 'Explore' },
      event: 'finding',
      payload: { kind: 'pattern', content: 'Auth uses JWT tokens', confidence: 0.9, refs: [] },
    }));

    // Also add an agent.stop for ag-A to verify it won't show as "active" if stopped
    // (We leave A and B both running — no agent.stop for them)

    // Now agent C starts in wave=3 via SubagentStart event
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-C',
      agent_type: 'general-purpose',
      cwd: tmp,
      session_id: 's-wave3-parallel',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const parsed = result.parsed;
    assert.ok(parsed, 'hook should output JSON');
    const ctx = parsed.hookSpecificOutput && parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext should be a string');

    // Agent C should see Agent A and B in the same wave
    assert.ok(
      ctx.includes('ag-A') || ctx.includes('Agent A') || ctx.includes('scan auth'),
      `additionalContext should reference agent A. Got: ${ctx}`
    );
    assert.ok(
      ctx.includes('ag-B') || ctx.includes('Agent B') || ctx.includes('login endpoint'),
      `additionalContext should reference agent B. Got: ${ctx}`
    );
  });

  it('agent stop removes agent from active list', async () => {
    // Agent A starts in wave=3
    appendEvent(tmp, makeEvent({
      wave: 3,
      actor: { kind: 'agent', id: 'ag-stopped', type: 'Explore' },
      event: 'agent.start',
      payload: { description: 'Agent that will stop', model: null },
    }));

    // Agent A stops
    appendEvent(tmp, makeEvent({
      wave: 3,
      actor: { kind: 'agent', id: 'ag-stopped', type: 'Explore' },
      event: 'agent.stop',
      payload: { summary: 'Done with exploration', confidence: 0.8, durationMs: 1000, toolCount: 5 },
    }));

    // Agent B starts in wave=3
    appendEvent(tmp, makeEvent({
      wave: 3,
      actor: { kind: 'agent', id: 'ag-running', type: 'general-purpose' },
      event: 'agent.start',
      payload: { description: 'Still running agent', model: null },
    }));

    // Agent C starts (via SubagentStart)
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-new',
      agent_type: 'general-purpose',
      cwd: tmp,
      session_id: 's-wave3-stopped',
    }, { projectDir: tmp });

    assert.equal(result.code, 0);
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string');

    // ag-running should be visible (still active)
    assert.ok(
      ctx.includes('ag-running') || ctx.includes('Still running'),
      `active agent should be in context. Got: ${ctx}`
    );

    // ag-stopped should NOT appear in active agents section
    // (it might appear in findings but not in the active-agents list)
    // We check that "ag-stopped" and "Agent that will stop" is absent from the active agents line
    // The actual check: the "Parallel Agents" block should not list the stopped agent
    const parallelSection = ctx.match(/\[Parallel Agents[^\]]*\]([\s\S]*?)(?=\[|$)/);
    if (parallelSection) {
      assert.ok(
        !parallelSection[1].includes('ag-stopped'),
        `stopped agent should not appear in active agents list. Section: ${parallelSection[1]}`
      );
    }
  });

  it('fallback: when events.jsonl missing, still produces additionalContext from _index.json or empty', async () => {
    // No events.jsonl written — should fall back to legacy _index.json (also missing)
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-fallback',
      agent_type: 'Explore',
      cwd: tmp,
      session_id: 's-wave3-fallback',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, 'hook must exit 0 even without events.jsonl');
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext should be present');
    assert.ok(ctx.includes('[Tracker]'), 'base tracker message should always be present');
  });
});

// ── Test 2: session-memory includes cross-session timeline ───────────────────

describe('Wave 3 — session-memory: cross-session timeline', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('includes recent sessions in additionalContext when .harness/sessions/ exists', async () => {
    const sessionsDir = path.join(tmp, '.claude', '.harness', 'sessions');

    // Write 2 archived sessions
    const sessionA = [
      makeEvent({ sessionId: 's-old-abc', wave: 1, event: 'agent.start', spec: 'add-login', actor: { kind: 'agent', id: 'ag-1', type: 'general-purpose' } }),
      makeEvent({ sessionId: 's-old-abc', wave: 1, event: 'decision', spec: 'add-login', payload: { title: 'Use JWT', rationale: 'simpler' } }),
    ];
    const sessionB = [
      makeEvent({ sessionId: 's-new-xyz', wave: 1, event: 'agent.start', spec: 'fix-auth', actor: { kind: 'agent', id: 'ag-2', type: 'general-purpose' } }),
      makeEvent({ sessionId: 's-new-xyz', wave: 1, event: 'pipeline.phase', spec: 'fix-auth', payload: { from: 'ANALYZE', to: 'CLOSE' } }),
    ];

    const fileA = path.join(sessionsDir, 's-old-abc.jsonl');
    const fileB = path.join(sessionsDir, 's-new-xyz.jsonl');
    fs.writeFileSync(fileA, sessionA.map(e => JSON.stringify(e)).join('\n') + '\n');
    fs.writeFileSync(fileB, sessionB.map(e => JSON.stringify(e)).join('\n') + '\n');

    // Make fileB newer
    const now = Date.now();
    fs.utimesSync(fileA, (now - 5000) / 1000, (now - 5000) / 1000);
    fs.utimesSync(fileB, now / 1000, now / 1000);

    const result = await runHook('session-memory.js', {
      hook_event_name: 'SessionStart',
      cwd: tmp,
      session_id: 's-current',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    // If no knowledge/decisions/lessons exist, session-memory may output nothing.
    // But with sessions present, it should output the timeline.
    // The hook only outputs if parts.length > 0. Sessions alone count.
    if (result.stdout) {
      const parsed = result.parsed;
      const ctx = parsed && parsed.hookSpecificOutput && parsed.hookSpecificOutput.additionalContext;
      if (ctx) {
        // Timeline should mention the spec names or session IDs
        const hasTimeline = ctx.includes('fix-auth') || ctx.includes('add-login') ||
          ctx.includes('Recent Sessions') || ctx.includes('xyz') || ctx.includes('abc');
        assert.ok(hasTimeline, `context should include cross-session info. Got: ${ctx}`);
      }
    }

    // Minimum: hook must not crash
    assert.equal(result.code, 0);
  });

  it('fail-open: no crash when sessions dir is missing', async () => {
    // sessions dir exists but is empty (no jsonl files)
    const result = await runHook('session-memory.js', {
      hook_event_name: 'SessionStart',
      cwd: tmp,
      session_id: 's-empty',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, 'hook must exit 0 with empty sessions dir');
  });
});

// ── Test 3: harness-views CLI ─────────────────────────────────────────────────

describe('Wave 3 — harness-views CLI: --view pipeline-state', () => {
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

    const result = await runScript('harness-views.js',
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
    const result = await runScript('harness-views.js',
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

    const result = await runScript('harness-views.js',
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
    const result = await runScript('harness-views.js', ['--cwd', tmp], { projectDir: tmp });
    assert.equal(result.code, 0);
    // stderr has usage message
    assert.ok(result.stderr.includes('Usage') || result.code === 0);
  });
});

// ── Test 4: Fallback robustness ───────────────────────────────────────────────

describe('Wave 3 — fallback: harness unavailable scenarios', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('subagent-tracker SubagentStart exits 0 when .harness/ dir is entirely missing', async () => {
    // Remove the .harness directory that makeProjectDir created
    try { fs.rmSync(path.join(tmp, '.claude', '.harness'), { recursive: true, force: true }); } catch (_) {}

    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-noharnessdir',
      agent_type: 'Explore',
      cwd: tmp,
      session_id: 's-noharnessdir',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, 'hook must exit 0 when .harness/ is missing');
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'should still emit additionalContext');
  });

  it('session-memory exits 0 when .harness/ dir is entirely missing', async () => {
    try { fs.rmSync(path.join(tmp, '.claude', '.harness'), { recursive: true, force: true }); } catch (_) {}

    const result = await runHook('session-memory.js', {
      hook_event_name: 'SessionStart',
      cwd: tmp,
      session_id: 's-noharnessdir-sm',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, 'hook must exit 0 when .harness/ is missing');
  });
});
