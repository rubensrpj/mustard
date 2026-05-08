#!/usr/bin/env node
/**
 * Tests for skill-validate-gate.js.
 * Run with: node --test .claude/hooks/__tests__/skill-validate-gate.test.js
 */

const { describe, it } = require('node:test');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const path = require('node:path');

const HOOKS_DIR = path.resolve(__dirname, '..');
const HOOK = 'skill-validate-gate.js';

function runHook(inputObj, env = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [path.join(HOOKS_DIR, HOOK)], {
      env: { ...process.env, ...env },
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', d => (stdout += d));
    child.stderr.on('data', d => (stderr += d));
    child.on('error', reject);
    child.on('close', code => {
      let parsed = null;
      if (stdout.trim()) { try { parsed = JSON.parse(stdout.trim()); } catch (_) {} }
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });

    const payload = typeof inputObj === 'string' ? inputObj : JSON.stringify(inputObj);
    child.stdin.write(payload);
    child.stdin.end();
  });
}

const VALID_SKILL = `---
name: my-skill
description: Comprehensive helper. Use when the user wants to do something specific that requires guidance and triggers automatic activation reliably.
source: manual
---

# My Skill

Body content here.
`;

const INVALID_SKILL_NO_SOURCE = `---
name: my-skill
description: Comprehensive helper. Use when the user wants to do something specific that requires guidance and triggers automatic activation reliably.
---

# My Skill

Body content here.
`;

function writeInput(content, filePath) {
  return {
    tool_name: 'Write',
    tool_input: {
      file_path: filePath || '/project/.claude/skills/my-skill/SKILL.md',
      content,
    },
  };
}

describe('skill-validate-gate.js', () => {
  const warnEnv   = { MUSTARD_SKILL_VALIDATE_GATE_MODE: 'warn' };
  const strictEnv = { MUSTARD_SKILL_VALIDATE_GATE_MODE: 'strict' };
  const offEnv    = { MUSTARD_SKILL_VALIDATE_GATE_MODE: 'off' };

  it('valid skill in warn mode → silent, no deny', async () => {
    const result = await runHook(writeInput(VALID_SKILL), warnEnv);
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, 'deny');
  });

  it('valid skill in strict mode → silent, no deny', async () => {
    const result = await runHook(writeInput(VALID_SKILL), strictEnv);
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, 'deny');
  });

  it('invalid skill in warn mode → stderr advisory, exit 0, no deny', async () => {
    const result = await runHook(writeInput(INVALID_SKILL_NO_SOURCE), warnEnv);
    assert.equal(result.code, 0);
    assert.match(result.stderr, /skill-validate-gate/);
    assert.match(result.stderr, /source/);
    assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, 'deny');
  });

  it('invalid skill in strict mode → deny with reason, exit 0', async () => {
    const result = await runHook(writeInput(INVALID_SKILL_NO_SOURCE), strictEnv);
    assert.equal(result.code, 0);
    assert.equal(result.parsed?.hookSpecificOutput?.permissionDecision, 'deny');
    assert.match(result.parsed?.hookSpecificOutput?.permissionDecisionReason || '', /source/);
  });

  it('invalid skill in off mode → silent, no deny, no stderr', async () => {
    const result = await runHook(writeInput(INVALID_SKILL_NO_SOURCE), offEnv);
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, 'deny');
  });

  it('non-SKILL.md path → silent, no action', async () => {
    const result = await runHook(writeInput(INVALID_SKILL_NO_SOURCE, '/project/notes.md'), strictEnv);
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    assert.equal(result.parsed, null);
  });

  it('garbage input → fail-open (exit 0)', async () => {
    const result = await runHook('not-json-at-all', strictEnv);
    assert.equal(result.code, 0);
    assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, 'deny');
  });

  it('Edit tool simulating bad change → strict denies', async () => {
    const editInput = {
      tool_name: 'Edit',
      tool_input: {
        file_path: '/project/.claude/skills/my-skill/SKILL.md',
        old_string: 'placeholder',
        new_string: 'placeholder',
      },
    };
    const result = await runHook(editInput, strictEnv);
    assert.equal(result.code, 0);
    assert.equal(result.parsed?.hookSpecificOutput?.permissionDecision, 'deny');
  });
});
