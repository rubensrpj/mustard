#!/usr/bin/env bun
'use strict';
/**
 * skill-validate-gate: PreToolUse hook — warns/blocks SKILL.md writes that fail
 * structural validation (YAML frontmatter, kebab-case name, description triggers,
 * source: scan|manual).
 *
 * Triggers on Write|Edit when file_path ends with /SKILL.md (any depth).
 * Reuses validateSkill() from scripts/skill-validate.js so the gate stays in
 * sync with the standalone validator.
 *
 * Env:
 *   MUSTARD_SKILL_VALIDATE_GATE_MODE = off | warn (default) | strict
 *
 * Fail-open: any internal error → exit 0 with "allow".
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
let emitMetric = () => {};
try { emitMetric = require('./_lib/metrics-emit.js').emitMetric; } catch (_) {}

function getMode() {
  const raw = (process.env.MUSTARD_SKILL_VALIDATE_GATE_MODE || 'warn').toLowerCase();
  if (raw === 'off' || raw === 'warn' || raw === 'strict') return raw;
  return 'warn';
}

function isSkillPath(filePath) {
  if (!filePath) return false;
  const p = filePath.replace(/\\/g, '/');
  return /\/SKILL\.md$/.test(p) || p === 'SKILL.md';
}

function simulateEdit(toolInput) {
  const filePath = toolInput.file_path;
  let current;
  try {
    current = fs.readFileSync(filePath, 'utf8');
  } catch (_) {
    current = '';
  }
  const oldStr = toolInput.old_string || '';
  const newStr = toolInput.new_string || '';
  if (toolInput.replace_all) {
    return current.split(oldStr).join(newStr);
  }
  const idx = current.indexOf(oldStr);
  if (idx === -1) return current;
  return current.slice(0, idx) + newStr + current.slice(idx + oldStr.length);
}

function loadValidator() {
  const candidates = [
    path.join(process.env.CLAUDE_PROJECT_DIR || process.cwd(), '.claude', 'scripts', 'skill-validate.js'),
    path.resolve(__dirname, '..', 'scripts', 'skill-validate.js'),
  ];
  for (const candidate of candidates) {
    try {
      if (fs.existsSync(candidate)) return require(candidate);
    } catch (_) { /* try next */ }
  }
  return null;
}

function deny(reason) {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'deny',
      permissionDecisionReason: reason,
    },
  }) + '\n');
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    const mode = getMode();
    if (mode === 'off') { process.exit(0); }

    let data;
    try { data = JSON.parse(input); } catch (_) { process.exit(0); }

    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || '';
    if (!isSkillPath(filePath)) { process.exit(0); }

    const toolName = data.tool_name || '';
    let content;
    if (toolName === 'Write') {
      content = toolInput.content || '';
    } else if (toolName === 'Edit') {
      content = simulateEdit(toolInput);
    } else {
      process.exit(0);
    }

    const validator = loadValidator();
    if (!validator || typeof validator.validateSkill !== 'function') {
      // Validator unavailable — fail open silently.
      process.exit(0);
    }

    const result = validator.validateSkill(content);
    if (result.ok) { process.exit(0); }

    try {
      emitMetric('skill-validate-gate', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: mode === 'strict' ? 'blocked' : 'warned',
        extras: { errors: (result.errors || []).length, file: path.basename(filePath) },
        cwd: data.cwd,
      });
    } catch (_) {}

    const errorList = (result.errors || []).map(e => `  - ${e}`).join('\n');
    const reason =
      `[skill-validate-gate] SKILL.md fails structural validation:\n${errorList}\n` +
      `Run \`bun .claude/scripts/skill-validate.js\` for details.`;

    if (mode === 'strict') {
      deny(reason);
      process.exit(0);
    }

    // warn mode: advisory only
    process.stderr.write(reason + '\n');
    process.exit(0);
  } catch (err) {
    process.stderr.write(`[skill-validate-gate] Error (fail-open): ${err.message}\n`);
    process.exit(0);
  }
});
