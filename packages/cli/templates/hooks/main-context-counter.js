#!/usr/bin/env bun
'use strict';
/**
 * MAIN-CONTEXT-COUNTER: enforces L0 (Universal Delegation) on the orchestrator.
 *
 * The parent/main context is meant to coordinate, not implement. Every Read/
 * Edit/Bash it runs directly inflates its own context window — and once that
 * window fills, the harness silently compacts old messages, so the orchestrator
 * loses memory of earlier waves. In a long wave-plan that means waves 8-12 get
 * planned by an amnesic orchestrator. L0 exists to prevent exactly that.
 *
 * This hook counts main-context tool calls *between* Task dispatches:
 *   - A `Task` dispatch resets the counter (work was delegated — good).
 *   - Read/Edit/Write/Bash/Grep/Glob/NotebookEdit in the main context increment it.
 *   - Tool calls inside a subagent (SubagentStart..SubagentStop) are NOT counted —
 *     a subagent burning its own context is expected and fine.
 *
 * Modes (env MUSTARD_MAIN_BUDGET_MODE):
 *   - off    — disabled
 *   - warn   — default; stderr nudge at WARN_AT, never blocks
 *   - strict — warn at WARN_AT, deny at DENY_AT (forces a break-and-delegate)
 *
 * State: .claude/.agent-state/main-context.counter.json
 *   { mainCount, subagentDepth, lastResetAt, updatedAt }
 *
 * Fail-open: any error exits 0 without affecting the tool call.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

const WARN_AT = 8;
const DENY_AT = 12;
const COUNTED_TOOLS = new Set(['Read', 'Edit', 'Write', 'Bash', 'Grep', 'Glob', 'NotebookEdit']);
const COUNTER_FILE = 'main-context.counter.json';

function getMode() {
  return (process.env.MUSTARD_MAIN_BUDGET_MODE || 'warn').toLowerCase();
}

function counterPath(projectDir) {
  return path.join(projectDir, '.claude', '.agent-state', COUNTER_FILE);
}

function readState(projectDir) {
  const fallback = { mainCount: 0, subagentDepth: 0, lastResetAt: null, updatedAt: null };
  try {
    const p = counterPath(projectDir);
    if (!fs.existsSync(p)) return fallback;
    const parsed = JSON.parse(fs.readFileSync(p, 'utf8'));
    return {
      mainCount: Number.isFinite(parsed.mainCount) ? parsed.mainCount : 0,
      subagentDepth: Number.isFinite(parsed.subagentDepth) ? parsed.subagentDepth : 0,
      lastResetAt: parsed.lastResetAt || null,
      updatedAt: parsed.updatedAt || null,
    };
  } catch (_) {
    return fallback;
  }
}

function writeState(projectDir, state) {
  try {
    const dir = path.join(projectDir, '.claude', '.agent-state');
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    state.updatedAt = new Date().toISOString();
    fs.writeFileSync(counterPath(projectDir), JSON.stringify(state), 'utf8');
  } catch (_) { /* fail-open */ }
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('main-context-counter')) { process.exit(0); }
    const mode = getMode();
    if (mode === 'off') process.exit(0);

    const data = JSON.parse(input);
    const event = data.hook_event_name;
    const projectDir = data.cwd || process.cwd();
    const state = readState(projectDir);

    // ── Lifecycle: keep the subagent-depth gauge honest ──────────────────────
    if (event === 'SessionStart') {
      writeState(projectDir, { mainCount: 0, subagentDepth: 0, lastResetAt: new Date().toISOString() });
      process.exit(0);
    }
    if (event === 'SubagentStart') {
      state.subagentDepth += 1;
      writeState(projectDir, state);
      process.exit(0);
    }
    if (event === 'SubagentStop') {
      state.subagentDepth = Math.max(0, state.subagentDepth - 1);
      writeState(projectDir, state);
      process.exit(0);
    }

    if (event !== 'PreToolUse') process.exit(0);

    const tool = data.tool_name || '';

    // A Task/Agent dispatch IS delegation — reset the main-context counter.
    if (tool === 'Task' || tool === 'Agent') {
      state.mainCount = 0;
      state.lastResetAt = new Date().toISOString();
      writeState(projectDir, state);
      process.exit(0);
    }

    // Only count main-context work tools. Inside a subagent → not our concern.
    if (!COUNTED_TOOLS.has(tool)) process.exit(0);
    if (state.subagentDepth > 0) process.exit(0);

    state.mainCount += 1;
    const count = state.mainCount;
    writeState(projectDir, state);

    if (mode === 'strict' && count >= DENY_AT) {
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason:
            `[main-context-counter] ${count} tool calls in the main context without a Task dispatch ` +
            `(L0 Universal Delegation). Stop and delegate: dispatch a Task agent for this work so the ` +
            `orchestrator context stays lean. Set MUSTARD_MAIN_BUDGET_MODE=warn to allow with a warning.`,
        },
      }) + '\n');
      process.exit(0);
    }

    if (count === WARN_AT || (count > WARN_AT && (count - WARN_AT) % 4 === 0)) {
      process.stderr.write(
        `[main-context-counter] ${count} tool calls no main context sem delegar (L0). ` +
        `Considere dispatch via Task — cada Read/Edit direto infla o context do orquestrador e ` +
        `acelera o truncamento de mensagens antigas.\n`
      );
    }

    process.exit(0);
  } catch (_) {
    process.exit(0); // fail-open
  }
});
