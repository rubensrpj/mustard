#!/usr/bin/env bun
/**
 * Tests for spec-size-gate.js and skill-size-gate.js.
 * Run with: bun test templates/hooks/__tests__/size-gates.test.js
 */

const { describe, it } = require('bun:test');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const path = require('node:path');

const HOOKS_DIR = path.resolve(__dirname, '..');

function runHook(hookFile, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const env = { ...process.env, ...(opts.env || {}) };
    const child = spawn(process.execPath, [path.join(HOOKS_DIR, hookFile)], {
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
      if (stdout.trim()) {
        try { parsed = JSON.parse(stdout.trim()); } catch (_) {}
      }
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });

    const payload = typeof inputObj === 'string' ? inputObj : JSON.stringify(inputObj);
    child.stdin.write(payload);
    child.stdin.end();
  });
}

/** Generate a string with N lines of content. */
function makeContent(lineCount) {
  return Array.from({ length: lineCount }, (_, i) => `line ${i + 1}`).join('\n');
}

function specWriteInput(lineCount, filePath) {
  return {
    tool_name: 'Write',
    tool_input: {
      file_path: filePath || '/project/.claude/spec/active/my-epic/spec.md',
      content: makeContent(lineCount),
    },
  };
}

function skillWriteInput(lineCount, filePath) {
  return {
    tool_name: 'Write',
    tool_input: {
      file_path: filePath || '/project/.claude/skills/my-skill/SKILL.md',
      content: makeContent(lineCount),
    },
  };
}

// ─── spec-size-gate.js ────────────────────────────────────────────────────────

describe('spec-size-gate.js', () => {
  const hook = 'spec-size-gate.js';
  const warnEnv  = { MUSTARD_SPEC_SIZE_MODE: 'warn' };
  const strictEnv = { MUSTARD_SPEC_SIZE_MODE: 'strict' };

  it('silent: 150 lines → no stderr, exit 0, no deny', async () => {
    const result = await runHook(hook, specWriteInput(150), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('warn: 250 lines → advisory on stderr, exit 0, allow (warn mode)', async () => {
    const result = await runHook(hook, specWriteInput(250), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.length > 0, 'expected advisory on stderr');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('strict-warn: 450 lines → stronger advisory on stderr, exit 0, allow (warn mode)', async () => {
    const result = await runHook(hook, specWriteInput(450), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.includes('strict-warn'), 'expected strict-warn advisory');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('block (warn mode): 550 lines → advisory on stderr, exit 0, allow (not deny)', async () => {
    const result = await runHook(hook, specWriteInput(550), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.length > 0, 'expected advisory on stderr');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('block (strict mode): 550 lines → deny', async () => {
    const result = await runHook(hook, specWriteInput(550), { env: strictEnv });
    assert.equal(result.code, 0);
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.equal(decision, 'deny', 'expected deny in strict mode at 550 lines');
  });

  it('boundary (strict mode): 499 lines → no deny', async () => {
    const result = await runHook(hook, specWriteInput(499), { env: strictEnv });
    assert.equal(result.code, 0);
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('path filter: non-spec .md file → silent (no advisory)', async () => {
    const result = await runHook(hook, {
      tool_name: 'Write',
      tool_input: {
        file_path: '/project/README.md',
        content: makeContent(550),
      },
    }, { env: warnEnv });
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('fail-open: bad JSON stdin → exit 0', async () => {
    const result = await runHook(hook, 'not-valid-json', { env: warnEnv });
    assert.equal(result.code, 0);
  });

  it('mode=off: 550 lines in spec path → exit 0 silently', async () => {
    const result = await runHook(hook, specWriteInput(550), { env: { MUSTARD_SPEC_SIZE_MODE: 'off' } });
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('Edit variant: 250-line virtual result → advisory on stderr', async () => {
    const result = await runHook(hook, {
      tool_name: 'Edit',
      tool_input: {
        file_path: '/project/.claude/spec/active/my-epic/spec.md',
        // File doesn't exist on disk — simulateEdit treats current as ''
        old_string: '',
        new_string: makeContent(250),
      },
    }, { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.length > 0, 'expected advisory on stderr for Edit with 250 lines result');
  });
});

// ─── skill-size-gate.js ───────────────────────────────────────────────────────

describe('skill-size-gate.js', () => {
  const hook = 'skill-size-gate.js';
  const warnEnv   = { MUSTARD_SKILL_SIZE_MODE: 'warn' };
  const strictEnv = { MUSTARD_SKILL_SIZE_MODE: 'strict' };

  it('silent: 150 lines → no stderr, exit 0, no deny', async () => {
    const result = await runHook(hook, skillWriteInput(150), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('warn: 250 lines → advisory on stderr, exit 0', async () => {
    const result = await runHook(hook, skillWriteInput(250), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.length > 0, 'expected advisory on stderr');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('strict-warn: 450 lines → stronger advisory on stderr, exit 0', async () => {
    const result = await runHook(hook, skillWriteInput(450), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.includes('strict-warn'), 'expected strict-warn advisory');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('block (warn mode): 550 lines → advisory, exit 0, no deny', async () => {
    const result = await runHook(hook, skillWriteInput(550), { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.length > 0, 'expected advisory on stderr');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('block (strict mode): 550 lines → deny', async () => {
    const result = await runHook(hook, skillWriteInput(550), { env: strictEnv });
    assert.equal(result.code, 0);
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.equal(decision, 'deny', 'expected deny in strict mode at 550 lines');
  });

  it('boundary (strict mode): 499 lines → no deny', async () => {
    const result = await runHook(hook, skillWriteInput(499), { env: strictEnv });
    assert.equal(result.code, 0);
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('path filter: non-SKILL.md file → silent', async () => {
    const result = await runHook(hook, {
      tool_name: 'Write',
      tool_input: {
        file_path: '/project/.claude/skills/my-skill/README.md',
        content: makeContent(550),
      },
    }, { env: warnEnv });
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('fail-open: bad JSON stdin → exit 0', async () => {
    const result = await runHook(hook, 'not-valid-json', { env: warnEnv });
    assert.equal(result.code, 0);
  });

  it('generated skill (warn mode): 550 lines → skip silently (generated header)', async () => {
    const result = await runHook(hook, {
      tool_name: 'Write',
      tool_input: {
        file_path: '/project/.claude/skills/my-skill/SKILL.md',
        content: '<!-- mustard:generated -->\n' + makeContent(549),
      },
    }, { env: warnEnv });
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '', 'generated skills should be skipped in warn mode');
  });

  it('generated skill (strict mode): 550 lines → deny (strict applies to all)', async () => {
    const result = await runHook(hook, {
      tool_name: 'Write',
      tool_input: {
        file_path: '/project/.claude/skills/my-skill/SKILL.md',
        content: '<!-- mustard:generated -->\n' + makeContent(549),
      },
    }, { env: strictEnv });
    assert.equal(result.code, 0);
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.equal(decision, 'deny', 'strict mode should deny even generated skills at 550 lines');
  });

  it('mode=off: 550 lines → exit 0 silently', async () => {
    const result = await runHook(hook, skillWriteInput(550), { env: { MUSTARD_SKILL_SIZE_MODE: 'off' } });
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    const decision = result.parsed?.hookSpecificOutput?.permissionDecision;
    assert.notEqual(decision, 'deny');
  });

  it('Edit variant: 250-line virtual result → advisory on stderr', async () => {
    const result = await runHook(hook, {
      tool_name: 'Edit',
      tool_input: {
        file_path: '/project/.claude/skills/my-skill/SKILL.md',
        old_string: '',
        new_string: makeContent(250),
      },
    }, { env: warnEnv });
    assert.equal(result.code, 0);
    assert.ok(result.stderr.length > 0, 'expected advisory for Edit with 250 lines result');
  });
});
