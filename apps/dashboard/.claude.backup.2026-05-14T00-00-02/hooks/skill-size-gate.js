#!/usr/bin/env bun
'use strict';
/**
 * skill-size-gate: PreToolUse hook — warns/blocks oversized SKILL.md files.
 *
 * Triggers on Write|Edit when file_path ends with /SKILL.md (any depth).
 *
 * Thresholds: warn 200 → strict-warn 400 → block 500.
 * Env: MUSTARD_SKILL_SIZE_MODE = off | warn (default) | strict
 *
 * In warn mode: skip generated skills (file starts with <!-- mustard:generated -->).
 * In strict mode: apply to all SKILL.md files including generated ones.
 *
 * Logic delegated to `_lib/size-gate.js#run` (shared with spec-size-gate.js).
 *
 * @version 2.0.0
 */

const { run } = require('./_lib/size-gate.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

function isSkillPath(filePath) {
  if (!filePath) return false;
  const p = filePath.replace(/\\/g, '/');
  return /\/SKILL\.md$/.test(p) || p === 'SKILL.md';
}

function skipGenerated(content, mode) {
  return mode === 'warn'
    && typeof content === 'string'
    && content.trimStart().startsWith('<!-- mustard:generated -->');
}

run({
  name: 'skill-size-gate',
  envVar: 'MUSTARD_SKILL_SIZE_MODE',
  defaultMode: 'warn',
  isTargetPath: isSkillPath,
  thresholds: { warn: 200, strictWarn: 400, block: 500 },
  blockReason: (lines) =>
    `[skill-size-gate] SKILL.md exceeds 500 lines (${lines} lines) — split verbose sections into references/examples.md`,
  skipWhen: skipGenerated,
  onDecision: ({ lines, decision, filePath }) => {
    emitMetric('skill-size-gate', {
      tokensAffected: 0,
      tokensSaved: 0,
      note: decision === 'blocked' ? 'blocked' : 'over-size',
      extras: { lines, limit: 500, file: filePath, category: decision === 'blocked' ? 'prevention' : 'workflow' },
    });
  },
});
