#!/usr/bin/env bun
/**
 * Integration tests for Mustard hooks — cross-hook interaction scenarios.
 * Adds to the existing 26 unit tests without modifying hooks.test.js.
 * Run with: bun test templates/hooks/__tests__/
 */

'use strict';

const { describe, it, beforeAll, afterAll } = require('bun:test');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');

const HOOKS_DIR = path.resolve(__dirname, '..');
const PROJECT_DIR = path.resolve(__dirname, '..', '..', '..');

function runHook(hookFile, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const cwd = opts.cwd || PROJECT_DIR;
    const child = spawn(process.execPath, [path.join(HOOKS_DIR, hookFile)], {
      cwd,
      env: {
        ...process.env,
        CLAUDE_PROJECT_DIR: opts.projectDir || PROJECT_DIR,
        CONTEXT_BUDGET_MODE: opts.budgetMode || 'strict',
      },
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    child.on('error', reject);
    child.on('close', (code) => {
      let parsed = null;
      if (stdout.trim()) {
        try { parsed = JSON.parse(stdout.trim()); } catch { /* not JSON */ }
      }
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });

    const input = typeof inputObj === 'string' ? inputObj : JSON.stringify(inputObj);
    child.stdin.write(input);
    child.stdin.end();
  });
}

// ─── Suite 1: Fail-open — malformed stdin ────────────────────────────────────

describe('Suite 1: fail-open on malformed input', () => {
  const malformed = ['', 'not-json', '{"tool_name":}', '{}', '{"x":1}'];

  for (const hook of ['context-budget.js', 'pre-compact.js', 'subagent-tracker.js']) {
    for (const bad of malformed) {
      it(`${hook} exits 0 on malformed stdin: ${JSON.stringify(bad).slice(0, 30)}`, async () => {
        const result = await runHook(hook, bad);
        assert.equal(result.code, 0, `Expected exit 0, got ${result.code}. stderr: ${result.stderr}`);
      });
    }
  }

  // spec-hygiene reads from cwd fs only — stdin is largely unused, still test empty
  it('spec-hygiene.js exits 0 on empty stdin', async () => {
    const result = await runHook('spec-hygiene.js', '');
    assert.equal(result.code, 0);
  });
});

// ─── Suite 2: context-budget edge cases ─────────────────────────────────────

describe('Suite 2: context-budget edge cases', () => {
  function taskPayload(subagent_type, promptLen, description = '') {
    return {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      tool_input: {
        subagent_type,
        description,
        prompt: 'A'.repeat(promptLen),
      },
    };
  }

  it('Explore prompt at exactly 10000 chars → allow', async () => {
    const r = await runHook('context-budget.js', taskPayload('Explore', 10000));
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'allow');
  });

  it('Explore prompt at 10001 chars → deny', async () => {
    const r = await runHook('context-budget.js', taskPayload('Explore', 10001));
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'deny');
  });

  it('Empty prompt → allow (advisory path, no block)', async () => {
    const r = await runHook('context-budget.js', taskPayload('Explore', 0));
    assert.equal(r.code, 0);
    // no deny
    assert.notEqual(r.parsed?.permissionDecision, 'deny');
  });

  it('Unicode emoji prompt counted by .length (4-byte chars count as 2 in JS)', async () => {
    // 🎯 is a surrogate pair — .length === 2 in JS
    // Fill to 10002 chars using emoji + padding so it exceeds 10000
    const emoji = '🎯'; // length === 2
    const padding = 'A'.repeat(9999);
    const prompt = emoji + padding; // length = 10001 → deny
    const payload = {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      tool_input: { subagent_type: 'Explore', description: '', prompt },
    };
    const r = await runHook('context-budget.js', payload);
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'deny');
  });

  it('subagent_type undefined → fail-open allow (no hard budget for unknown types)', async () => {
    const r = await runHook('context-budget.js', taskPayload(undefined, 50000));
    assert.equal(r.code, 0);
    assert.notEqual(r.parsed?.permissionDecision, 'deny');
  });

  it('general-purpose at 30000 chars → allow', async () => {
    const r = await runHook('context-budget.js', taskPayload('general-purpose', 30000, 'implement feature'));
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'allow');
  });

  it('general-purpose at 30001 chars → deny', async () => {
    const r = await runHook('context-budget.js', taskPayload('general-purpose', 30001, 'implement feature'));
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'deny');
  });

  it('general-purpose with "review" in description at 12000 chars → allow', async () => {
    const r = await runHook('context-budget.js', taskPayload('general-purpose', 12000, 'review pull request'));
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'allow');
  });

  it('general-purpose with "review" in description at 12001 chars → deny', async () => {
    const r = await runHook('context-budget.js', taskPayload('general-purpose', 12001, 'review pull request'));
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'deny');
  });

  it('Plan type at 50000 chars → advisory only (no deny)', async () => {
    const r = await runHook('context-budget.js', taskPayload('Plan', 50000, 'plan architecture'));
    assert.equal(r.code, 0);
    assert.notEqual(r.parsed?.permissionDecision, 'deny');
  });
});

// ─── Suite 2.1: Dumb Zone advisory (% of model window) ──────────────────────
// Reference: Liu et al. 2023 (arXiv:2307.03172) + Dex Horthy "Dumb Zone" 40%

describe('Suite 2.1: Dumb Zone advisory', () => {
  function planPayload(promptLen, model) {
    // Use Plan type so per-role hard block is skipped (advisory-only path)
    const payload = {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      tool_input: {
        subagent_type: 'Plan',
        description: 'plan architecture',
        prompt: 'A'.repeat(promptLen),
      },
    };
    if (model) payload.model = model;
    return payload;
  }

  it('Plan with 40K-token prompt + sonnet (200K window) = 20% → no advisory', async () => {
    // 40K tokens × 4 chars = 160K chars; 160K/200K = 20% < 40%
    const r = await runHook('context-budget.js', planPayload(160000, 'claude-sonnet-4-5'));
    assert.equal(r.code, 0);
    const txt = JSON.stringify(r.parsed || {});
    assert.ok(!/Dumb Zone/.test(txt), 'Should not warn at 20% window');
  });

  it('Plan with 90K-token prompt + sonnet (200K window) = 45% → Dumb Zone advisory', async () => {
    // 90K tokens × 4 = 360K chars; 360K/200K = ~45% ≥ 40%
    const r = await runHook('context-budget.js', planPayload(360000, 'claude-sonnet-4-5'));
    assert.equal(r.code, 0);
    const txt = JSON.stringify(r.parsed || {});
    assert.ok(/Dumb Zone/.test(txt), `Expected Dumb Zone advisory at ~45%; got: ${txt.slice(0, 200)}`);
    assert.ok(!/Compact Now/.test(txt), 'Should NOT trigger Compact at 45% (only ≥65%)');
  });

  it('Plan with 140K-token prompt + sonnet (200K window) = 70% → Compact Now advisory', async () => {
    // 140K tokens × 4 = 560K chars; 560K/200K = 70% ≥ 65%
    const r = await runHook('context-budget.js', planPayload(560000, 'claude-sonnet-4-5'));
    assert.equal(r.code, 0);
    const txt = JSON.stringify(r.parsed || {});
    assert.ok(/Compact Now/.test(txt), `Expected Compact Now advisory at 70%; got: ${txt.slice(0, 200)}`);
  });

  it('Plan with 90K-token prompt + opus-1m (1M window) = 9% → no advisory', async () => {
    // 90K tokens / 1M = 9% — well below 40% even though absolute is large
    const r = await runHook('context-budget.js', planPayload(360000, 'claude-opus-4-7-1m'));
    assert.equal(r.code, 0);
    const txt = JSON.stringify(r.parsed || {});
    assert.ok(!/Dumb Zone/.test(txt), 'Should not warn at 9% of 1M window');
    assert.ok(!/Compact Now/.test(txt), 'Should not advise compact at 9%');
  });

  it('Plan with no model hint and small prompt → no advisory', async () => {
    const r = await runHook('context-budget.js', planPayload(8000)); // 2K tokens, no model
    assert.equal(r.code, 0);
    const txt = JSON.stringify(r.parsed || {});
    assert.ok(!/Dumb Zone/.test(txt), 'Should not warn for tiny prompt');
  });
});

// ─── Suite 3: spec-hygiene classification ────────────────────────────────────

describe('Suite 3: spec-hygiene classification', () => {
  let tmpDir;

  beforeAll(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-hygiene-'));
  });

  afterAll(() => {
    try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch (_) {}
  });

  function makeSpec(name, content) {
    const specDir = path.join(tmpDir, '.claude', 'spec', 'active', name);
    fs.mkdirSync(specDir, { recursive: true });
    fs.writeFileSync(path.join(specDir, 'spec.md'), content, 'utf8');
    return specDir;
  }

  it('Status: completed + all [x] → spec moved to completed (auto-move)', async () => {
    makeSpec('spec-done', [
      '### Status: completed | Phase: CLOSE',
      '## Checklist',
      '- [x] Step one',
      '- [x] Step two',
    ].join('\n'));

    const r = await runHook('spec-hygiene.js', '{}', { cwd: tmpDir, projectDir: tmpDir });
    assert.equal(r.code, 0);
    const completedDir = path.join(tmpDir, '.claude', 'spec', 'completed', 'spec-done');
    assert.ok(fs.existsSync(completedDir), 'Spec should have been moved to completed/');
  });

  it('Status: implementing + all [x] → warn only (not moved)', async () => {
    makeSpec('spec-implementing', [
      '### Status: implementing | Phase: EXECUTE',
      '## Checklist',
      '- [x] Step one',
      '- [x] Step two',
    ].join('\n'));

    const r = await runHook('spec-hygiene.js', '{}', { cwd: tmpDir, projectDir: tmpDir });
    assert.equal(r.code, 0);
    const activeDir = path.join(tmpDir, '.claude', 'spec', 'active', 'spec-implementing');
    assert.ok(fs.existsSync(activeDir), 'Spec should remain in active/ (warn only)');
    assert.ok(r.stderr.includes('implementing') || r.stderr.includes('complete'), 'Should warn in stderr');
  });

  it('Status: draft + partial [ ] → silent (no move, no warn)', async () => {
    makeSpec('spec-draft', [
      '### Status: draft | Phase: ANALYZE',
      '## Checklist',
      '- [x] Step one',
      '- [ ] Step two pending',
    ].join('\n'));

    const r = await runHook('spec-hygiene.js', '{}', { cwd: tmpDir, projectDir: tmpDir });
    assert.equal(r.code, 0);
    const activeDir = path.join(tmpDir, '.claude', 'spec', 'active', 'spec-draft');
    assert.ok(fs.existsSync(activeDir), 'Draft spec should remain untouched');
  });

  it('Spec with ## Concerns BLOCKED → silent even if completed', async () => {
    makeSpec('spec-blocked', [
      '### Status: completed | Phase: CLOSE',
      '## Concerns',
      'BLOCKED: waiting for API approval',
      '## Checklist',
      '- [x] Step one',
    ].join('\n'));

    const r = await runHook('spec-hygiene.js', '{}', { cwd: tmpDir, projectDir: tmpDir });
    assert.equal(r.code, 0);
    const activeDir = path.join(tmpDir, '.claude', 'spec', 'active', 'spec-blocked');
    assert.ok(fs.existsSync(activeDir), 'Blocked spec should not be moved');
  });

  it('Spec without ## Checklist section → silent (defensive)', async () => {
    makeSpec('spec-no-checklist', [
      '### Status: completed | Phase: CLOSE',
      '## Summary',
      'Some feature',
    ].join('\n'));

    const r = await runHook('spec-hygiene.js', '{}', { cwd: tmpDir, projectDir: tmpDir });
    assert.equal(r.code, 0);
    const activeDir = path.join(tmpDir, '.claude', 'spec', 'active', 'spec-no-checklist');
    // No checklist → total=0 → allDone=false → silent (not moved)
    assert.ok(fs.existsSync(activeDir), 'No-checklist spec should not be moved');
  });
});

// ─── Suite 4: Hook sequence (simulated session) ──────────────────────────────

describe('Suite 4: hook sequence (simulated session)', () => {
  it('SessionStart → spec-hygiene.js exits 0', async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-seq-'));
    try {
      const r = await runHook('spec-hygiene.js', JSON.stringify({ hook_event_name: 'SessionStart' }), {
        cwd: tmpDir, projectDir: tmpDir,
      });
      assert.equal(r.code, 0);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it('PreToolUse(Task) → context-budget.js allows valid payload', async () => {
    const payload = {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      tool_input: {
        subagent_type: 'general-purpose',
        description: 'implement user service',
        prompt: 'A'.repeat(1000),
      },
    };
    const r = await runHook('context-budget.js', payload);
    assert.equal(r.code, 0);
    assert.equal(r.parsed?.permissionDecision, 'allow');
  });

  it('PostToolUse(Task) → subagent-tracker.js exits 0 with valid payload', async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-seq2-'));
    fs.mkdirSync(path.join(tmpDir, '.claude', '.agent-state'), { recursive: true });
    try {
      const r = await runHook('subagent-tracker.js', {
        hook_event_name: 'SubagentStart',
        agent_id: 'seq-agent-1',
        agent_type: 'general-purpose',
        session_id: 'seq-session',
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });
      assert.equal(r.code, 0);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it('No shared state leaks between hook invocations', async () => {
    // Run context-budget twice with different roles — each must return independently
    const exploreResult = await runHook('context-budget.js', {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      tool_input: { subagent_type: 'Explore', description: '', prompt: 'A'.repeat(10001) },
    });
    const gpResult = await runHook('context-budget.js', {
      hook_event_name: 'PreToolUse',
      tool_name: 'Task',
      tool_input: { subagent_type: 'general-purpose', description: 'implement', prompt: 'A'.repeat(1000) },
    });

    assert.equal(exploreResult.parsed?.permissionDecision, 'deny', 'Explore over-budget should deny');
    assert.equal(gpResult.parsed?.permissionDecision, 'allow', 'GP under-budget should allow independently');
  });
});
