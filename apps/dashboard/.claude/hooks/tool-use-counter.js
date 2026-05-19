#!/usr/bin/env node
'use strict';
/**
 * TOOL-USE COUNTER: Counts tool uses per active Explore subagent and enforces a hard cap.
 *
 * Handles 4 events:
 * - SubagentStart:  Creates a counter file for Explore agents + injects budget reminder
 * - SubagentStop:   Removes counter file
 * - PreToolUse:     Increments counter; denies if hard limit exceeded; warns at threshold
 * - SessionStart:   Deletes all stale *.counter.json files (fresh session = clean counters)
 *
 * Counter file: .claude/.agent-state/{agent_id}.counter.json
 * Format: { type, limit, warnAt, count, createdAt }
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

const HARD_LIMIT = 20;
const WARN_THRESHOLD = 15;
// Explore agents get a tighter budget: warn at 12, deny at 15.
// Parallel tool calls cause the deny-at-20 to arrive too late (agents reach 27+).
const EXPLORE_LIMIT = 15;
const EXPLORE_WARN = 12;
const ENFORCED_TYPES = new Set(['Explore']);
const COUNTER_STALE_MS = 10 * 60 * 1000; // 10 minutes

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('tool-use-counter')) { process.exit(0); }
    const data = JSON.parse(input);
    const event = data.hook_event_name;
    const projectDir = data.cwd || process.cwd();
    const stateDir = path.join(projectDir, '.claude', '.agent-state');

    if (event === 'SubagentStart') {
      handleStart(data, stateDir);
    } else if (event === 'SubagentStop') {
      handleStop(data, stateDir);
    } else if (event === 'PreToolUse') {
      handlePreToolUse(data, stateDir);
    } else if (event === 'SessionStart') {
      handleSessionStart(stateDir);
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[tool-use-counter] Error: ${err.message}\n`);
    process.exit(0);
  }
});

/**
 * SubagentStart: Create a counter file for Explore agents and inject a budget reminder.
 */
function handleStart(data, stateDir) {
  const agentId = data.agent_id || `unknown-${Date.now()}`;
  const agentType = data.agent_type || 'unknown';

  if (!ENFORCED_TYPES.has(agentType)) {
    // No counter needed for non-enforced types
    process.exit(0);
  }

  ensureDir(stateDir);

  const counterFile = path.join(stateDir, `${agentId}.counter.json`);
  const limit = agentType === 'Explore' ? EXPLORE_LIMIT : HARD_LIMIT;
  const warnAt = agentType === 'Explore' ? EXPLORE_WARN : WARN_THRESHOLD;
  const counter = {
    type: agentType,
    limit,
    warnAt,
    count: 0,
    createdAt: new Date().toISOString(),
  };
  fs.writeFileSync(counterFile, JSON.stringify(counter, null, 2), 'utf8');

  const response = {
    hookSpecificOutput: {
      hookEventName: 'SubagentStart',
      additionalContext:
        `[Tool Budget] This agent has a ${limit}-tool-use budget. ` +
        `Use Grep over Read where possible. Return findings as soon as root cause is clear.`,
    },
  };
  console.log(JSON.stringify(response));
}

/**
 * SubagentStop: Remove the counter file for the stopped agent.
 */
function handleStop(data, stateDir) {
  const agentId = data.agent_id || '';
  if (!agentId) return;

  const counterFile = path.join(stateDir, `${agentId}.counter.json`);
  try {
    if (fs.existsSync(counterFile)) {
      fs.unlinkSync(counterFile);
    }
  } catch {}
}

/**
 * PreToolUse: Find all active counter files, increment counts, enforce limits.
 * If ANY counter exceeds the hard limit → deny. Warn at threshold.
 */
function handlePreToolUse(data, stateDir) {
  // Fast path: no state dir means no active Explore agents
  if (!fs.existsSync(stateDir)) {
    process.exit(0);
  }

  let counterFiles;
  try {
    counterFiles = fs.readdirSync(stateDir).filter(f => f.endsWith('.counter.json'));
  } catch {
    process.exit(0);
  }

  // Fast path: no counter files
  if (counterFiles.length === 0) {
    process.exit(0);
  }

  const now = Date.now();
  let denyOutput = null;
  let warnOutput = null;

  for (const file of counterFiles) {
    const filePath = path.join(stateDir, file);
    let counter;

    try {
      counter = JSON.parse(fs.readFileSync(filePath, 'utf8'));
    } catch {
      // Corrupt counter file — skip
      continue;
    }

    // Stale check: delete and skip
    const age = now - new Date(counter.createdAt || 0).getTime();
    if (age > COUNTER_STALE_MS) {
      try { fs.unlinkSync(filePath); } catch {}
      continue;
    }

    // Increment count
    counter.count = (counter.count || 0) + 1;

    try {
      fs.writeFileSync(filePath, JSON.stringify(counter, null, 2), 'utf8');
    } catch {}

    const count = counter.count;
    const limit = counter.limit || HARD_LIMIT;
    const warnAt = counter.warnAt || WARN_THRESHOLD;

    if (count >= limit) {
      emitMetric('tool-use-counter', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'hard-limit',
        extras: {
          agent_type: counter.type,
          count: counter.count,
          limit: counter.limit,
          category: 'prevention',
        },
      });
      denyOutput = {
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason:
            `[Tool Budget] Explore agent reached ${limit} tool uses (limit). ` +
            `Wrap up your findings.`,
        },
      };
      // Deny takes priority — no need to check remaining counters
      break;
    }

    if (count === warnAt && !denyOutput) {
      emitMetric('tool-use-counter', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'warn-threshold',
        extras: {
          agent_type: counter.type,
          count: counter.count,
          limit: counter.limit,
          category: 'routing-advisory',
        },
      });
      // Only set warnOutput once (first counter to hit threshold wins)
      if (!warnOutput) {
        warnOutput = {
          hookSpecificOutput: {
            hookEventName: 'PreToolUse',
            additionalContext:
              `[Tool Budget] ${count}/${limit} tool uses. ` +
              `Begin wrapping up — return findings after completing current investigation.`,
          },
        };
      }
    }
  }

  if (denyOutput) {
    console.log(JSON.stringify(denyOutput));
  } else if (warnOutput) {
    console.log(JSON.stringify(warnOutput));
  }

  process.exit(0);
}

/**
 * SessionStart: Delete all stale *.counter.json files.
 * Fresh session = clean counters.
 */
function handleSessionStart(stateDir) {
  try {
    if (!fs.existsSync(stateDir)) return;
    const counterFiles = fs.readdirSync(stateDir).filter(f => f.endsWith('.counter.json'));
    for (const file of counterFiles) {
      try {
        fs.unlinkSync(path.join(stateDir, file));
      } catch {}
    }
  } catch {}
}

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}
