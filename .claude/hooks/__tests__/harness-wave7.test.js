#!/usr/bin/env node
'use strict';
/**
 * Harness Wave 7 — Spec Hierarchy (parent/child) Tests
 *
 * Covers:
 * 1. spec-link.js CLI creates parent + child state records correctly
 * 2. spec-link.js is idempotent (re-link does not duplicate)
 * 3. buildSpecTree constructs hierarchy correctly
 * 4. buildSpecTree does not recurse infinitely (cycle detection)
 * 5. buildCrossSessionTimeline marks epic when children_specs exist
 * 6. spec.link event is append-only in the harness log
 * 7. buildSpecTree returns { error: 'spec not found' } for unknown rootSpec
 * 8. COORDINATE is accepted as a valid phase value in pipeline-state
 *
 * Run with: node --test templates/hooks/__tests__/harness-wave7.test.js
 */

const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const SPEC_LINK = path.join(SCRIPTS_DIR, 'spec-link.js');
const HARNESS_VIEWS = path.join(SCRIPTS_DIR, 'harness-views.js');

// ── Helpers ───────────────────────────────────────────────────────────────────

function runScript(scriptPath, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const child = spawn(process.execPath, [scriptPath, ...args], {
      cwd: projectDir,
      env: { ...process.env, MUSTARD_DISABLED_HOOKS: 'all' },
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
    if (opts.stdin) child.stdin.write(opts.stdin);
    child.stdin.end();
  });
}

/** Create a project dir with .harness and .pipeline-states dirs */
function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w7-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness', 'sessions'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.pipeline-states'), { recursive: true });
  return dir;
}

function readState(projectDir, specName) {
  const f = path.join(projectDir, '.claude', '.pipeline-states', specName + '.json');
  return JSON.parse(fs.readFileSync(f, 'utf8'));
}

function writeState(projectDir, specName, obj) {
  const f = path.join(projectDir, '.claude', '.pipeline-states', specName + '.json');
  fs.writeFileSync(f, JSON.stringify(obj, null, 2), 'utf8');
}

function readEvents(projectDir) {
  const f = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  if (!fs.existsSync(f)) return [];
  return fs.readFileSync(f, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map(l => { try { return JSON.parse(l); } catch (_) { return null; } })
    .filter(Boolean);
}

// ── Test 1: spec-link.js CLI creates parent + child state records ─────────────

describe('Wave 7 — spec-link CLI: creates parent+child states', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('creates children_specs in parent and parent_spec in child', async () => {
    const result = await runScript(SPEC_LINK, [
      '--parent', 'auth-oauth',
      '--child', 'auth-oauth-callback',
      '--reason', 'separar endpoint',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, `exit 0 expected, stderr: ${result.stderr}`);
    assert.ok(result.parsed, `should have parsed JSON output: ${result.stdout}`);
    assert.equal(result.parsed.ok, true);

    const parentState = readState(tmp, 'auth-oauth');
    assert.ok(Array.isArray(parentState.children_specs), 'parent must have children_specs array');
    assert.ok(parentState.children_specs.includes('auth-oauth-callback'), 'parent must list child');
    assert.equal(parentState.parent_spec, null, 'parent parent_spec must be null');

    const childState = readState(tmp, 'auth-oauth-callback');
    assert.equal(childState.parent_spec, 'auth-oauth', 'child parent_spec must point to parent');
    assert.ok(Array.isArray(childState.children_specs), 'child must have children_specs array');
  });

  it('appends spec.link event to harness log', async () => {
    await runScript(SPEC_LINK, [
      '--parent', 'auth-oauth',
      '--child', 'auth-oauth-callback',
      '--reason', 'split callback',
      '--cwd', tmp,
    ], { projectDir: tmp });

    const events = readEvents(tmp);
    const linkEvents = events.filter(e => e.event === 'spec.link');
    assert.equal(linkEvents.length, 1, 'exactly 1 spec.link event expected');
    const ev = linkEvents[0];
    assert.equal(ev.payload.parent, 'auth-oauth');
    assert.equal(ev.payload.child, 'auth-oauth-callback');
    assert.equal(ev.payload.reason, 'split callback');
  });
});

// ── Test 2: spec-link.js is idempotent ───────────────────────────────────────

describe('Wave 7 — spec-link: idempotent (no duplicate children)', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('calling spec-link twice does not duplicate child in parent', async () => {
    const args = [
      '--parent', 'auth-oauth',
      '--child', 'auth-oauth-callback',
      '--reason', 'split',
      '--cwd', tmp,
    ];
    await runScript(SPEC_LINK, args, { projectDir: tmp });
    await runScript(SPEC_LINK, args, { projectDir: tmp });

    const parentState = readState(tmp, 'auth-oauth');
    const count = parentState.children_specs.filter(c => c === 'auth-oauth-callback').length;
    assert.equal(count, 1, 'child should appear exactly once in parent.children_specs');
  });

  it('spec.link events are each emitted (append-only log — 2 calls = 2 events)', async () => {
    const args = [
      '--parent', 'auth-oauth',
      '--child', 'auth-oauth-callback',
      '--reason', 'split',
      '--cwd', tmp,
    ];
    await runScript(SPEC_LINK, args, { projectDir: tmp });
    await runScript(SPEC_LINK, args, { projectDir: tmp });

    const events = readEvents(tmp);
    const linkEvents = events.filter(e => e.event === 'spec.link');
    // Append-only: each call appends an event even if idempotent on state files
    assert.equal(linkEvents.length, 2, 'log should have 2 spec.link events (append-only)');
  });
});

// ── Test 3: buildSpecTree constructs hierarchy correctly ──────────────────────

describe('Wave 7 — buildSpecTree: correct hierarchy from disk state', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns tree with 2 children', async () => {
    // Setup states on disk
    writeState(tmp, 'auth-oauth', {
      spec: 'auth-oauth', phase: 'COORDINATE',
      parent_spec: null, children_specs: ['auth-oauth-callback', 'auth-oauth-refresh'],
    });
    writeState(tmp, 'auth-oauth-callback', {
      spec: 'auth-oauth-callback', phase: 'CLOSE',
      parent_spec: 'auth-oauth', children_specs: [],
    });
    writeState(tmp, 'auth-oauth-refresh', {
      spec: 'auth-oauth-refresh', phase: 'ANALYZE',
      parent_spec: 'auth-oauth', children_specs: [],
    });

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'spec-tree',
      '--spec', 'auth-oauth',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, `exit 0 expected, stderr: ${result.stderr}`);
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    assert.equal(result.parsed.spec, 'auth-oauth');
    assert.ok(Array.isArray(result.parsed.children), 'children must be array');
    assert.equal(result.parsed.children.length, 2, 'must have 2 children');

    const childNames = result.parsed.children.map(c => c.spec).sort();
    assert.deepEqual(childNames, ['auth-oauth-callback', 'auth-oauth-refresh']);
  });

  it('--compact returns only spec + phase + children (no extra fields)', async () => {
    writeState(tmp, 'auth-oauth', {
      spec: 'auth-oauth', phase: 'COORDINATE',
      parent_spec: null, children_specs: ['auth-oauth-callback'],
    });
    writeState(tmp, 'auth-oauth-callback', {
      spec: 'auth-oauth-callback', phase: 'CLOSE',
      parent_spec: 'auth-oauth', children_specs: [],
    });

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'spec-tree',
      '--spec', 'auth-oauth',
      '--compact',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0);
    const node = result.parsed;
    assert.ok(node && !node.error, `no error expected: ${JSON.stringify(node)}`);
    assert.equal(node.spec, 'auth-oauth');
    assert.equal(node.phase, 'COORDINATE');
    assert.equal(node.children.length, 1);
    assert.equal(node.children[0].spec, 'auth-oauth-callback');
    assert.equal(node.children[0].phase, 'CLOSE');
    // compact should NOT have parent_spec key
    assert.ok(!('parent_spec' in node), 'compact should not include parent_spec');
  });
});

// ── Test 4: cycle detection ───────────────────────────────────────────────────

describe('Wave 7 — buildSpecTree: cycle detection', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns error when A.parent=B and B.parent=A (direct cycle)', async () => {
    // A lists B as child, B lists A as child → cycle
    writeState(tmp, 'spec-a', {
      spec: 'spec-a', phase: 'COORDINATE',
      parent_spec: 'spec-b', children_specs: ['spec-b'],
    });
    writeState(tmp, 'spec-b', {
      spec: 'spec-b', phase: 'ANALYZE',
      parent_spec: 'spec-a', children_specs: ['spec-a'],
    });

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'spec-tree',
      '--spec', 'spec-a',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, 'exit 0 (fail-open)');
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    // Should return an error about cycle detected
    assert.ok(
      result.parsed.error && result.parsed.error.toLowerCase().includes('cycle'),
      `expected cycle error, got: ${JSON.stringify(result.parsed)}`
    );
  });
});

// ── Test 5: buildCrossSessionTimeline marks epic ──────────────────────────────

describe('Wave 7 — buildCrossSessionTimeline: epic label when children_specs exist', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('compact cross-session-timeline includes epic info for specs with children', async () => {
    // Write a session file referencing auth-oauth as a spec
    const sessionsDir = path.join(tmp, '.claude', '.harness', 'sessions');
    const sessionFile = path.join(sessionsDir, 's-epic-test.jsonl');
    const sessionEvent = JSON.stringify({
      v: 1, ts: new Date().toISOString(), sessionId: 's-epic-test',
      wave: 1, actor: { kind: 'hook' }, event: 'agent.start', spec: 'auth-oauth',
      payload: {},
    });
    fs.writeFileSync(sessionFile, sessionEvent + '\n', 'utf8');

    // Write parent state with children on disk
    writeState(tmp, 'auth-oauth', {
      spec: 'auth-oauth', phase: 'COORDINATE',
      parent_spec: null, children_specs: ['auth-oauth-callback', 'auth-oauth-refresh'],
    });
    writeState(tmp, 'auth-oauth-callback', {
      spec: 'auth-oauth-callback', phase: 'CLOSE',
      parent_spec: 'auth-oauth', children_specs: [],
    });
    writeState(tmp, 'auth-oauth-refresh', {
      spec: 'auth-oauth-refresh', phase: 'ANALYZE',
      parent_spec: 'auth-oauth', children_specs: [],
    });

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'cross-session-timeline',
      '--compact',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, `exit 0 expected, stderr: ${result.stderr}`);
    const arr = result.parsed;
    assert.ok(Array.isArray(arr), `expected array: ${result.stdout}`);
    assert.ok(arr.length > 0, 'should have at least 1 session entry');

    const session = arr[0];
    const epicEntry = (session.specs || []).find(s => s.epic === 'auth-oauth');
    assert.ok(epicEntry, `expected epic entry for auth-oauth, got: ${JSON.stringify(session.specs)}`);
    assert.ok(
      typeof epicEntry.children === 'string' && epicEntry.children.includes('children CLOSED'),
      `epic entry should have children string: ${JSON.stringify(epicEntry)}`
    );
  });
});

// ── Test 6: spec not found returns error ──────────────────────────────────────

describe('Wave 7 — buildSpecTree: spec not found', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns { error: "spec not found" } for non-existent root', async () => {
    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'spec-tree',
      '--spec', 'does-not-exist',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, 'exit 0 (fail-open)');
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    assert.ok(
      result.parsed.error && result.parsed.error.includes('not found'),
      `expected "not found" error, got: ${JSON.stringify(result.parsed)}`
    );
  });
});

// ── Test 7: COORDINATE phase is accepted ──────────────────────────────────────

describe('Wave 7 — COORDINATE phase: pipeline-state accepts COORDINATE', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('pipeline-state view shows COORDINATE phase from harness events', async () => {
    // Write harness events.jsonl with a COORDINATE phase event
    const evFile = path.join(tmp, '.claude', '.harness', 'events.jsonl');
    const phaseEvent = JSON.stringify({
      v: 1, ts: new Date().toISOString(), sessionId: 's-coord', wave: 1,
      actor: { kind: 'hook' }, event: 'pipeline.phase', spec: 'auth-oauth',
      payload: { from: 'ANALYZE', to: 'COORDINATE' },
    });
    fs.appendFileSync(evFile, phaseEvent + '\n', 'utf8');

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'pipeline-state',
      '--spec', 'auth-oauth',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0);
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    assert.equal(result.parsed.phase, 'COORDINATE', `expected COORDINATE phase, got: ${result.parsed.phase}`);
  });
});

// ── Test 8: spec-link exits 0 on missing args (fail-open) ────────────────────

describe('Wave 7 — spec-link: fail-open on missing args', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('exits 0 when --parent is missing', async () => {
    const result = await runScript(SPEC_LINK, [
      '--child', 'auth-oauth-callback',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, 'fail-open: must exit 0 even on bad args');
  });

  it('exits 0 when --child is missing', async () => {
    const result = await runScript(SPEC_LINK, [
      '--parent', 'auth-oauth',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, 'fail-open: must exit 0 even on bad args');
  });
});
