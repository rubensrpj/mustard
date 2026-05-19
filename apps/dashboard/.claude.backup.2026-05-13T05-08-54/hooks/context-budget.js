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
 *   general-purpose      → 7,500 tokens × 4 = 30,000 chars  (HARD BLOCK)
 *   Plan                 → advisory only, no hard block
 *
 * The general-purpose budget was raised to 30k chars (from 20k) when the /scan
 * orchestrator started inlining cluster slices + sample code in the prompt —
 * agents need that pre-extracted context to skip Reads, but the prompt itself
 * grew accordingly.
 *
 * @version 2.1.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

function getMode() {
  if (process.env.CONTEXT_BUDGET_MODE) return process.env.CONTEXT_BUDGET_MODE;
  try {
    const modeFile = path.join(process.cwd(), '.claude', '.metrics', '.mode');
    if (fs.existsSync(modeFile)) return fs.readFileSync(modeFile, 'utf8').trim();
  } catch (_) { /* fail-silent, fallback to default */ }
  return 'strict';
}

const MODE = getMode();

// Conservative regex: only match .claude/skills/**/*.md, .claude/context/**/*.md, SKILL.md references
const MD_REF_PATTERN = /\.claude\/(?:skills|context)\/[^\s"'`]+\.md|SKILL\.md/g;

// ─── Dumb Zone (canonical) ────────────────────────────────────────────────────
// Source: Dex Horthy (HumanLayer) + Liu et al. 2023 (arXiv:2307.03172, "Lost in
// the Middle"). Consensus: ≥40% of model window degrades reasoning ("Dumb Zone").
// We use 40% as WARN threshold and 65% as compact-suggestion threshold.
const WINDOW_BY_MODEL = {
  haiku:  200_000,
  sonnet: 200_000,
  opus:   200_000,
};
const OPUS_1M_WINDOW   = 1_000_000;
const DEFAULT_WINDOW   = 200_000;
const DUMB_ZONE_PCT    = 0.40; // warn at this % of window
const COMPACT_PCT      = 0.65; // advise /compact at this %

/**
 * Resolve the model window in tokens from a model id string.
 * Recognises haiku/sonnet/opus + the "1m" suffix (e.g. "claude-opus-4-7-1m").
 * Falls back to DEFAULT_WINDOW (200K) when unknown.
 * @param {string} modelId
 * @returns {number}
 */
function resolveWindow(modelId) {
  const s = (modelId || '').toLowerCase();
  if (!s) return DEFAULT_WINDOW;
  if (/[\[\(\-_]1m[\]\)\-_]?$|1m\b/.test(s)) return OPUS_1M_WINDOW;
  if (s.includes('haiku'))  return WINDOW_BY_MODEL.haiku;
  if (s.includes('sonnet')) return WINDOW_BY_MODEL.sonnet;
  if (s.includes('opus'))   return WINDOW_BY_MODEL.opus;
  return DEFAULT_WINDOW;
}

// Legacy absolute fallback: kept ONLY when no model context is available.
const LEGACY_TOKEN_THRESHOLD = 50000;

// Role budgets in chars (1 token ≈ 4 chars)
const BUDGET_EXPLORE          = 10000; // 2,500 tokens × 4
const BUDGET_REVIEW           = 12000; // 3,000 tokens × 4  (general-purpose with "review" in description)
const BUDGET_GENERAL          = 30000; // 7,500 tokens × 4  (general-purpose, other) — raised from 20k to fit /scan's inlined cluster + sample blocks
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

        // Emit only when actionable: a block, or a near-miss above 90% of limit.
        // Under-budget passes are no-ops for token economy and used to inflate
        // tokens_affected without contributing to tokens_saved.
        const wouldBlock = actual > limit;
        const nearMiss = actual > limit * 0.9;
        if (wouldBlock || nearMiss) {
          emitMetric('budget-check', {
            tokensAffected: Math.round(actual / 4),
            tokensSaved: wouldBlock ? Math.max(0, Math.round((actual - limit) / 4)) : 0,
            note: wouldBlock ? 'blocked' : 'near-miss',
            extras: {
              role: roleLabel,
              actual_chars: actual,
              limit,
              would_block: wouldBlock,
              mode: MODE,
              category: wouldBlock ? 'prevention' : 'routing-advisory',
            },
          });
        }

        // Apply mode decision (separate concern):
        if (MODE === 'observe') {
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

    // ── ADVISORY: Dumb Zone (% window) + legacy .md size ─────────────────────
    // Compute referenced .md tokens (if any) + prompt tokens.
    let totalBytes = 0;
    if (prompt) {
      const matches = prompt.match(MD_REF_PATTERN) || [];
      const uniquePaths = [...new Set(matches)];

      const CHUNK_SIZE = 25;
      if (uniquePaths.length > 50) {
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
    }

    const refTokens   = Math.round(totalBytes / 4);
    const promptTokens = Math.round((prompt || '').length / 4);
    const totalTokens = refTokens + promptTokens;

    if (totalTokens === 0) { process.exit(0); }

    // Resolve model window from payload.model (Claude Code passes it on Task)
    // or fallback to env var, then DEFAULT_WINDOW.
    const modelHint = data.model || toolInput.model || process.env.MUSTARD_MODEL_HINT || '';
    const windowTokens = resolveWindow(modelHint);
    const pct = totalTokens / windowTokens;

    let advisory = null;

    if (pct >= COMPACT_PCT) {
      // Above 65% — strongly suggest /compact
      const kTokens = Math.round(totalTokens / 1000);
      const pctRounded = Math.round(pct * 100);
      advisory =
        `[Dumb Zone — Compact Now] Estimated context ~${kTokens}K tokens = ${pctRounded}% of ${Math.round(windowTokens / 1000)}K window. ` +
        `Above ${Math.round(COMPACT_PCT * 100)}% reasoning quality drops sharply (Liu et al. 2023). ` +
        `Run /compact then /resume, or split the task.`;
    } else if (pct >= DUMB_ZONE_PCT) {
      // Above 40% — Dumb Zone warning
      const kTokens = Math.round(totalTokens / 1000);
      const pctRounded = Math.round(pct * 100);
      advisory =
        `[Dumb Zone Advisory] Estimated context ~${kTokens}K tokens = ${pctRounded}% of ${Math.round(windowTokens / 1000)}K window. ` +
        `≥${Math.round(DUMB_ZONE_PCT * 100)}% degrades reasoning ("Dumb Zone", Dex Horthy / Liu et al. 2023). ` +
        `Consider trimming recommended_skills, narrowing scope, or running /compact.`;
    } else if (refTokens > LEGACY_TOKEN_THRESHOLD && !modelHint) {
      // Legacy fallback: when no model hint, retain old 50K absolute warn for .md refs
      const kTokens = Math.round(refTokens / 1000);
      advisory =
        `[Context Budget Advisory] ~${kTokens}K tokens of .md refs loaded (>50K). Trim recommended_skills or split task.`;
    }

    if (advisory) {
      console.log(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          additionalContext: advisory,
        }
      }));
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[context-budget] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
