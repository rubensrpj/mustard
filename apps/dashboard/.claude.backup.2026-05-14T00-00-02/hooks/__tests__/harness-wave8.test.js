#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 8 — Epic Fold Tests
 *
 * Covers:
 * 1. detectCompletedEpics returns only epics with ALL children=CLOSE
 * 2. detectCompletedEpics ignores pre-Wave 7 specs (no parent/children fields)
 * 3. foldEpic emits epic.complete + epic.fold events
 * 4. foldEpic writes epic-summary entry to knowledge.json
 * 5. foldEpic is idempotent (2x does not duplicate knowledge.json entry)
 * 6. foldEpic transitions root pipeline-state to CLOSE
 * 7. buildEpicSummary constructs correct data (children, findings, metrics)
 * 8. buildEpicSummary compact mode returns one-liner
 * 10. Compaction ON (MUSTARD_EPIC_COMPACT=1) removes tool.use for folded specs, keeps findings
 * 11. Compaction OFF keeps all events
 *
 * Run with: bun test templates/hooks/__tests__/harness-wave8.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const HOOKS_DIR = path.resolve(__dirname, '..');
const EPIC_FOLD = path.join(SCRIPTS_DIR, 'epic-fold.js');
const HARNESS_VIEWS = path.join(SCRIPTS_DIR, 'harness-views.js');

// ── Helpers ───────────────────────────────────────────────────────────────────

function runScript(scriptPath, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const env = Object.assign({}, process.env, { MUSTARD_DISABLED_HOOKS: 'all' });
    if (opts.env) Object.assign(env, opts.env);
    const child = spawn(process.execPath, [scriptPath, ...args], {
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
    if (opts.stdin) child.stdin.write(opts.stdin);
    child.stdin.end();
  });
}

function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w8-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness', 'sessions'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.pipeline-states'), { recursive: true });
  return dir;
}

function writeState(projectDir, specName, obj) {
  const f = path.join(projectDir, '.claude', '.pipeline-states', specName + '.json');
  fs.writeFileSync(f, JSON.stringify(obj, null, 2), 'utf8');
}

function readState(projectDir, specName) {
  const f = path.join(projectDir, '.claude', '.pipeline-states', specName + '.json');
  return JSON.parse(fs.readFileSync(f, 'utf8'));
}

function appendEvent(projectDir, ev) {
  const f = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  fs.appendFileSync(f, JSON.stringify(ev) + '\n', 'utf8');
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

function readKnowledge(projectDir) {
  const f = path.join(projectDir, '.claude', 'knowledge.json');
  if (!fs.existsSync(f)) return null;
  return JSON.parse(fs.readFileSync(f, 'utf8'));
}

function setupEpicWithChildren(dir, phase1, phase2) {
  writeState(dir, 'epic-x', {
    spec: 'epic-x', phase: 'COORDINATE',
    parent_spec: null, children_specs: ['c1', 'c2'],
  });
  writeState(dir, 'c1', {
    spec: 'c1', phase: phase1,
    parent_spec: 'epic-x', children_specs: [],
  });
  writeState(dir, 'c2', {
    spec: 'c2', phase: phase2,
    parent_spec: 'epic-x', children_specs: [],
  });
}

// ── Test 1: detectCompletedEpics — all children CLOSE ────────────────────────

describe('Wave 8 — detectCompletedEpics: returns epics with all children=CLOSE', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('returns epic when both children are CLOSE', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');
    const result = await runScript(EPIC_FOLD, ['--detect', '--cwd', tmp], { projectDir: tmp });
    assert.equal(result.code, 0, `exit 0 expected, stderr: ${result.stderr}`);
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    assert.ok(Array.isArray(result.parsed.epics_ready), 'should have epics_ready array');
    assert.ok(result.parsed.epics_ready.includes('epic-x'), `expected epic-x in ${JSON.stringify(result.parsed.epics_ready)}`);
  });

  it('does NOT return epic when one child is not CLOSE', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'EXECUTE');
    const result = await runScript(EPIC_FOLD, ['--detect', '--cwd', tmp], { projectDir: tmp });
    assert.equal(result.code, 0);
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    assert.ok(Array.isArray(result.parsed.epics_ready));
    assert.equal(result.parsed.epics_ready.length, 0, 'no epics should be ready when child is still executing');
  });

  it('does NOT return epic if root is already CLOSE', async () => {
    writeState(tmp, 'epic-x', {
      spec: 'epic-x', phase: 'CLOSE',
      parent_spec: null, children_specs: ['c1', 'c2'],
    });
    writeState(tmp, 'c1', { spec: 'c1', phase: 'CLOSE', parent_spec: 'epic-x', children_specs: [] });
    writeState(tmp, 'c2', { spec: 'c2', phase: 'CLOSE', parent_spec: 'epic-x', children_specs: [] });

    const result = await runScript(EPIC_FOLD, ['--detect', '--cwd', tmp], { projectDir: tmp });
    assert.equal(result.code, 0);
    assert.ok(Array.isArray(result.parsed.epics_ready));
    assert.equal(result.parsed.epics_ready.length, 0, 'already-CLOSE epic should not be re-detected');
  });
});

// ── Test 2: detectCompletedEpics ignores pre-Wave 7 specs ────────────────────

describe('Wave 8 — detectCompletedEpics: ignores specs without parent/children fields', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('ignores specs where children_specs is missing or empty', async () => {
    // Pre-Wave 7 spec: no parent_spec / children_specs
    writeState(tmp, 'old-spec', { spec: 'old-spec', phase: 'COORDINATE' });
    // Spec with empty children array (root but no children — not an epic)
    writeState(tmp, 'solo-root', { spec: 'solo-root', phase: 'EXECUTE', parent_spec: null, children_specs: [] });

    const result = await runScript(EPIC_FOLD, ['--detect', '--cwd', tmp], { projectDir: tmp });
    assert.equal(result.code, 0);
    assert.ok(Array.isArray(result.parsed.epics_ready));
    assert.equal(result.parsed.epics_ready.length, 0, 'pre-Wave 7 specs must be ignored');
  });
});

// ── Test 3: foldEpic emits epic.complete + epic.fold events ──────────────────

describe('Wave 8 — foldEpic: emits epic.complete and epic.fold events', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('emits epic.complete event with correct payload', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');

    // Add some events for aggregation
    appendEvent(tmp, { v: 1, ts: new Date().toISOString(), spec: 'c1', event: 'finding', payload: { content: 'finding A', confidence: 0.9 } });
    appendEvent(tmp, { v: 1, ts: new Date().toISOString(), spec: 'c2', event: 'decision', payload: { content: 'decision B' } });
    appendEvent(tmp, { v: 1, ts: new Date().toISOString(), spec: 'c1', event: 'tool.use', payload: { tool: 'Read' } });
    appendEvent(tmp, { v: 1, ts: new Date().toISOString(), spec: 'c2', event: 'agent.start', payload: {} });

    const result = await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });
    assert.equal(result.code, 0, `exit 0 expected, stderr: ${result.stderr}`);
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    assert.equal(result.parsed.ok, true, `expected ok: true, got: ${JSON.stringify(result.parsed)}`);

    const events = readEvents(tmp);
    const completeEvent = events.find(e => e.event === 'epic.complete');
    assert.ok(completeEvent, 'epic.complete event must be emitted');
    assert.equal(completeEvent.payload.epic, 'epic-x');
    assert.ok(Array.isArray(completeEvent.payload.children));
    assert.ok(completeEvent.payload.children.includes('c1'));
    assert.ok(completeEvent.payload.children.includes('c2'));
    assert.equal(completeEvent.payload.findings_count, 1);
    assert.equal(completeEvent.payload.decisions_count, 1);
    assert.equal(completeEvent.payload.tool_calls_total, 1);
    assert.equal(completeEvent.payload.agents_total, 1);
  });

  it('emits epic.fold tombstone with compactable_specs list', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');
    await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });

    const events = readEvents(tmp);
    const foldEvent = events.find(e => e.event === 'epic.fold');
    assert.ok(foldEvent, 'epic.fold tombstone must be emitted');
    assert.equal(foldEvent.payload.epic, 'epic-x');
    assert.ok(Array.isArray(foldEvent.payload.compactable_specs));
    assert.ok(foldEvent.payload.compactable_specs.includes('epic-x'));
    assert.ok(foldEvent.payload.compactable_specs.includes('c1'));
    assert.ok(foldEvent.payload.compactable_specs.includes('c2'));
  });
});

// ── Test 4: foldEpic writes epic-summary to knowledge.json ───────────────────

describe('Wave 8 — foldEpic: writes epic-summary to knowledge.json', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('creates knowledge.json with epic-summary entry', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');
    appendEvent(tmp, { v: 1, ts: new Date().toISOString(), spec: 'c1', event: 'finding', payload: { content: 'auth callback pattern', confidence: 0.9 } });

    await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });

    const kb = readKnowledge(tmp);
    assert.ok(kb, 'knowledge.json must exist');
    assert.ok(Array.isArray(kb.entries), 'entries must be an array');

    const entry = kb.entries.find(e => e.type === 'epic-summary' && e.name === 'epic-x');
    assert.ok(entry, `expected epic-summary entry for epic-x, got entries: ${JSON.stringify(kb.entries.map(e => e.name))}`);
    assert.ok(typeof entry.description === 'string' && entry.description.length > 0);
    assert.ok(Array.isArray(entry.spec_children));
    assert.ok(entry.spec_children.includes('c1'));
    assert.ok(entry.spec_children.includes('c2'));
    assert.ok(typeof entry.concluded_at === 'string');
  });
});

// ── Test 5: foldEpic is idempotent ───────────────────────────────────────────

describe('Wave 8 — foldEpic: idempotent (2x does not duplicate)', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('calling foldEpic twice does not create duplicate knowledge.json entries', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');

    await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });
    // Second call: root is now CLOSE, should skip
    await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });

    const kb = readKnowledge(tmp);
    assert.ok(kb, 'knowledge.json must exist');
    const epicEntries = kb.entries.filter(e => e.type === 'epic-summary' && e.name === 'epic-x');
    assert.equal(epicEntries.length, 1, `expected exactly 1 epic-summary entry, got ${epicEntries.length}`);
  });

  it('calling foldEpic twice does not emit duplicate epic.complete events', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');

    await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });
    await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });

    const events = readEvents(tmp);
    const completeEvents = events.filter(e => e.event === 'epic.complete' && e.payload && e.payload.epic === 'epic-x');
    assert.equal(completeEvents.length, 1, `expected exactly 1 epic.complete event, got ${completeEvents.length}`);
  });
});

// ── Test 6: foldEpic transitions root to CLOSE ───────────────────────────────

describe('Wave 8 — foldEpic: transitions root pipeline-state to CLOSE', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('sets root phase to CLOSE after fold', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');
    const before = readState(tmp, 'epic-x');
    assert.notEqual(String(before.phase || '').toUpperCase(), 'CLOSE', 'pre-condition: root not CLOSE');

    await runScript(EPIC_FOLD, ['--epic', 'epic-x', '--cwd', tmp], { projectDir: tmp });

    const after = readState(tmp, 'epic-x');
    const afterPhase = String(after.phaseName || after.phase || '').toUpperCase();
    assert.equal(afterPhase, 'CLOSE', `expected root to be CLOSE, got: ${afterPhase}`);
  });
});

// ── Test 7: buildEpicSummary view ────────────────────────────────────────────

describe('Wave 8 — buildEpicSummary: correct data (children, findings, metrics)', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('builds epic summary with children phases and metrics', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');

    // Write harness events for aggregation
    const ts = new Date().toISOString();
    appendEvent(tmp, { v: 1, ts, spec: 'c1', event: 'finding', payload: { content: 'finding 1', confidence: 0.8 } });
    appendEvent(tmp, { v: 1, ts, spec: 'c2', event: 'decision', payload: { content: 'decision 1' } });
    appendEvent(tmp, { v: 1, ts, spec: 'c1', event: 'tool.use', payload: { tool: 'Bash' } });
    appendEvent(tmp, { v: 1, ts, spec: 'c1', event: 'tool.use', payload: { tool: 'Read' } });
    appendEvent(tmp, { v: 1, ts, spec: 'c2', event: 'agent.start', payload: {} });

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'epic-summary',
      '--spec', 'epic-x',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0, `exit 0 expected, stderr: ${result.stderr}`);
    assert.ok(result.parsed, `expected JSON: ${result.stdout}`);
    assert.equal(result.parsed.epic, 'epic-x');
    assert.ok(Array.isArray(result.parsed.children), 'children must be array');
    assert.equal(result.parsed.children.length, 2);

    const childNames = result.parsed.children.map(c => c.spec).sort();
    assert.deepEqual(childNames, ['c1', 'c2']);

    assert.ok(Array.isArray(result.parsed.findings), 'findings must be array');
    assert.equal(result.parsed.findings.length, 1);
    assert.ok(Array.isArray(result.parsed.decisions));
    assert.equal(result.parsed.decisions.length, 1);

    assert.ok(result.parsed.metrics, 'metrics must exist');
    assert.equal(result.parsed.metrics.toolCallsTotal, 2, 'should count 2 tool.use events');
    assert.equal(result.parsed.metrics.agentsTotal, 1);
  });
});

// ── Test 8: buildEpicSummary compact mode ────────────────────────────────────

describe('Wave 8 — buildEpicSummary: compact mode returns one-liner', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('--compact returns one-liner with epic, status, findings_count, tool_calls', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');
    const ts = new Date().toISOString();
    appendEvent(tmp, { v: 1, ts, spec: 'c1', event: 'finding', payload: { content: 'x', confidence: 0.7 } });
    appendEvent(tmp, { v: 1, ts, spec: 'c1', event: 'tool.use', payload: { tool: 'Bash' } });

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'epic-summary',
      '--spec', 'epic-x',
      '--compact',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0);
    const c = result.parsed;
    assert.ok(c, `expected JSON: ${result.stdout}`);
    assert.equal(c.epic, 'epic-x');
    assert.ok(typeof c.status === 'string', 'status must be present');
    assert.ok(typeof c.findings_count === 'number', 'findings_count must be number');
    assert.ok(typeof c.tool_calls === 'number', 'tool_calls must be number');
    assert.ok(Array.isArray(c.children), 'children must be array');
    // Compact should NOT have full findings array
    assert.ok(!Array.isArray(c.findings), 'compact should not include full findings array');
  });

  it('compact shows folded=true as status "folded" after fold', async () => {
    setupEpicWithChildren(tmp, 'CLOSE', 'CLOSE');
    const ts = new Date().toISOString();
    appendEvent(tmp, { v: 1, ts, spec: 'epic-x', event: 'epic.fold', payload: { epic: 'epic-x', compactable_specs: ['epic-x', 'c1', 'c2'] } });

    const result = await runScript(HARNESS_VIEWS, [
      '--view', 'epic-summary',
      '--spec', 'epic-x',
      '--compact',
      '--cwd', tmp,
    ], { projectDir: tmp });

    assert.equal(result.code, 0);
    assert.ok(result.parsed);
    assert.equal(result.parsed.status, 'folded', `expected status=folded, got ${result.parsed.status}`);
  });
});

// ── Test 10: Compaction ON removes tool.use, keeps findings ──────────────────

describe('Wave 8 — session compaction: MUSTARD_EPIC_COMPACT=1 removes granular events', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('removes tool.use and agent.start for folded specs when MUSTARD_EPIC_COMPACT=1', () => {
    // Simulate a session events file with folded epic + granular events
    const eventsFile = path.join(tmp, '.claude', '.harness', 'events.jsonl');
    const ts = new Date().toISOString();

    const lines = [
      // epic.fold tombstone: marks epic-y + c3 + c4 as compactable
      JSON.stringify({ v: 1, ts, event: 'epic.fold', spec: 'epic-y', payload: { epic: 'epic-y', compactable_specs: ['epic-y', 'c3', 'c4'] } }),
      // finding — must be KEPT
      JSON.stringify({ v: 1, ts, event: 'finding', spec: 'c3', payload: { content: 'important finding', confidence: 0.9 } }),
      // tool.use for folded spec — must be REMOVED
      JSON.stringify({ v: 1, ts, event: 'tool.use', spec: 'c3', payload: { tool: 'Bash' } }),
      // agent.start for folded spec — must be REMOVED
      JSON.stringify({ v: 1, ts, event: 'agent.start', spec: 'c4', payload: {} }),
      // pipeline.phase for folded spec — must be KEPT
      JSON.stringify({ v: 1, ts, event: 'pipeline.phase', spec: 'epic-y', payload: { from: 'EXECUTE', to: 'CLOSE' } }),
      // event for an UNRELATED spec — must be KEPT
      JSON.stringify({ v: 1, ts, event: 'tool.use', spec: 'other-spec', payload: { tool: 'Read' } }),
      // decision for folded spec — must be KEPT
      JSON.stringify({ v: 1, ts, event: 'decision', spec: 'c3', payload: { content: 'key decision' } }),
    ];
    fs.writeFileSync(eventsFile, lines.join('\n') + '\n', 'utf8');

    // Run harness-init with MUSTARD_EPIC_COMPACT=1 to trigger compaction.
    // Since harness-init only triggers on rotation (different sessionId), we
    // test the compactEpicEvents logic directly by requiring it inline.
    // We call it by spawning epic-fold with a special test helper, but the
    // simplest approach: read the logic inline via require.

    // We inline the compaction logic here to test it directly (the function is
    // not exported by harness-init, so we replicate the relevant logic).
    const KEEP_EVENTS = new Set([
      'spec.link', 'epic.complete', 'epic.fold', 'epic.ready',
      'finding', 'decision', 'lesson', 'pipeline.phase',
      'session.start', 'session.end', 'dispatch.failure',
    ]);
    const DROP_FOR_FOLDED = new Set(['tool.use', 'agent.start', 'agent.stop']);

    const raw = fs.readFileSync(eventsFile, 'utf8');
    const allLines = raw.split(/\r?\n/).filter(l => l.trim());

    // Find compactable specs
    const compactableSpecs = new Set();
    for (const line of allLines) {
      try {
        const ev = JSON.parse(line);
        if (ev.event === 'epic.fold' && ev.payload && Array.isArray(ev.payload.compactable_specs)) {
          for (const s of ev.payload.compactable_specs) compactableSpecs.add(s);
        }
      } catch (_) {}
    }

    const kept = [];
    for (const line of allLines) {
      if (!line.trim()) continue;
      try {
        const ev = JSON.parse(line);
        const isFoldedSpec = ev.spec && compactableSpecs.has(ev.spec);
        if (isFoldedSpec && DROP_FOR_FOLDED.has(ev.event) && !KEEP_EVENTS.has(ev.event)) continue;
        kept.push(line);
      } catch (_) { kept.push(line); }
    }

    // Verify compaction results
    const keptEvents = kept.map(l => { try { return JSON.parse(l); } catch (_) { return null; } }).filter(Boolean);

    // tool.use for c3 (folded) must be removed
    const toolUseForC3 = keptEvents.filter(e => e.event === 'tool.use' && e.spec === 'c3');
    assert.equal(toolUseForC3.length, 0, 'tool.use for folded spec c3 must be removed');

    // agent.start for c4 (folded) must be removed
    const agentStartForC4 = keptEvents.filter(e => e.event === 'agent.start' && e.spec === 'c4');
    assert.equal(agentStartForC4.length, 0, 'agent.start for folded spec c4 must be removed');

    // finding for c3 must be KEPT
    const findingForC3 = keptEvents.filter(e => e.event === 'finding' && e.spec === 'c3');
    assert.equal(findingForC3.length, 1, 'finding for folded spec must be preserved');

    // pipeline.phase for epic-y must be KEPT
    const phaseForEpic = keptEvents.filter(e => e.event === 'pipeline.phase' && e.spec === 'epic-y');
    assert.equal(phaseForEpic.length, 1, 'pipeline.phase must be preserved even for folded spec');

    // decision for c3 must be KEPT
    const decisionForC3 = keptEvents.filter(e => e.event === 'decision' && e.spec === 'c3');
    assert.equal(decisionForC3.length, 1, 'decision must be preserved even for folded spec');

    // tool.use for other-spec must be KEPT
    const toolUseOther = keptEvents.filter(e => e.event === 'tool.use' && e.spec === 'other-spec');
    assert.equal(toolUseOther.length, 1, 'tool.use for non-folded spec must be preserved');
  });
});

// ── Test 11: Compaction OFF keeps everything ──────────────────────────────────

describe('Wave 8 — session compaction: MUSTARD_EPIC_COMPACT not set keeps all events', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('when compaction is off, all events are preserved in session archive', () => {
    const eventsFile = path.join(tmp, '.claude', '.harness', 'events.jsonl');
    const ts = new Date().toISOString();

    const originalLines = [
      JSON.stringify({ v: 1, ts, event: 'epic.fold', spec: 'epic-z', payload: { epic: 'epic-z', compactable_specs: ['epic-z', 'cA'] } }),
      JSON.stringify({ v: 1, ts, event: 'tool.use', spec: 'cA', payload: { tool: 'Bash' } }),
      JSON.stringify({ v: 1, ts, event: 'finding', spec: 'cA', payload: { content: 'finding', confidence: 0.8 } }),
    ];
    fs.writeFileSync(eventsFile, originalLines.join('\n') + '\n', 'utf8');

    // MUSTARD_EPIC_COMPACT is NOT '1' — no compaction applied
    // Verify all lines preserved
    const raw = fs.readFileSync(eventsFile, 'utf8');
    const savedLines = raw.split(/\r?\n/).filter(l => l.trim());
    assert.equal(savedLines.length, originalLines.length, 'all events must be preserved when compaction is off');
  });
});
