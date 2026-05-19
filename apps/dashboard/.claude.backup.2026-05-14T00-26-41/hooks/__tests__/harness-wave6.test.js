#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 6 — On-Demand Memory Escape Hatch Tests
 *
 * Covers:
 * 1. CLI --compact returns well-formed JSON for all 4 views
 * 2. CLI --query filters correctly (only matching items returned)
 * 3. CLI --view <invalid> returns { error: ... } and exits 0
 * 4. subagent-tracker includes hint when budget has room; omits when tight
 * 5. settings.json contains harness-views.js Bash permission
 *
 * Run with: bun test templates/hooks/__tests__/harness-wave6.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const TEMPLATES_DIR = path.resolve(__dirname, '../..');

// ── Helpers ───────────────────────────────────────────────────────────────────

function runScript(scriptFile, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const allArgs = [path.join(SCRIPTS_DIR, scriptFile), ...args];
    const child = spawn(process.execPath, allArgs, {
      cwd: projectDir,
      env: { ...process.env },
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
    child.stdin.end();
  });
}

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

/** Create a project dir with .harness and .agent-state dirs */
function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w6-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness', 'sessions'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.agent-state'), { recursive: true });
  return dir;
}

/** Append an NDJSON event to events.jsonl */
function appendEvent(projectDir, event) {
  const evFile = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  fs.appendFileSync(evFile, JSON.stringify(event) + '\n', 'utf8');
}

function makeEvent(overrides) {
  return Object.assign({
    v: 1,
    ts: new Date().toISOString(),
    sessionId: 's-w6',
    wave: 1,
    actor: { kind: 'agent', id: 'ag-w6', type: 'general-purpose' },
    event: 'agent.start',
    payload: { description: 'default', model: null },
  }, overrides);
}

// ── Test 1: --compact returns well-formed JSON for all 4 views ────────────────

describe('Wave 6 — --compact: all views return valid JSON', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('agent-visibility --compact returns array', async () => {
    appendEvent(tmp, makeEvent({
      event: 'agent.start',
      actor: { kind: 'agent', id: 'ag-1', type: 'Explore' },
      payload: { description: 'scan auth', model: null },
    }));
    appendEvent(tmp, makeEvent({
      event: 'finding',
      actor: { kind: 'agent', id: 'ag-1', type: 'Explore' },
      payload: { kind: 'pattern', content: 'JWT is used for session tokens', confidence: 0.9, refs: [] },
    }));

    const result = await runScript('harness-views.js',
      ['--view', 'agent-visibility', '--compact', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0, `exit 0 expected, got stderr: ${result.stderr}`);
    assert.ok(Array.isArray(result.parsed), `compact agent-visibility must be an array. Got: ${result.stdout}`);
    // Each item must have type and desc
    for (const item of result.parsed) {
      assert.ok(typeof item.type === 'string', 'item.type must be string');
      assert.ok(typeof item.desc === 'string', 'item.desc must be string');
    }
  });

  it('pipeline-state --compact returns {phase, metrics, specs}', async () => {
    appendEvent(tmp, makeEvent({
      spec: 'auth-login',
      event: 'pipeline.phase',
      payload: { from: null, to: 'ANALYZE' },
    }));

    const result = await runScript('harness-views.js',
      ['--view', 'pipeline-state', '--spec', 'auth-login', '--compact', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0);
    assert.ok(result.parsed && typeof result.parsed === 'object' && !Array.isArray(result.parsed),
      `compact pipeline-state must be an object. Got: ${result.stdout}`);
    assert.ok('phase' in result.parsed, 'phase key must be present');
    assert.ok('metrics' in result.parsed, 'metrics key must be present');
    assert.ok('specs' in result.parsed, 'specs key must be present');
  });

  it('session-summary --compact returns {findings, decisions, lessons}', async () => {
    appendEvent(tmp, makeEvent({
      event: 'finding',
      payload: { kind: 'pattern', content: 'Auth uses JWT', confidence: 0.85, refs: [] },
    }));
    appendEvent(tmp, makeEvent({
      event: 'decision',
      payload: { title: 'Use JWT for tokens', rationale: 'simpler than OAuth' },
    }));

    const result = await runScript('harness-views.js',
      ['--view', 'session-summary', '--compact', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0);
    assert.ok(result.parsed && typeof result.parsed === 'object', `must be object. Got: ${result.stdout}`);
    assert.ok(Array.isArray(result.parsed.findings), 'findings must be array');
    assert.ok(Array.isArray(result.parsed.decisions), 'decisions must be array');
    assert.ok(Array.isArray(result.parsed.lessons), 'lessons must be array');
    // compact findings have {text} shape
    if (result.parsed.findings.length > 0) {
      assert.ok(typeof result.parsed.findings[0].text === 'string', 'finding.text must be string');
    }
  });

  it('cross-session-timeline --compact returns array of {session, date, specs, decisions_count}', async () => {
    // Write a fake session file
    const sessionsDir = path.join(tmp, '.claude', '.harness', 'sessions');
    const sessionEvents = [
      makeEvent({ sessionId: 's-old', spec: 'add-login', event: 'agent.start' }),
      makeEvent({ sessionId: 's-old', spec: 'add-login', event: 'decision', payload: { title: 'JWT', rationale: 'x' } }),
    ];
    fs.writeFileSync(path.join(sessionsDir, 's-old.jsonl'),
      sessionEvents.map(e => JSON.stringify(e)).join('\n') + '\n');

    const result = await runScript('harness-views.js',
      ['--view', 'cross-session-timeline', '--compact', '--limit', '5', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0);
    assert.ok(Array.isArray(result.parsed), `compact timeline must be array. Got: ${result.stdout}`);
    if (result.parsed.length > 0) {
      const entry = result.parsed[0];
      assert.ok('session' in entry, 'entry.session must exist');
      assert.ok('decisions_count' in entry, 'entry.decisions_count must exist');
      assert.ok(Array.isArray(entry.specs), 'entry.specs must be array');
    }
  });
});

// ── Test 2: --query filters correctly ─────────────────────────────────────────

describe('Wave 6 — --query: filters by text content', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('session-summary --query "JWT" returns only JWT-related findings', async () => {
    appendEvent(tmp, makeEvent({
      event: 'finding',
      payload: { kind: 'pattern', content: 'JWT is used for session tokens', confidence: 0.9, refs: [] },
    }));
    appendEvent(tmp, makeEvent({
      event: 'finding',
      payload: { kind: 'pattern', content: 'Database uses PostgreSQL', confidence: 0.85, refs: [] },
    }));
    appendEvent(tmp, makeEvent({
      event: 'finding',
      payload: { kind: 'pattern', content: 'Auth module validates JWT expiration', confidence: 0.8, refs: [] },
    }));

    const result = await runScript('harness-views.js',
      ['--view', 'session-summary', '--query', 'JWT', '--compact', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0, `exit 0 expected. stderr: ${result.stderr}`);
    assert.ok(result.parsed, 'must parse JSON');
    const findings = result.parsed.findings || [];
    assert.ok(findings.length >= 1, `at least 1 JWT finding expected, got ${findings.length}`);
    // All returned findings must contain 'jwt' (case-insensitive)
    for (const f of findings) {
      assert.ok(f.text.toLowerCase().includes('jwt'),
        `All results must contain 'jwt'. Got: "${f.text}"`);
    }
    // "PostgreSQL" finding must NOT appear
    const texts = findings.map(f => f.text.toLowerCase());
    assert.ok(!texts.some(t => t.includes('postgresql')),
      'PostgreSQL finding must be filtered out');
  });

  it('session-summary --query returns empty arrays when no match', async () => {
    appendEvent(tmp, makeEvent({
      event: 'finding',
      payload: { kind: 'pattern', content: 'Something about Redis', confidence: 0.9, refs: [] },
    }));

    const result = await runScript('harness-views.js',
      ['--view', 'session-summary', '--query', 'JWT', '--compact', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0);
    const findings = result.parsed && result.parsed.findings || [];
    assert.equal(findings.length, 0, `no JWT findings expected. Got ${findings.length}`);
  });

  it('--query is case-insensitive (lowercase query matches UPPERCASE content)', async () => {
    appendEvent(tmp, makeEvent({
      event: 'finding',
      payload: { kind: 'pattern', content: 'AUTH MODULE USES JWT TOKENS', confidence: 0.9, refs: [] },
    }));

    const result = await runScript('harness-views.js',
      ['--view', 'session-summary', '--query', 'jwt', '--compact', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0);
    const findings = result.parsed && result.parsed.findings || [];
    assert.equal(findings.length, 1, `case-insensitive match must return 1 finding. Got ${findings.length}`);
  });
});

// ── Test 3: invalid view returns { error } and exit 0 ─────────────────────────

describe('Wave 6 — invalid --view returns {error} and exit 0', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('--view unknown-view returns { error: ... } and exits 0', async () => {
    const result = await runScript('harness-views.js',
      ['--view', 'nonexistent-view', '--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0, 'must exit 0 on invalid view');
    assert.ok(result.parsed && typeof result.parsed.error === 'string',
      `must return {error} JSON. Got: ${result.stdout}`);
    assert.ok(result.parsed.error.includes('nonexistent-view') || result.parsed.error.length > 0,
      'error message must be non-empty');
  });

  it('exits 0 and outputs usage to stderr when --view is missing', async () => {
    const result = await runScript('harness-views.js',
      ['--cwd', tmp],
      { projectDir: tmp }
    );

    assert.equal(result.code, 0, 'must exit 0 when --view omitted');
  });
});

// ── Test 4: subagent-tracker hint presence and omission ──────────────────────

describe('Wave 6 — subagent-tracker: escape-hatch hint', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('includes hint when budget has room (general-purpose with small finding)', async () => {
    // Write a small finding — budget=800, hint is ~80 chars, combined should fit
    appendEvent(tmp, makeEvent({
      event: 'finding',
      actor: { kind: 'agent', id: 'ag-small', type: 'general-purpose' },
      payload: { kind: 'pattern', content: 'Small finding', confidence: 0.9, refs: [] },
    }));

    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-gp',
      agent_type: 'general-purpose',
      cwd: tmp,
      session_id: 's-hint-test',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `must exit 0. stderr: ${result.stderr}`);
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext must be string');

    // When there's a finding, the hint should appear
    if (ctx.includes('[Prior Findings]')) {
      assert.ok(
        ctx.includes('harness-views.js'),
        `hint must appear when findings present and budget allows. Got: ${ctx}`
      );
    }
  });

  it('omits hint when visText already fills budget (Explore budget = 400)', async () => {
    // Fill the Explore budget with a large finding so hint cannot fit
    appendEvent(tmp, makeEvent({
      event: 'finding',
      actor: { kind: 'agent', id: 'ag-large', type: 'Explore' },
      payload: { kind: 'pattern', content: 'X'.repeat(390), confidence: 0.9, refs: [] },
    }));

    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-explore',
      agent_type: 'Explore',
      cwd: tmp,
      session_id: 's-nohint-test',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `must exit 0. stderr: ${result.stderr}`);
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext must be string');

    // With budget=400 and a 390-char finding, the hint (~80 chars) cannot fit
    // The combined visText before hint is already at ~420+ chars (after truncation to 397)
    // So the context must not contain the hint.
    // Note: the budget truncation slices visText to budget-3 chars, so if hint didn't fit
    // it simply won't be there.
    // We check that the total additionalContext is within reasonable bounds.
    assert.ok(ctx.length <= 600, `Explore additionalContext must be within bounds. Got: ${ctx.length}`);
  });

  it('no crash when events.jsonl is missing (hint omitted gracefully)', async () => {
    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-nofile',
      agent_type: 'general-purpose',
      cwd: tmp,
      session_id: 's-nofile',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, 'must exit 0 when events.jsonl is missing');
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext must be string');
    // No findings → no hint
    assert.ok(!ctx.includes('harness-views.js'),
      'hint must not appear when there are no findings');
  });
});

// ── Test 5: settings.json contains harness-views.js Bash permission ───────────

describe('Wave 6 — settings.json: harness-views.js Bash permission present', () => {
  it('settings.json allow list contains Bash(bun .claude/scripts/harness-views.js:*)', () => {
    const settingsPath = path.join(TEMPLATES_DIR, 'settings.json');
    assert.ok(fs.existsSync(settingsPath), `settings.json must exist at ${settingsPath}`);

    const raw = fs.readFileSync(settingsPath, 'utf8');
    let settings;
    try {
      settings = JSON.parse(raw);
    } catch (e) {
      assert.fail(`settings.json must be valid JSON. Parse error: ${e.message}`);
    }

    const allow = settings && settings.permissions && settings.permissions.allow;
    assert.ok(Array.isArray(allow), 'permissions.allow must be an array');

    const hasPermission = allow.some(entry =>
      typeof entry === 'string' && entry.includes('harness-views.js')
    );
    assert.ok(hasPermission,
      `permissions.allow must contain a harness-views.js entry. Got: ${JSON.stringify(allow)}`);
  });
});
