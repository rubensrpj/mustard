#!/usr/bin/env node
'use strict';
/**
 * CONTEXT-BUDGET: PreToolUse hook with two responsibilities:
 * 1. Advisory: warns when referenced .md files exceed ~50K tokens (existing logic, preserved)
 * 2. Enforcement: blocks Task dispatches whose prompt.length exceeds role budget
 *
 * IMPORTANT — what "budget" means here:
 *   Budgets apply to `input.prompt` (the Task briefing Claude sends to the subagent),
 *   NOT to context the subagent gathers internally via Grep/Read/Glob/etc.
 *   A 10K-char Explore budget limits the briefing text, not the agent's exploration scope.
 *
 * Role budgets (1 token ≈ 4 chars — conservative conversion):
 *   Explore              → 2,500 tokens × 4 = 10,000 chars  (HARD BLOCK)
 *   general-purpose+review → 3,000 tokens × 4 = 12,000 chars (HARD BLOCK)
 *   general-purpose      → 5,000 tokens × 4 = 20,000 chars  (HARD BLOCK)
 *   Plan                 → advisory only, no hard block
 *
 * @version 2.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

const MODE = process.env.CONTEXT_BUDGET_MODE || 'strict';
const METRICS_DIR = path.join(process.cwd(), '.claude', '.metrics');
const METRICS_FILE = path.join(METRICS_DIR, 'budget-observations.jsonl');

// Conservative regex: only match .claude/skills/**/*.md, .claude/context/**/*.md, SKILL.md references
const MD_REF_PATTERN = /\.claude\/(?:skills|context)\/[^\s"'`]+\.md|SKILL\.md/g;

const TOKEN_THRESHOLD = 50000; // advisory threshold (tokens)

// Role budgets in chars (1 token ≈ 4 chars)
const BUDGET_EXPLORE          = 10000; // 2,500 tokens × 4
const BUDGET_REVIEW           = 12000; // 3,000 tokens × 4  (general-purpose with "review" in description)
const BUDGET_GENERAL          = 20000; // 5,000 tokens × 4  (general-purpose, other)
// Plan: no hard budget — advisory only

/**
 * Classify the Task role and return the char budget (or null = advisory only).
 * @param {string} subagentType  e.g. "Explore", "Plan", "general-purpose"
 * @param {string} description   Task description string
 * @returns {number|null}
 */
function getBudget(subagentType, description) {
  const type = (subagentType || '').toLowerCase();
  const desc = (description || '').toLowerCase();

  if (type === 'explore') return BUDGET_EXPLORE;
  if (type === 'plan') return null; // advisory only
  if (type === 'general-purpose') {
    return desc.includes('review') ? BUDGET_REVIEW : BUDGET_GENERAL;
  }
  // Unknown types: no hard block
  return null;
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('context-budget')) { process.exit(0); }

    const data = JSON.parse(input);
    const event = data.hook_event_name || '';
    const toolName = data.tool_name || '';
    const projectDir = process.env.CLAUDE_PROJECT_DIR || data.cwd || process.cwd();
    const toolInput = data.tool_input || {};
    const prompt = toolInput.prompt || '';

    // ── ENFORCEMENT: block over-budget Task dispatches ──────────────────────
    if (event === 'PreToolUse' && toolName === 'Task' && prompt) {
      const subagentType = toolInput.subagent_type || '';
      const description  = toolInput.description   || '';
      const budget = getBudget(subagentType, description);

      if (budget !== null) {
        const actual = prompt.length;
        const limit = budget;
        const roleLabel = subagentType === 'general-purpose'
          ? (description.toLowerCase().includes('review') ? 'general-purpose(review)' : 'general-purpose')
          : subagentType;

        if (MODE === 'observe') {
          try {
            fs.mkdirSync(METRICS_DIR, { recursive: true });
            const entry = JSON.stringify({
              ts: new Date().toISOString(),
              role: roleLabel,
              actual_chars: actual,
              limit,
              would_block: actual > limit
            });
            fs.appendFileSync(METRICS_FILE, entry + '\n');
          } catch (_e) { /* fail-silent */ }
          process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n');
          process.exit(0);
        }

        if (MODE === 'warn') {
          const suffix = actual > limit ? ' WOULD_BLOCK' : '';
          process.stderr.write(`[budget-gate WARN] role=${roleLabel} actual=${actual} limit=${limit}${suffix}\n`);
          process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n');
          process.exit(0);
        }

        // MODE === 'strict' (default): hard-block if over budget
        if (actual > limit) {
          const limitTokens = Math.round(limit / 4);
          const actualTokens = Math.round(actual / 4);
          process.stdout.write(JSON.stringify({
            permissionDecision: 'deny',
            permissionDecisionReason:
              `[Context Budget] Task prompt exceeds role budget. ` +
              `Role: ${roleLabel} | Limit: ${limitTokens} tokens (~${limit} chars) | ` +
              `Actual: ${actualTokens} tokens (~${actual} chars). ` +
              `Trim the prompt or split the task.`
          }) + '\n');
          process.exit(0);
        }

        // Under budget → allow (fall through to advisory check below)
        process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n');
        process.exit(0);
      }
    }

    // ── ADVISORY: warn about total referenced .md file size ─────────────────
    if (!prompt) { process.exit(0); }

    const matches = prompt.match(MD_REF_PATTERN) || [];
    const uniquePaths = [...new Set(matches)];

    let totalBytes = 0;
    const CHUNK_SIZE = 25;
    if (uniquePaths.length > 50) {
      // Process in chunks of 25 to avoid blocking on large path sets
      for (let i = 0; i < uniquePaths.length; i += CHUNK_SIZE) {
        const chunk = uniquePaths.slice(i, i + CHUNK_SIZE);
        for (const relPath of chunk) {
          try {
            const absPath = path.join(projectDir, relPath);
            if (fs.existsSync(absPath)) {
              totalBytes += fs.statSync(absPath).size;
            }
          } catch (e) { /* skip unreadable paths */ }
        }
      }
    } else {
      for (const relPath of uniquePaths) {
        try {
          const absPath = path.join(projectDir, relPath);
          if (fs.existsSync(absPath)) {
            totalBytes += fs.statSync(absPath).size;
          }
        } catch (e) { /* skip unreadable paths */ }
      }
    }

    if (totalBytes === 0) { process.exit(0); }

    const estimatedTokens = Math.round(totalBytes / 4);

    if (estimatedTokens > TOKEN_THRESHOLD) {
      const kTokens = Math.round(estimatedTokens / 1000);
      console.log(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          additionalContext:
            '[Context Budget Advisory] Context budget warning: ~' + kTokens + 'K tokens will be loaded into this subagent (>50K threshold). Consider trimming recommended_skills or splitting the task.'
        }
      }));
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[context-budget] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
