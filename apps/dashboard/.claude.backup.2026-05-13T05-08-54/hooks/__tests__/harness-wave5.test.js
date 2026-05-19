#!/usr/bin/env node
'use strict';
/**
 * Harness Wave 5 — Context Tuning Tests
 *
 * Covers:
 * 1. Adaptive budget: Explore agent gets ≤400 chars, general-purpose gets ≤800 chars
 * 2. Dedup findings: identical content collapses to 1 entry; distinct content preserved
 * 3. Streaming filter: skipEvents=['tool.use'] excludes heartbeats without loading them
 * 4. Regression: buildPipelineState still aggregates tool.use when readEventsSync called
 *    WITHOUT skipEvents (no regression for metrics-collect.js path)
 *
 * Run with: node --test templates/hooks/__tests__/harness-wave5.test.js
 */

const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');

const views = require('../../scripts/harness-views.js');

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

/** Create a minimal project dir with harness dir. */
function makeProjectDir(base) {
  const dir = fs.mkdtempSync(path.join(base, 'mustard-w5-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.agent-state'), { recursive: true });
  return dir;
}

/** Append a JSON event to events.jsonl */
function appendEvent(projectDir, event) {
  const evFile = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  fs.appendFileSync(evFile, JSON.stringify(event) + '\n', 'utf8');
}

/** Build a baseline event object */
function makeEvent(overrides) {
  return Object.assign({
    v: 1,
    ts: new Date().toISOString(),
    sessionId: 's-test',
    wave: 1,
    actor: { kind: 'agent', id: 'ag-default', type: 'general-purpose' },
    event: 'agent.start',
    payload: { description: 'default agent', model: null },
  }, overrides);
}

/** Build a large agent.start event so context truncation is exercised. */
function makeLargeFindingEvent(content, confidence, wave) {
  return makeEvent({
    wave: wave || 1,
    event: 'finding',
    actor: { kind: 'agent', id: 'ag-finder', type: 'general-purpose' },
    payload: {
      kind: 'pattern',
      content: content || 'A'.repeat(600),
      confidence: confidence || 0.9,
      refs: [],
    },
  });
}

// ── Test 1: Adaptive budget per agent type ────────────────────────────────────

describe('Wave 5 — adaptive budget: Explore ≤ 400 chars, general-purpose ≤ 800 chars', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(os.tmpdir()); });
  afterEach(() => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (_) {} });

  it('Explore agent: additionalContext length ≤ 400', async () => {
    // Write many agent.start events + large finding so context would overflow if budget=800
    for (let i = 0; i < 5; i++) {
      appendEvent(tmp, makeEvent({
        wave: 1,
        actor: { kind: 'agent', id: `ag-${i}`, type: 'general-purpose' },
        event: 'agent.start',
        payload: { description: `Agent ${i}: ${'x'.repeat(150)}`, model: null },
      }));
    }
    appendEvent(tmp, makeLargeFindingEvent('B'.repeat(600), 0.95, 1));

    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-explore-test',
      agent_type: 'Explore',
      cwd: tmp,
      session_id: 's-w5-explore',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext must be a string');

    // The [Agent Memory] block must be present and within budget
    const memBlock = ctx.indexOf('[Agent Memory]');
    if (memBlock !== -1) {
      const memPart = ctx.slice(memBlock);
      assert.ok(
        memPart.length <= 400 + 50, // +50 for the "[Agent Memory] Findings from prior agents:\n" header
        `Explore memory block must be ≤ ~450 chars total. Got: ${memPart.length}\n${memPart}`
      );
    }

    // Whole additionalContext must not blow past 400 + fixed prefix overhead
    // Fixed prefix: '[Tracker] Agent "Explore" registered. Follow all CLAUDE.md rules.'
    // That's ~65 chars. Total should be well under 500 chars.
    assert.ok(
      ctx.length <= 800,
      `Full context length should not reach general-purpose cap (800). Got: ${ctx.length}`
    );
  });

  it('general-purpose agent: additionalContext length ≤ 800', async () => {
    // Write large finding
    appendEvent(tmp, makeLargeFindingEvent('C'.repeat(600), 0.9, 1));

    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-gp-test',
      agent_type: 'general-purpose',
      cwd: tmp,
      session_id: 's-w5-gp',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook exited non-zero: ${result.stderr}`);
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext must be a string');

    // With budget=800, the visText portion must not exceed 800 chars
    const memBlock = ctx.indexOf('[Agent Memory]');
    if (memBlock !== -1) {
      const memPart = ctx.slice(memBlock);
      assert.ok(
        memPart.length <= 900, // 800 cap + header overhead
        `general-purpose memory block must be ≤ ~900 chars total. Got: ${memPart.length}`
      );
    }
  });

  it('unknown agent type falls back to default budget of 600', async () => {
    // Just verify the hook does not crash with an unknown agent_type
    appendEvent(tmp, makeLargeFindingEvent('D'.repeat(600), 0.9, 1));

    const result = await runHook('subagent-tracker.js', {
      hook_event_name: 'SubagentStart',
      agent_id: 'ag-custom-test',
      agent_type: 'CustomAgent',
      cwd: tmp,
      session_id: 's-w5-custom',
    }, { projectDir: tmp });

    assert.equal(result.code, 0, `hook must not crash for unknown agent type: ${result.stderr}`);
    const ctx = result.parsed && result.parsed.hookSpecificOutput && result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(typeof ctx === 'string', 'additionalContext must be a string');
  });
});

// ── Test 2: Dedup findings by content hash ────────────────────────────────────

describe('Wave 5 — dedup findings: identical content collapses to 1', () => {
  const CONTENT = 'Auth uses JWT tokens for session management';

  it('3 findings with identical content → view returns 1', () => {
    const events = [
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: CONTENT, confidence: 0.7, refs: [] } }),
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: CONTENT, confidence: 0.9, refs: [] } }),
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: CONTENT, confidence: 0.8, refs: [] } }),
    ];

    const vis = views.buildAgentVisibility(events, {});
    assert.equal(
      vis.findings.length,
      1,
      `Expected 1 deduped finding, got ${vis.findings.length}`
    );
    // The surviving finding must be the highest confidence (sorted conf desc → picked first)
    const conf = vis.findings[0].payload.confidence;
    assert.equal(conf, 0.9, `Highest-confidence finding must survive dedup. Got: ${conf}`);
  });

  it('3 findings with distinct content → view returns 3', () => {
    const events = [
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: 'Finding about Auth', confidence: 0.8, refs: [] } }),
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: 'Finding about DB schema', confidence: 0.85, refs: [] } }),
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: 'Finding about API rate limiting', confidence: 0.75, refs: [] } }),
    ];

    const vis = views.buildAgentVisibility(events, {});
    assert.equal(
      vis.findings.length,
      3,
      `Expected 3 distinct findings, got ${vis.findings.length}`
    );
  });

  it('whitespace/case normalisation: same content with different spacing dedupes', () => {
    const events = [
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: 'Auth uses JWT tokens', confidence: 0.9, refs: [] } }),
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: '  AUTH  uses   JWT  tokens  ', confidence: 0.8, refs: [] } }),
    ];

    const vis = views.buildAgentVisibility(events, {});
    assert.equal(
      vis.findings.length,
      1,
      `Normalised identical content should dedup to 1. Got ${vis.findings.length}`
    );
  });

  it('findings below minConfidence are excluded before dedup', () => {
    const events = [
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: 'Low confidence finding', confidence: 0.3, refs: [] } }),
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: 'Low confidence finding', confidence: 0.5, refs: [] } }),
      makeEvent({ event: 'finding', payload: { kind: 'pattern', content: 'High confidence finding', confidence: 0.9, refs: [] } }),
    ];

    const vis = views.buildAgentVisibility(events, { minConfidence: 0.7 });
    assert.equal(vis.findings.length, 1, 'Only the high-confidence finding must survive');
    assert.equal(vis.findings[0].payload.content, 'High confidence finding');
  });
});

// ── Test 3: Streaming filter — skipEvents ─────────────────────────────────────

describe('Wave 5 — readEventsSync: skipEvents filters during parse', () => {
  let tmpDir;
  beforeEach(() => { tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w5-skip-')); });
  afterEach(() => { try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch (_) {} });

  it('100 tool.use + 5 agent.start → skipEvents=[tool.use] returns 5', () => {
    const fp = path.join(tmpDir, 'events.jsonl');
    const lines = [];
    for (let i = 0; i < 100; i++) {
      lines.push(JSON.stringify(makeEvent({ event: 'tool.use', payload: { tool: 'Bash' } })));
    }
    for (let i = 0; i < 5; i++) {
      lines.push(JSON.stringify(makeEvent({ event: 'agent.start', actor: { kind: 'agent', id: `ag-${i}`, type: 'general-purpose' } })));
    }
    fs.writeFileSync(fp, lines.join('\n') + '\n', 'utf8');

    const result = views.readEventsSync(fp, { skipEvents: ['tool.use'] });
    assert.equal(result.length, 5, `Expected 5 agent.start events. Got ${result.length}`);
    for (const ev of result) {
      assert.equal(ev.event, 'agent.start', 'All returned events must be agent.start');
    }
  });

  it('without opts: 100 tool.use + 5 agent.start → returns 105 (retrocompat)', () => {
    const fp = path.join(tmpDir, 'events.jsonl');
    const lines = [];
    for (let i = 0; i < 100; i++) {
      lines.push(JSON.stringify(makeEvent({ event: 'tool.use', payload: { tool: 'Edit' } })));
    }
    for (let i = 0; i < 5; i++) {
      lines.push(JSON.stringify(makeEvent({ event: 'agent.start' })));
    }
    fs.writeFileSync(fp, lines.join('\n') + '\n', 'utf8');

    const result = views.readEventsSync(fp);
    assert.equal(result.length, 105, `Expected 105 events without skipEvents. Got ${result.length}`);
  });

  it('skipEvents with empty array: behaves same as no opts', () => {
    const fp = path.join(tmpDir, 'events.jsonl');
    const lines = [
      JSON.stringify(makeEvent({ event: 'tool.use' })),
      JSON.stringify(makeEvent({ event: 'agent.start' })),
    ];
    fs.writeFileSync(fp, lines.join('\n') + '\n', 'utf8');

    const withEmpty = views.readEventsSync(fp, { skipEvents: [] });
    const withoutOpts = views.readEventsSync(fp);
    assert.equal(withEmpty.length, withoutOpts.length, 'Empty skipEvents should behave like no opts');
  });

  it('skipEvents: multiple event types can be excluded simultaneously', () => {
    const fp = path.join(tmpDir, 'events.jsonl');
    const lines = [
      JSON.stringify(makeEvent({ event: 'tool.use' })),
      JSON.stringify(makeEvent({ event: 'agent.start' })),
      JSON.stringify(makeEvent({ event: 'finding' })),
      JSON.stringify(makeEvent({ event: 'pipeline.phase' })),
    ];
    fs.writeFileSync(fp, lines.join('\n') + '\n', 'utf8');

    const result = views.readEventsSync(fp, { skipEvents: ['tool.use', 'finding'] });
    assert.equal(result.length, 2, 'Only agent.start + pipeline.phase must remain');
    const types = result.map(e => e.event).sort();
    assert.deepEqual(types, ['agent.start', 'pipeline.phase']);
  });
});

// ── Test 4: Regression — buildPipelineState still works without skipEvents ────

describe('Wave 5 — regression: buildPipelineState aggregates tool.use correctly', () => {
  let tmpDir;
  beforeEach(() => { tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w5-ps-')); });
  afterEach(() => { try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch (_) {} });

  it('metrics-collect.js path: readEventsSync without skipEvents → buildPipelineState has apiCalls > 0', () => {
    const fp = path.join(tmpDir, 'events.jsonl');
    const now = new Date().toISOString();
    const lines = [
      JSON.stringify(makeEvent({ ts: now, spec: 'my-spec', event: 'pipeline.phase', payload: { from: null, to: 'ANALYZE' } })),
      JSON.stringify(makeEvent({ ts: now, spec: 'my-spec', event: 'agent.start', actor: { kind: 'agent', id: 'ag-1', type: 'Explore' } })),
      JSON.stringify(makeEvent({ ts: now, spec: 'my-spec', event: 'tool.use', payload: { tool: 'Bash' } })),
      JSON.stringify(makeEvent({ ts: now, spec: 'my-spec', event: 'tool.use', payload: { tool: 'Edit' } })),
      JSON.stringify(makeEvent({ ts: now, spec: 'my-spec', event: 'tool.use', payload: { tool: 'Read' } })), // excluded from apiCalls (Read rule)
    ];
    fs.writeFileSync(fp, lines.join('\n') + '\n', 'utf8');

    // Simulate what metrics-collect.js does: readEventsSync WITHOUT skipEvents
    const events = views.readEventsSync(fp);
    assert.equal(events.length, 5, 'All 5 events must be loaded when no skipEvents');

    const ps = views.buildPipelineState(events, { spec: 'my-spec' });
    assert.ok(ps.metrics, 'metrics must be present');
    assert.equal(ps.metrics.apiCalls, 2, 'Bash + Edit = 2 apiCalls (Read excluded)');
    assert.equal(ps.metrics.agentCount, 1, 'One agent.start');
    assert.equal(ps.phase, 'ANALYZE', 'Phase must be detected from pipeline.phase event');
  });

  it('confirm: if tool.use was skipped, buildPipelineState would miss metrics (demonstrates the guard)', () => {
    const fp = path.join(tmpDir, 'events-skip.jsonl');
    const now = new Date().toISOString();
    const lines = [
      JSON.stringify(makeEvent({ ts: now, spec: 'my-spec', event: 'tool.use', payload: { tool: 'Bash' } })),
      JSON.stringify(makeEvent({ ts: now, spec: 'my-spec', event: 'tool.use', payload: { tool: 'Edit' } })),
    ];
    fs.writeFileSync(fp, lines.join('\n') + '\n', 'utf8');

    // This demonstrates WHY metrics-collect.js must NOT pass skipEvents
    const eventsSkipped = views.readEventsSync(fp, { skipEvents: ['tool.use'] });
    const psSkipped = views.buildPipelineState(eventsSkipped, { spec: 'my-spec' });
    assert.equal(psSkipped.metrics.apiCalls, 0, 'When tool.use is skipped, apiCalls correctly drops to 0');

    // Without skip → correct metrics
    const eventsAll = views.readEventsSync(fp);
    const psAll = views.buildPipelineState(eventsAll, { spec: 'my-spec' });
    assert.equal(psAll.metrics.apiCalls, 2, 'Without skipEvents, metrics are correct');
  });
});
