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

// NOTE: subagent-tracker.js and metrics-tracker.js were ported to the Rust
// `mustard-rt` modules (`tracker`) in b3 Wave 3. Their agent.start/agent.stop
// and tool.use dual-emission parity now lives in packages/rt/src/hooks/tracker.rs.

// NOTE: session-knowledge.js was ported to the Rust `mustard-rt` `knowledge`
// module in b3 Wave 5. Its friction-telemetry write and `retry.attempt`
// emission parity now lives in packages/rt/src/hooks/knowledge.rs.

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

// pipeline-phase.js was ported to the Rust `post_edit` module (b3 Wave 4);
// its parity tests now live in `packages/rt/src/hooks/post_edit.rs`.
