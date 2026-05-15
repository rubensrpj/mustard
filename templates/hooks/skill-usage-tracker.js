#!/usr/bin/env bun
'use strict';
/**
 * skill-usage-tracker: PostToolUse hook (matcher: Skill) that records every
 * Skill invocation as a `skill.invoked` event on the harness event bus.
 *
 * Fail-open: any error → exit 0 silently. Never writes to stdout (no decision).
 * Output: side-effect only (appends one JSONL line to `.harness/events.jsonl`,
 * and — when `MUSTARD_HARNESS_DUAL_EMIT=1` — also one row in `mustard.db`).
 *
 * Payload shape:
 *   { skill: <string>, args: <string ≤200 chars>, is_error?: true }
 *
 * @version 1.0.0
 */

let shouldRun;
try { ({ shouldRun } = require('./_lib/hook-env.js')); }
catch (_) { shouldRun = () => true; }

let emit, getCurrentSessionId, getCurrentWave, getCurrentSpec;
try {
  ({ emit, getCurrentSessionId, getCurrentWave, getCurrentSpec } = require('./_lib/harness-event.js'));
} catch (_) {
  emit = () => false;
  getCurrentSessionId = () => null;
  getCurrentWave = () => 0;
  getCurrentSpec = () => null;
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('skill-usage-tracker')) { process.exit(0); }

    const data = JSON.parse(input);
    if ((data.tool_name || '') !== 'Skill') { process.exit(0); }

    const toolInput = (data.tool_input && typeof data.tool_input === 'object') ? data.tool_input : {};
    const skillName = (toolInput.skill || toolInput.name || 'unknown').toString();
    const argsRaw = (toolInput.args == null ? '' : String(toolInput.args));

    const payload = {
      skill: skillName,
      args: argsRaw.slice(0, 200),
    };
    const resp = data.tool_response;
    if (resp && resp.is_error === true) payload.is_error = true;

    const projectDir = data.cwd || process.env.CLAUDE_PROJECT_DIR || process.cwd();

    emit('skill.invoked', payload, {
      cwd: projectDir,
      sessionId: getCurrentSessionId(data),
      wave: getCurrentWave(data),
      spec: getCurrentSpec(data),
      actor: { kind: 'hook', id: 'skill-usage-tracker' },
    });

    process.exit(0);
  } catch (_) {
    process.exit(0); // fail-open
  }
});
