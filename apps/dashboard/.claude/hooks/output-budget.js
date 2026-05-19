#!/usr/bin/env node
'use strict';
/**
 * OUTPUT-BUDGET: PostToolUse hook that measures agent response size after
 * Task completion. Emits metrics and injects an advisory warning when the
 * response exceeds the role line budget. Does NOT block or modify the response.
 *
 * Budget thresholds (line count of tool_response):
 *   Explore                      →  30 lines
 *   general-purpose (impl)       →  40 lines
 *   general-purpose (review)     →  60 lines
 *   Plan                         →  80 lines
 *
 * Classification of "review" vs "impl":
 *   If description includes 'review' → review budget
 *   Otherwise → impl budget
 *
 * @version 1.0.0
 */

const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

/** Line budgets by role. */
const BUDGETS = {
  explore: 30,
  'general-purpose(review)': 60,
  'general-purpose': 40,
  plan: 80,
};

/**
 * Classify Task role into a budget key and return its line limit.
 * @param {string} subagentType  e.g. "Explore", "Plan", "general-purpose"
 * @param {string} description   Task description string
 * @returns {{ role: string, limit: number }}
 */
function getRoleAndLimit(subagentType, description) {
  const type = (subagentType || '').toLowerCase();
  const desc = (description || '').toLowerCase();

  if (type === 'explore') return { role: 'Explore', limit: BUDGETS.explore };
  if (type === 'plan')    return { role: 'Plan',    limit: BUDGETS.plan };
  if (type === 'general-purpose') {
    if (desc.includes('review')) {
      return { role: 'general-purpose(review)', limit: BUDGETS['general-purpose(review)'] };
    }
    return { role: 'general-purpose', limit: BUDGETS['general-purpose'] };
  }
  // Unknown type: use general-purpose impl budget as a safe default
  return { role: type || 'unknown', limit: BUDGETS['general-purpose'] };
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { input += chunk; });
process.stdin.on('end', () => {
  try {
    if (!shouldRun('output-budget')) { process.exit(0); }

    const data = JSON.parse(input);
    const toolName = data.tool_name || '';

    // Only act on Task completions
    if (toolName !== 'Task') { process.exit(0); }

    const toolInput    = data.tool_input    || {};
    const toolResponse = data.tool_response || '';

    // If no response captured, nothing to measure
    if (!toolResponse || typeof toolResponse !== 'string') { process.exit(0); }

    const subagentType = toolInput.subagent_type || '';
    const description  = toolInput.description   || '';

    const { role, limit } = getRoleAndLimit(subagentType, description);
    const actual         = toolResponse.split('\n').length;
    const tokensAffected = Math.round(toolResponse.length / 4);
    const overBudget     = actual > limit;
    const overBy         = overBudget ? actual - limit : 0;

    if (overBudget) {
      emitMetric('output-budget', {
        tokensAffected,
        tokensSaved: 0,
        note: 'over-budget',
        extras: { role, actual_lines: actual, limit, over_by: overBy },
      });

      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PostToolUse',
          additionalContext:
            `[Output Budget] Agent response exceeded return cap. Role: ${role} | Limit: ${limit} lines | Actual: ${actual} lines. Future dispatches: focus on files changed + non-obvious decisions + blockers only.`,
        },
      }) + '\n');
    } else {
      emitMetric('output-budget', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'passed',
        extras: { role, actual_lines: actual, limit },
      });
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[output-budget] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
