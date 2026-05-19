#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 11 — Anti-slope hooks Tests
 *
 * Covers:
 * 1.  duplication-check: new class similar to registry entry → warn emitted
 * 2.  duplication-check: completely different symbol → no warn
 * 3.  duplication-check strict: similar symbol → block (decision=block)
 * 4.  convention-check: knowledge with "Repository always in /Repositories" → wrong path → warn
 * 5.  convention-check: correct path → no warn
 * 6.  convention-check: knowledge entry not extractable → no warn, no error
 * 7.  buildSlopeReport: counts warns correctly across events
 * 8.  duplication-check fail-open: corrupted entity-registry → exit 0
 * 9.  convention-check fail-open: invalid knowledge.json → exit 0
 *
 * Run with: bun test templates/hooks/__tests__/harness-wave11.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const DUP_CHECK = path.join(HOOKS_DIR, 'duplication-check.js');
const CONV_CHECK = path.join(HOOKS_DIR, 'convention-check.js');
const HARNESS_VIEWS = path.join(SCRIPTS_DIR, 'event-projections.js');

// ── Helpers ───────────────────────────────────────────────────────────────────

function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w11-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness', 'sessions'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.pipeline-states'), { recursive: true });
  return dir;
}

function cleanDir(dir) {
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch (_) {}
}

function writeEntityRegistry(projectDir, entities) {
  const registryPath = path.join(projectDir, '.claude', 'entity-registry.json');
  fs.writeFileSync(registryPath, JSON.stringify(entities, null, 2), 'utf8');
}

function writeKnowledge(projectDir, entries) {
  const kPath = path.join(projectDir, '.claude', 'knowledge.json');
  fs.writeFileSync(kPath, JSON.stringify(entries, null, 2), 'utf8');
}

function writeHarnessEvents(projectDir, events) {
  const eventsFile = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  const lines = events.map(e => JSON.stringify(e)).join('\n') + '\n';
  fs.writeFileSync(eventsFile, lines, 'utf8');
}

/** Run a PostToolUse hook with JSON stdin */
function runHook(hookPath, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const env = Object.assign({}, process.env);
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
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });
    child.stdin.write(JSON.stringify(inputObj));
    child.stdin.end();
  });
}

function makeWriteInput(projectDir, filePath, content) {
  return {
    tool: 'Write',
    tool_input: { file_path: filePath, content },
    cwd: projectDir,
  };
}

function makeEditInput(projectDir, filePath, newString) {
  return {
    tool: 'Edit',
    tool_input: { file_path: filePath, new_string: newString },
    cwd: projectDir,
  };
}

function makeHarnessEvent(eventName, payload, overrides = {}) {
  return Object.assign({
    v: 1,
    ts: new Date().toISOString(),
    sessionId: 's-test',
    wave: 0,
    actor: { kind: 'hook' },
    event: eventName,
    payload,
  }, overrides);
}

// ── Test 1: duplication-check warns on similar symbol ─────────────────────────

describe('Wave 11 — duplication-check: similar symbol → warn', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('emits warn to stderr when class name is similar to registry entry', async () => {
    // Registry has AuthService — we write AuthServices (Levenshtein similarity >= 0.85)
    writeEntityRegistry(tmp, {
      AuthService: { name: 'AuthService', file: 'src/Services/AuthService.ts' },
    });

    const filePath = path.join(tmp, 'src', 'Services', 'AuthServices.ts');
    const content = 'export class AuthServices {\n  login() {}\n}\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(DUP_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_DUPLICATION_MODE: 'warn' },
    });

    assert.equal(result.code, 0, 'hook must exit 0');
    assert.ok(
      result.stderr.includes('[duplication-check]') || result.stderr.includes('AuthService'),
      `expected duplication warn in stderr, got: ${result.stderr}`
    );
  });
});

// ── Test 2: duplication-check no warn on different symbol ─────────────────────

describe('Wave 11 — duplication-check: different symbol → no warn', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('does not emit warn when symbol is unrelated to registry', async () => {
    writeEntityRegistry(tmp, {
      AuthService: { name: 'AuthService', file: 'src/Services/AuthService.ts' },
    });

    const filePath = path.join(tmp, 'src', 'Services', 'CompletelyDifferent.ts');
    const content = 'export class CompletelyDifferent {\n  process() {}\n}\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(DUP_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_DUPLICATION_MODE: 'warn' },
    });

    assert.equal(result.code, 0, 'hook must exit 0');
    assert.ok(
      !result.stderr.includes('[duplication-check]'),
      `expected NO duplication warn, got stderr: ${result.stderr}`
    );
    // stdout should be empty or not a block decision
    if (result.parsed) {
      assert.notEqual(result.parsed.decision, 'block',
        `should not block for unrelated symbol, got: ${result.parsed.decision}`);
    }
  });
});

// ── Test 3: duplication-check strict → block ──────────────────────────────────

describe('Wave 11 — duplication-check strict: similar symbol → block', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('emits block decision in strict mode for similar symbol', async () => {
    writeEntityRegistry(tmp, {
      AuthService: { name: 'AuthService', file: 'src/Services/AuthService.ts' },
    });

    const filePath = path.join(tmp, 'src', 'Services', 'AuthServices.ts');
    const content = 'export class AuthServices {\n  login() {}\n}\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(DUP_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_DUPLICATION_MODE: 'strict' },
    });

    assert.equal(result.code, 0, 'hook must exit 0');
    // In strict mode, hook writes JSON with decision: block to stdout
    assert.ok(result.parsed, `expected JSON on stdout in strict mode, got: ${result.stdout}`);
    assert.equal(result.parsed.decision, 'block',
      `expected decision=block, got: ${result.parsed.decision}`);
    assert.ok(
      result.parsed.reason && result.parsed.reason.includes('duplication-check'),
      `reason should include [duplication-check]: ${result.parsed.reason}`
    );
  });
});

// ── Test 4: convention-check warns on wrong path ──────────────────────────────

describe('Wave 11 — convention-check: wrong path → warn', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('emits warn when Repository file is not in /Repositories/ directory', async () => {
    writeKnowledge(tmp, [
      {
        type: 'convention',
        confidence: 0.9,
        content: 'Repository always in /Repositories',
      },
    ]);

    // File is in /Services/ not /Repositories/
    const filePath = path.join(tmp, 'src', 'Services', 'FooRepository.cs');
    const content = 'public class FooRepository {}\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(CONV_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_CONVENTION_MODE: 'warn' },
    });

    assert.equal(result.code, 0, 'hook must exit 0');
    assert.ok(
      result.stderr.includes('[convention-check]'),
      `expected convention warn in stderr, got: ${result.stderr}`
    );
  });
});

// ── Test 5: convention-check no warn on correct path ─────────────────────────

describe('Wave 11 — convention-check: correct path → no warn', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('does not emit warn when Repository file IS in /Repositories/ directory', async () => {
    writeKnowledge(tmp, [
      {
        type: 'convention',
        confidence: 0.9,
        content: 'Repository always in /Repositories',
      },
    ]);

    // File IS in /Repositories/
    const filePath = path.join(tmp, 'src', 'Repositories', 'FooRepository.cs');
    const content = 'public class FooRepository {}\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(CONV_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_CONVENTION_MODE: 'warn' },
    });

    assert.equal(result.code, 0, 'hook must exit 0');
    // Should not have a VIOLATION warn — may have the "N active rule(s)" diagnostic
    const hasViolationWarn = result.stderr.includes('Convention violation') ||
      result.stderr.includes('violation');
    assert.ok(!hasViolationWarn,
      `expected NO convention violation warn, got stderr: ${result.stderr}`);
  });
});

// ── Test 6: convention-check non-extractable entry → no warn ─────────────────

describe('Wave 11 — convention-check: non-extractable knowledge entry → no warn, no error', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('silently ignores entries that cannot yield a rule', async () => {
    writeKnowledge(tmp, [
      {
        type: 'convention',
        confidence: 0.95,
        // Vague phrase that the hook cannot parse into a rule
        content: 'Always write clean code with good practices',
      },
    ]);

    const filePath = path.join(tmp, 'src', 'Anything', 'SomeFile.ts');
    const content = 'export const x = 1;\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(CONV_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_CONVENTION_MODE: 'warn' },
    });

    // Should exit 0, no error, no violation warn
    assert.equal(result.code, 0, 'hook must exit 0');
    const hasViolationWarn = result.stderr.includes('Convention violation') ||
      result.stderr.includes('violation');
    assert.ok(!hasViolationWarn,
      `expected no violation warn for non-extractable entry, stderr: ${result.stderr}`);
  });
});

// ── Test 7: buildSlopeReport counts warns correctly ──────────────────────────

describe('Wave 11 — buildSlopeReport: counts anti-slope warns correctly', () => {
  it('counts duplication.warn and convention.warn from events', () => {
    const views = require('../../scripts/event-projections.js');

    const events = [
      makeHarnessEvent('duplication.warn', { file: 'src/a.ts', symbols: ['AuthServices'] }),
      makeHarnessEvent('duplication.warn', { file: 'src/b.ts', symbols: ['UserServices'] }),
      makeHarnessEvent('convention.warn', { file: 'src/c.ts', violations: [] }),
      makeHarnessEvent('agent.start', { description: 'not a slope event' }),
    ];

    const report = views.buildSlopeReport(events, { lookback_sessions: 1 });

    assert.equal(report.duplication, 2, `expected 2 duplication warns, got: ${report.duplication}`);
    assert.equal(report.convention, 1, `expected 1 convention warn, got: ${report.convention}`);
    assert.ok(Array.isArray(report.top_paths), 'top_paths must be array');
  });

  it('returns zeros when no slope events present', () => {
    const views = require('../../scripts/event-projections.js');

    const events = [
      makeHarnessEvent('agent.start', {}),
      makeHarnessEvent('tool.use', {}),
    ];

    const report = views.buildSlopeReport(events, { lookback_sessions: 1 });

    assert.equal(report.duplication, 0);
    assert.equal(report.convention, 0);
    assert.deepEqual(report.top_paths, []);
  });
});

// ── Test 11: duplication-check fail-open on corrupted registry ────────────────

describe('Wave 11 — duplication-check: fail-open on corrupted entity-registry', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('exits 0 and does not crash on invalid JSON in entity-registry.json', async () => {
    // Write corrupted JSON
    fs.writeFileSync(
      path.join(tmp, '.claude', 'entity-registry.json'),
      '{ this is not valid json !!!',
      'utf8'
    );

    const filePath = path.join(tmp, 'src', 'MyService.ts');
    const content = 'export class MyService {}\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(DUP_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_DUPLICATION_MODE: 'warn' },
    });

    // Must exit 0 (fail-open)
    assert.equal(result.code, 0, `hook must exit 0 on corrupted registry, code: ${result.code}`);
    // Must not produce a block decision
    if (result.parsed) {
      assert.notEqual(result.parsed.decision, 'block',
        'must not block on corrupted registry');
    }
  });
});

// ── Test 12: convention-check fail-open on invalid knowledge.json ─────────────

describe('Wave 11 — convention-check: fail-open on invalid knowledge.json', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('exits 0 and does not crash on invalid knowledge.json', async () => {
    // Write invalid JSON
    fs.writeFileSync(
      path.join(tmp, '.claude', 'knowledge.json'),
      'NOT JSON AT ALL',
      'utf8'
    );

    const filePath = path.join(tmp, 'src', 'FooRepository.ts');
    const content = 'export class FooRepository {}\n';
    const input = makeWriteInput(tmp, filePath, content);

    const result = await runHook(CONV_CHECK, input, {
      projectDir: tmp,
      env: { MUSTARD_CONVENTION_MODE: 'warn' },
    });

    assert.equal(result.code, 0, `hook must exit 0 on invalid knowledge.json, code: ${result.code}`);
    if (result.parsed) {
      assert.notEqual(result.parsed.decision, 'block',
        'must not block on invalid knowledge.json');
    }
  });
});

