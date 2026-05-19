#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 4 — Subtraction Tests
 *
 * Verifies that legacy stores are NO LONGER written:
 * 1. subagent-tracker does NOT create .agent-memory/_index.json
 * 2. subagent-tracker does NOT create .agent-state/_queue.json
 * 3. subagent-tracker does NOT create .agent-state/{id}.json
 * 4. metrics-tracker does NOT create .pipeline-states/*.metrics.json
 * 5. buildPipelineState derived from log contains metrics (tool counts, agent count)
 *
 * Run with: bun test templates/hooks/__tests__/harness-wave4.test.js
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

/** Create a minimal project dir with harness + pipeline-states dirs. */
function makeProjectDir(base) {
  const dir = fs.mkdtempSync(path.join(base, 'mustard-w4-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness'), { recursive: true });
  return dir;
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

// ── Test 1: subagent-tracker does NOT write .agent-memory/_index.json ─────────

describe('Wave 4 — subagent-tracker: no .agent-memory writes', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('SubagentStop does NOT create .agent-memory/_index.json', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStop',
      agent_id: 'ag-w4-stop',
      agent_type: 'general-purpose',
      cwd: tmp,
      session_id: 's-w4-stop',
      tool_response: { output: 'Completed the task successfully.' },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const agentMemDir = path.join(tmp, '.claude', '.agent-memory');
    assert.ok(!fs.existsSync(agentMemDir), '.agent-memory/ must NOT be created (Wave 4)');
  });

  it('SubagentStop does NOT create .agent-state/{id}.json', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStop',
      agent_id: 'ag-w4-state',
      agent_type: 'Explore',
      cwd: tmp,
      session_id: 's-w4-state',
      tool_response: { output: 'Done.' },
    }, { projectDir: tmp });

    assert.equal(result.code, 0);

    const stateFile = path.join(tmp, '.claude', '.agent-state', 'ag-w4-state.json');
    assert.ok(!fs.existsSync(stateFile), '.agent-state/{id}.json must NOT be created (Wave 4)');
  });
});

// ── Test 2: subagent-tracker does NOT write _queue.json ───────────────────────

describe('Wave 4 — subagent-tracker: no _queue.json writes', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('PreToolUse(Task) does NOT create .agent-state/_queue.json', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      cwd: tmp,
      session_id: 's-w4-queue',
      tool_input: {
        description: 'Explore the codebase for auth patterns',
        subagent_type: 'Explore',
        prompt: 'Find auth-related files.',
      },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const queueFile = path.join(tmp, '.claude', '.agent-state', '_queue.json');
    assert.ok(!fs.existsSync(queueFile), '_queue.json must NOT be created (Wave 4)');
  });

  it('SubagentStart does NOT create .agent-state/{id}.json', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-w4-start',
      agent_type: 'general-purpose',
      cwd: tmp,
      session_id: 's-w4-start',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const stateFile = path.join(tmp, '.claude', '.agent-state', 'ag-w4-start.json');
    assert.ok(!fs.existsSync(stateFile), '.agent-state/{id}.json must NOT be created in SubagentStart (Wave 4)');
  });
});

// ── Test 3: metrics-tracker does NOT create .pipeline-states/*.metrics.json ───

describe('Wave 4 — metrics-tracker: no sidecar writes', () => {
  let tmp;
  beforeEach(() => {
    tmp = makeProjectDir(os.tmpdir());
    const statesDir = path.join(tmp, '.claude', '.pipeline-states');
    fs.mkdirSync(statesDir, { recursive: true });
    fs.writeFileSync(
      path.join(statesDir, 'my-spec.json'),
      JSON.stringify({ spec: 'my-spec', phaseName: 'EXECUTE', startedAt: new Date().toISOString() })
    );
  });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('PostToolUse does NOT create .pipeline-states/*.metrics.json', async () => {
    const result = await runHook('metrics-tracker.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Bash',
      cwd: tmp,
      session_id: 's-w4-metrics',
      tool_input: { command: 'echo hello' },
      tool_response: { output: 'hello' },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const statesDir = path.join(tmp, '.claude', '.pipeline-states');
    const sidecar = path.join(statesDir, 'my-spec.metrics.json');
    assert.ok(!fs.existsSync(sidecar), '.metrics.json sidecar must NOT be created (Wave 4)');

    // But tool.use event MUST be in the harness log
    const events = readEvents(tmp);
    const toolUse = events.find(e => e.event === 'tool.use');
    assert.ok(toolUse, 'tool.use event must still be emitted to harness log');
    assert.equal(toolUse.payload.tool, 'Bash');
  });

  it('multiple PostToolUse calls: no sidecars, log grows correctly', async () => {
    for (let i = 0; i < 3; i++) {
      const r = await runHook('metrics-tracker.js', {
        hook_event_name: 'PostToolUse',
        tool_name: 'Write',
        cwd: tmp,
        session_id: 's-w4-multi',
        tool_input: { file_path: path.join(tmp, `src/f${i}.ts`) },
        tool_response: {},
      }, { projectDir: tmp });
      assert.equal(r.code, 0);
    }

    const statesDir = path.join(tmp, '.claude', '.pipeline-states');
    const files = fs.readdirSync(statesDir);
    assert.ok(!files.some(f => f.endsWith('.metrics.json')), `No sidecar expected, got: ${files.join(', ')}`);

    // Log has 3 tool.use events
    const events = readEvents(tmp);
    const toolUses = events.filter(e => e.event === 'tool.use');
    assert.equal(toolUses.length, 3, 'Three tool.use events expected in log');
  });
});

// ── Test 4: buildPipelineState derives metrics from log ───────────────────────

describe('Wave 4 — buildPipelineState: metrics from log', () => {
  const harnessViews = require(path.join(SCRIPTS_DIR, 'event-projections.js'));

  it('aggregates tool.use counts and agent count from events', () => {
    const now = new Date().toISOString();
    const events = [
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'add-login', event: 'pipeline.phase', payload: { from: null, to: 'ANALYZE' }, actor: { kind: 'hook' } },
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'add-login', event: 'agent.start', payload: { description: 'Explore codebase', model: null }, actor: { kind: 'agent', id: 'ag-1', type: 'Explore' } },
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'add-login', event: 'tool.use', payload: { tool: 'Bash', phase: 'ANALYZE' }, actor: { kind: 'hook', id: 'metrics-tracker' } },
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'add-login', event: 'tool.use', payload: { tool: 'Edit', phase: 'ANALYZE' }, actor: { kind: 'hook', id: 'metrics-tracker' } },
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'add-login', event: 'tool.use', payload: { tool: 'Bash', phase: 'EXECUTE' }, actor: { kind: 'hook', id: 'metrics-tracker' } },
      // Retries are now counted from dispatch.failure events (real signal), not
      // from a keyword-derived `retry` flag on tool.use.
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'add-login', event: 'dispatch.failure', payload: { agentType: 'general-purpose', phase: 'EXECUTE' }, actor: { kind: 'hook', id: 'subagent-tracker' } },
      // Read events should NOT count toward apiCalls
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'add-login', event: 'tool.use', payload: { tool: 'Read', phase: 'ANALYZE' }, actor: { kind: 'hook', id: 'metrics-tracker' } },
      { v: 1, ts: now, sessionId: 's1', wave: 1, spec: 'other-spec', event: 'tool.use', payload: { tool: 'Write', phase: 'EXECUTE' }, actor: { kind: 'hook', id: 'metrics-tracker' } },
    ];

    const result = harnessViews.buildPipelineState(events, { spec: 'add-login' });

    assert.equal(result.spec, 'add-login');
    assert.equal(result.phase, 'ANALYZE');
    assert.ok(result.metrics, 'metrics object must be present');
    assert.equal(result.metrics.apiCalls, 3, 'Bash + Edit + Bash = 3 (Read excluded)');
    assert.equal(result.metrics.toolBreakdown.Bash, 2, 'Bash used twice');
    assert.equal(result.metrics.toolBreakdown.Edit, 1, 'Edit used once');
    assert.equal(result.metrics.retries, 1, 'One dispatch.failure event');
    assert.equal(result.metrics.dispatchFailuresByPhase.EXECUTE, 1, 'failure attributed to EXECUTE phase');
    assert.equal(result.metrics.agentCount, 1, 'One agent.start event');
    // other-spec tool.use should not bleed in
    assert.ok(!result.metrics.toolBreakdown.Write, 'Write from other-spec must not appear');
  });

  it('returns zero metrics when no tool.use events', () => {
    const events = [
      { v: 1, ts: new Date().toISOString(), sessionId: 's1', wave: 1, spec: 'empty-spec', event: 'pipeline.phase', payload: { from: null, to: 'ANALYZE' }, actor: {} },
    ];

    const result = harnessViews.buildPipelineState(events, { spec: 'empty-spec' });
    assert.equal(result.metrics.apiCalls, 0);
    assert.equal(result.metrics.retries, 0);
    assert.equal(result.metrics.agentCount, 0);
  });

  it('no-spec mode: aggregates all events regardless of spec', () => {
    const now = new Date().toISOString();
    const events = [
      { v: 1, ts: now, wave: 1, spec: 'spec-a', event: 'tool.use', payload: { tool: 'Edit' }, actor: {} },
      { v: 1, ts: now, wave: 1, spec: 'spec-b', event: 'tool.use', payload: { tool: 'Write' }, actor: {} },
    ];

    const result = harnessViews.buildPipelineState(events, {});
    assert.equal(result.metrics.apiCalls, 2, 'Both events counted when no spec filter');
  });
});
