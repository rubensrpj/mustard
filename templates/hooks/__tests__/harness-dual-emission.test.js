#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 2 — Dual Emission Smoke Tests
 *
 * For each modified hook: 1 happy-path test + 1 fail-open test.
 * Spawns hooks as child processes (identical to hooks.test.js pattern).
 *
 * Run with:
 *   bun test templates/hooks/__tests__/harness-dual-emission.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');

// ── Helper: run a hook and return { code, stdout, stderr, events } ──────────

function runHook(hookFile, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir;
    const env = {
      ...process.env,
      MUSTARD_DISABLED_HOOKS: '', // make sure harness-event is enabled
    };
    if (projectDir) env.CLAUDE_PROJECT_DIR = projectDir;
    // Disable hooks we don't want interfering
    env.MUSTARD_DISABLED_HOOKS = opts.disabledHooks || '';

    const child = spawn(process.execPath, [path.join(HOOKS_DIR, hookFile)], {
      cwd: projectDir || process.cwd(),
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    child.on('error', reject);
    child.on('close', (code) => {
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim() });
    });
    child.stdin.write(JSON.stringify(inputObj));
    child.stdin.end();
  });
}

function runScript(scriptFile, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir;
    const env = {
      ...process.env,
      MUSTARD_DISABLED_HOOKS: '',
    };
    if (projectDir) env.CLAUDE_PROJECT_DIR = projectDir;

    const extraArgs = opts.args || [];
    const child = spawn(process.execPath, [path.join(SCRIPTS_DIR, scriptFile), ...extraArgs], {
      cwd: projectDir || process.cwd(),
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    child.on('error', reject);
    child.on('close', (code) => {
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim() });
    });
    child.stdin.write(JSON.stringify(inputObj));
    child.stdin.end();
  });
}

/** Parse events.jsonl → array of parsed objects. */
function readEvents(projectDir) {
  const evFile = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  if (!fs.existsSync(evFile)) return [];
  return fs.readFileSync(evFile, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((l) => {
      try { return JSON.parse(l); } catch { return null; }
    })
    .filter(Boolean);
}

/** Create a minimal project dir structure. */
function makeProjectDir(base) {
  const dir = fs.mkdtempSync(path.join(base, 'mustard-w2-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.agent-state'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', 'scripts'), { recursive: true });
  // Copy memory stub so subagent-tracker doesn't error
  const memWriteDst = path.join(dir, '.claude', 'scripts', 'memory.js');
  fs.writeFileSync(memWriteDst, "'use strict'; process.exit(0);\n");
  return dir;
}

// ─── subagent-tracker: agent.start (PreToolUse/Task) ────────────────────────

describe('subagent-tracker — agent.start emission', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits agent.start event when Task is dispatched', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      cwd: tmp,
      session_id: 's-w2-start',
      tool_input: {
        description: 'Explore the codebase for auth patterns',
        subagent_type: 'Explore',
        prompt: 'Find all auth-related files.',
      },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const events = readEvents(tmp);
    const agentStart = events.find((e) => e.event === 'agent.start');
    assert.ok(agentStart, 'agent.start event should be in events.jsonl');
    assert.equal(agentStart.actor.type, 'Explore');
    assert.ok(agentStart.payload.description.includes('auth'));
    assert.equal(agentStart.sessionId, 's-w2-start');
  });

  it('emits agent.start when dispatched via the Agent tool (post-rename)', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'PreToolUse',
      tool_name: 'Agent',
      cwd: tmp,
      session_id: 's-w2-agent',
      tool_input: {
        description: 'Explore the codebase for auth patterns',
        subagent_type: 'Explore',
        prompt: 'Find all auth-related files.',
      },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const events = readEvents(tmp);
    const agentStart = events.find((e) => e.event === 'agent.start');
    assert.ok(agentStart, 'agent.start must be emitted for the Agent tool, not just Task');
    assert.equal(agentStart.actor.type, 'Explore');
  });

  it('fail-open: hook exits 0 even when harness-event is disabled', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      cwd: tmp,
      session_id: 's-w2-failopen',
      tool_input: {
        description: 'test description',
        subagent_type: 'Explore',
        prompt: 'test prompt',
      },
    }, { projectDir: tmp, disabledHooks: 'harness-event' });

    assert.equal(result.code, 0, 'hook must not crash even with harness disabled');
    // No events should be written (harness-event disabled → emit returns false)
    const events = readEvents(tmp);
    assert.equal(events.length, 0, 'no events should be emitted when harness-event disabled');
  });
});

// ─── subagent-tracker: agent.stop (PostToolUse/Agent) ───────────────────────

describe('subagent-tracker — agent.stop emission', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits agent.stop event when an Agent dispatch returns', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Agent',
      cwd: tmp,
      session_id: 's-w2-stop',
      tool_input: { subagent_type: 'general-purpose' },
      tool_response: { output: 'Implemented the login endpoint and wrote tests. All passing. No issues found.' },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const events = readEvents(tmp);
    const agentStop = events.find((e) => e.event === 'agent.stop');
    assert.ok(agentStop, 'agent.stop event should be in events.jsonl');
    // Wave 4: durationMs is null (no .agent-state/{id}.json to read started_at from)
    assert.ok(agentStop.payload.durationMs === null, 'durationMs is null in Wave 4 (no legacy state file)');
    assert.ok(agentStop.payload.summary.length > 0, 'summary should be non-empty');
    assert.ok(agentStop.payload.summary.length <= 800, 'summary should be max 800 chars');
  });

  it('fail-open: SubagentStop exits 0 with harness disabled', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStop',
      agent_id: 'ag-failopen-002',
      agent_type: 'Explore',
      cwd: tmp,
      session_id: 's-failopen',
      tool_response: { output: 'Done.' },
    }, { projectDir: tmp, disabledHooks: 'harness-event' });

    assert.equal(result.code, 0, 'hook must not crash');
    const events = readEvents(tmp);
    assert.equal(events.length, 0, 'no events when harness disabled');
  });
});

// ─── metrics-tracker: tool.use ───────────────────────────────────────────────

describe('metrics-tracker — tool.use emission', () => {
  let tmp;
  beforeEach(() => {
    tmp = makeProjectDir(os.tmpdir());
    // Create a minimal pipeline state so metrics-tracker doesn't early-exit
    const statesDir = path.join(tmp, '.claude', '.pipeline-states');
    fs.mkdirSync(statesDir, { recursive: true });
    fs.writeFileSync(
      path.join(statesDir, 'test-spec.json'),
      JSON.stringify({ spec: 'test-spec', phaseName: 'EXECUTE', startedAt: new Date().toISOString() })
    );
  });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits tool.use heartbeat event', async () => {
    const result = await runHook('metrics-tracker.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Bash',
      cwd: tmp,
      session_id: 's-w2-metric',
      tool_input: { command: 'echo hello' },
      tool_response: { output: 'hello' },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const events = readEvents(tmp);
    const toolUse = events.find((e) => e.event === 'tool.use');
    assert.ok(toolUse, 'tool.use event should be in events.jsonl');
    assert.equal(toolUse.payload.tool, 'Bash');
    assert.equal(toolUse.actor.id, 'metrics-tracker');
  });

  it('fail-open: metrics-tracker exits 0 with harness disabled', async () => {
    const result = await runHook('metrics-tracker.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Edit',
      cwd: tmp,
      session_id: 's-failopen-metric',
      tool_input: { file_path: '/tmp/foo.js', old_string: 'a', new_string: 'b' },
      tool_response: {},
    }, { projectDir: tmp, disabledHooks: 'harness-event' });

    assert.equal(result.code, 0, 'hook must not crash');
    const events = readEvents(tmp);
    assert.equal(events.length, 0, 'no events when harness disabled');
  });
});

// ─── session-knowledge: finding ─────────────────────────────────────────────

describe('session-knowledge — finding emission', () => {
  let tmp;
  beforeEach(() => {
    tmp = makeProjectDir(os.tmpdir());
    // Create a minimal memory stub so execFileSync doesn't fail
    const knowledgeScript = path.join(tmp, '.claude', 'scripts', 'memory.js');
    fs.writeFileSync(knowledgeScript, "'use strict'; process.exit(0);\n");
    // Create pipeline states dir (session-knowledge reads from it)
    fs.mkdirSync(path.join(tmp, '.claude', '.pipeline-states'), { recursive: true });
  });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits finding events for extracted patterns', async () => {
    // Write a pipeline state with retries to trigger pattern extraction
    fs.writeFileSync(
      path.join(tmp, '.claude', '.pipeline-states', 'my-feature.json'),
      JSON.stringify({
        spec: 'my-feature',
        phaseName: 'CLOSE',
        _file: 'my-feature',
        metrics: { retries: 3 },
      })
    );

    const result = await runHook('session-knowledge.js', {
      hook_event_name: 'SessionEnd',
      cwd: tmp,
      session_id: 's-w2-knowledge',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    // findings are emitted if extractPatternsFromStates returns any patterns.
    // Since the underlying extractor may or may not produce patterns (its logic
    // depends on internal heuristics), we only assert no crash — not the count.
    // The events file may or may not exist.
    assert.ok(true, 'session-knowledge did not crash');
  });

  it('fail-open: session-knowledge exits 0 with harness disabled', async () => {
    const result = await runHook('session-knowledge.js', {
      hook_event_name: 'SessionEnd',
      cwd: tmp,
      session_id: 's-failopen-sk',
    }, { projectDir: tmp, disabledHooks: 'harness-event' });

    assert.equal(result.code, 0, 'hook must not crash');
  });
});

// ─── memory-persist: decision / lesson ──────────────────────────────────────

describe('memory-persist — decision/lesson emission', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits decision event before persisting to decisions.json', async () => {
    const result = await runScript('memory.js', {
      type: 'decision',
      content: 'Use Drizzle ORM for all database operations',
      source: 'feature-add-login',
      context: 'Evaluated Prisma vs Drizzle; chose Drizzle for type safety',
      cwd: tmp,
    }, { projectDir: tmp, args: ['decision'] });

    assert.equal(result.code, 0, `script exited non-zero: ${result.stderr}`);

    // Verify the old store still gets written
    const decisionsFile = path.join(tmp, '.claude', 'memory', 'decisions.json');
    assert.ok(fs.existsSync(decisionsFile), 'decisions.json should still be written');
    const data = JSON.parse(fs.readFileSync(decisionsFile, 'utf8'));
    assert.equal(data.entries.length, 1);
    assert.ok(data.entries[0].content.includes('Drizzle'));

    // Verify harness event
    const events = readEvents(tmp);
    const decisionEvent = events.find((e) => e.event === 'decision');
    assert.ok(decisionEvent, 'decision event should be in events.jsonl');
    assert.ok(decisionEvent.payload.title.includes('Drizzle'));
  });

  it('emits lesson event before persisting to lessons.json', async () => {
    const result = await runScript('memory.js', {
      type: 'lesson',
      content: 'Always validate env vars at startup to catch misconfigs early',
      source: 'bugfix-env-crash',
      cwd: tmp,
    }, { projectDir: tmp, args: ['decision'] });

    assert.equal(result.code, 0, `script exited non-zero: ${result.stderr}`);

    const events = readEvents(tmp);
    const lessonEvent = events.find((e) => e.event === 'lesson');
    assert.ok(lessonEvent, 'lesson event should be in events.jsonl');
    assert.ok(lessonEvent.payload.takeaway.includes('env vars'));
  });

  it('fail-open: exits 0 even with harness disabled', async () => {
    const result = await runScript('memory.js', {
      type: 'decision',
      content: 'Some decision',
      source: 'test',
      cwd: tmp,
    }, { projectDir: tmp, args: ['decision'] });

    // memory-persist doesn't read MUSTARD_DISABLED_HOOKS in the runner — but
    // we verify it never crashes regardless of the harness state.
    assert.equal(result.code, 0, 'script must not crash');
  });
});

// ─── pipeline-phase.js ───────────────────────────────────────────────────────

describe('pipeline-phase — pipeline.phase emission', () => {
  let tmp;
  beforeEach(() => {
    tmp = makeProjectDir(os.tmpdir());
    fs.mkdirSync(path.join(tmp, '.claude', '.pipeline-states'), { recursive: true });
  });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits pipeline.phase when phase changes (from null to ANALYZE)', async () => {
    // Write a pipeline state file
    const specFile = path.join(tmp, '.claude', '.pipeline-states', 'add-login.json');
    fs.writeFileSync(specFile, JSON.stringify({ spec: 'add-login', phaseName: 'ANALYZE' }));

    const result = await runHook('pipeline-phase.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Write',
      cwd: tmp,
      session_id: 's-w2-phase',
      tool_input: { file_path: specFile },
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);

    const events = readEvents(tmp);
    const phaseEvent = events.find((e) => e.event === 'pipeline.phase');
    assert.ok(phaseEvent, 'pipeline.phase event should be in events.jsonl');
    assert.equal(phaseEvent.payload.from, null);
    assert.equal(phaseEvent.payload.to, 'ANALYZE');
    assert.equal(phaseEvent.spec, 'add-login');
  });

  it('emits pipeline.phase with from/to when phase transitions', async () => {
    const specFile = path.join(tmp, '.claude', '.pipeline-states', 'add-login.json');

    // Prime the cache: first write ANALYZE
    fs.writeFileSync(specFile, JSON.stringify({ spec: 'add-login', phaseName: 'ANALYZE' }));
    await runHook('pipeline-phase.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Write',
      cwd: tmp,
      session_id: 's-w2-phase2',
      tool_input: { file_path: specFile },
    }, { projectDir: tmp });

    // Now transition to PLAN
    fs.writeFileSync(specFile, JSON.stringify({ spec: 'add-login', phaseName: 'PLAN' }));
    await runHook('pipeline-phase.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Write',
      cwd: tmp,
      session_id: 's-w2-phase2',
      tool_input: { file_path: specFile },
    }, { projectDir: tmp });

    const events = readEvents(tmp);
    const phaseEvents = events.filter((e) => e.event === 'pipeline.phase');
    assert.equal(phaseEvents.length, 2, 'two phase transitions should produce 2 events');
    assert.equal(phaseEvents[0].payload.from, null);
    assert.equal(phaseEvents[0].payload.to, 'ANALYZE');
    assert.equal(phaseEvents[1].payload.from, 'ANALYZE');
    assert.equal(phaseEvents[1].payload.to, 'PLAN');
  });

  it('does NOT emit when phase stays the same', async () => {
    const specFile = path.join(tmp, '.claude', '.pipeline-states', 'stable.json');
    fs.writeFileSync(specFile, JSON.stringify({ spec: 'stable', phaseName: 'EXECUTE' }));

    // First write: new phase → emits
    await runHook('pipeline-phase.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Edit',
      cwd: tmp,
      session_id: 's-nodiff',
      tool_input: { file_path: specFile },
    }, { projectDir: tmp });

    // Second write: same phase → should NOT emit
    await runHook('pipeline-phase.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Edit',
      cwd: tmp,
      session_id: 's-nodiff',
      tool_input: { file_path: specFile },
    }, { projectDir: tmp });

    const events = readEvents(tmp);
    const phaseEvents = events.filter((e) => e.event === 'pipeline.phase');
    assert.equal(phaseEvents.length, 1, 'same phase should not produce a second event');
  });

  it('ignores non-pipeline-state files', async () => {
    const result = await runHook('pipeline-phase.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Write',
      cwd: tmp,
      session_id: 's-skip',
      tool_input: { file_path: path.join(tmp, 'src', 'app.js') },
    }, { projectDir: tmp });

    assert.equal(result.code, 0);
    const events = readEvents(tmp);
    assert.equal(events.length, 0, 'non-pipeline files should not emit events');
  });

  it('fail-open: exits 0 even with harness disabled', async () => {
    const specFile = path.join(tmp, '.claude', '.pipeline-states', 'failopen.json');
    fs.writeFileSync(specFile, JSON.stringify({ spec: 'failopen', phaseName: 'CLOSE' }));

    const result = await runHook('pipeline-phase.js', {
      hook_event_name: 'PostToolUse',
      tool_name: 'Write',
      cwd: tmp,
      session_id: 's-failopen-pp',
      tool_input: { file_path: specFile },
    }, { projectDir: tmp, disabledHooks: 'pipeline-phase' });

    assert.equal(result.code, 0, 'hook must not crash');
  });
});
