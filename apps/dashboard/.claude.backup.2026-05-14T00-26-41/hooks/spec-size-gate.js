#!/usr/bin/env bun
'use strict';
/**
 * spec-size-gate: PreToolUse hook — warns/blocks oversized spec files.
 *
 * Triggers on Write|Edit when file_path matches:
 *   .claude/spec/active/.../*.md
 *   .claude/spec/completed/.../*.md
 *   .../spec/.../*.md  (any .md inside a spec/ directory)
 *
 * Thresholds: warn 200 → strict-warn 400 → block 500.
 * Env: MUSTARD_SPEC_SIZE_MODE = off | warn (default) | strict
 *
 * Logic delegated to `_lib/size-gate.js#run` (shared with skill-size-gate.js).
 *
 * @version 2.0.0
 */

const { run } = require('./_lib/size-gate.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

function isSpecPath(filePath) {
  if (!filePath) return false;
  const p = filePath.replace(/\\/g, '/');
  if (/\.claude\/spec\/(active|completed)\/.+\.md$/.test(p)) return true;
  if (/\/spec\/.+\.md$/.test(p)) return true;
  return false;
}

run({
  name: 'spec-size-gate',
  envVar: 'MUSTARD_SPEC_SIZE_MODE',
  defaultMode: 'warn',
  isTargetPath: isSpecPath,
  thresholds: { warn: 200, strictWarn: 400, block: 500 },
  blockReason: (lines) =>
    `[spec-size-gate] spec exceeds 500 lines (${lines} lines) — split into references/{section}.md (see feature/SKILL.md § Spec Layout)`,
  onDecision: ({ lines, decision, filePath }) => {
    emitMetric('spec-size-gate', {
      tokensAffected: 0,
      tokensSaved: 0,
      note: decision === 'blocked' ? 'blocked' : 'over-size',
      extras: { lines, limit: 500, file: filePath, category: decision === 'blocked' ? 'prevention' : 'workflow' },
    });
  },
});
