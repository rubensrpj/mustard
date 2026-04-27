#!/usr/bin/env node
'use strict';
/**
 * skill-size-gate: PreToolUse hook — warns/blocks oversized SKILL.md files.
 *
 * Triggers on Write|Edit when file_path ends with /SKILL.md (any depth).
 *
 * Note: triggers on ALL SKILL.md writes, including generated ones
 * (templates/skills/skill-creator/SKILL.md is ~485 lines). In default
 * warn mode this is advisory only; strict mode would block.
 *
 * Thresholds:
 *   WARN_LINES        = 200  → advisory only (stderr)
 *   STRICT_WARN_LINES = 400  → stronger advisory (stderr)
 *   BLOCK_LINES       = 500  → deny in strict mode
 *
 * Env:
 *   MUSTARD_SKILL_SIZE_MODE = off | warn (default) | strict
 *
 * In warn mode: skip generated skills (file starts with <!-- mustard:generated -->)
 * In strict mode: apply to all SKILL.md files including generated ones.
 *
 * Fail-open: any internal error → exit 0.
 *
 * @version 1.0.0
 */

const fs = require('fs');

const WARN_LINES        = 200;
const STRICT_WARN_LINES = 400;
const BLOCK_LINES       = 500;

function getMode() {
  const raw = (process.env.MUSTARD_SKILL_SIZE_MODE || 'warn').toLowerCase();
  if (raw === 'off' || raw === 'warn' || raw === 'strict') return raw;
  return 'warn';
}

function isSkillPath(filePath) {
  if (!filePath) return false;
  const p = filePath.replace(/\\/g, '/');
  return /\/SKILL\.md$/.test(p) || p === 'SKILL.md';
}

function isGenerated(content) {
  return typeof content === 'string' && content.trimStart().startsWith('<!-- mustard:generated -->');
}

/**
 * For Edit: read current file, apply old_string → new_string (first match or replace_all).
 */
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

function countLines(content) {
  if (!content) return 0;
  return content.split('\n').length;
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

function allow() {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'allow',
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
    try {
      data = JSON.parse(input);
    } catch (_) {
      process.exit(0);
    }

    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || '';

    if (!isSkillPath(filePath)) {
      process.exit(0);
    }

    const toolName = data.tool_name || '';
    let content;
    if (toolName === 'Write') {
      content = toolInput.content || '';
    } else if (toolName === 'Edit') {
      content = simulateEdit(toolInput);
      if (content === null) { process.exit(0); }
    } else {
      process.exit(0);
    }

    // In warn mode: skip generated skills
    if (mode === 'warn' && isGenerated(content)) {
      process.exit(0);
    }

    const lines = countLines(content);

    if (lines >= BLOCK_LINES) {
      const msg = `[skill-size-gate] SKILL.md exceeds ${BLOCK_LINES} lines (${lines} lines) — split verbose sections into references/examples.md`;
      process.stderr.write(msg + '\n');
      if (mode === 'strict') {
        deny(msg);
        process.exit(0);
      } else {
        allow();
        process.exit(0);
      }
    } else if (lines >= STRICT_WARN_LINES) {
      process.stderr.write(`[skill-size-gate] WARN: SKILL.md is ${lines} lines (strict-warn threshold ${STRICT_WARN_LINES}) — consider splitting\n`);
    } else if (lines >= WARN_LINES) {
      process.stderr.write(`[skill-size-gate] ADVISORY: SKILL.md is ${lines} lines (warn threshold ${WARN_LINES})\n`);
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[skill-size-gate] Error (fail-open): ${err.message}\n`);
    process.exit(0);
  }
});
