#!/usr/bin/env bun
'use strict';
/**
 * MODEL-ROUTING-GATE: PreToolUse hook that validates the model selected for
 * Task/Agent dispatches against the pipeline's model routing table.
 *
 * Routing table:
 *   Explore agents           → haiku  (mechanical search only)
 *   Plan agents              → opus   (bad plan = bad implementation)
 *   Feature pipeline (any)   → opus   (quality-first)
 *   Bugfix pipeline          → opus   (diagnosis needs deep reasoning)
 *   Everything else          → sonnet (safe default)
 *
 * Upgrades are blocked (e.g., expected haiku but got opus).
 * Downgrades are allowed (saving money is fine).
 *
 * MODE (MUSTARD_MODEL_GATE_MODE env var):
 *   strict — deny with reason on upgrade violations (DEFAULT)
 *   warn   — advisory additionalContext, always allow
 *   off    — completely skip all checks
 *
 * Fail-open: exits 0 on any error — never blocks due to hook bugs.
 *
 * @version 1.0.0
 */

const fs   = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

// ── Cost rank: higher = more expensive ──────────────────────────────────────
const MODEL_COST_RANK = { haiku: 1, sonnet: 2, opus: 3 };

/**
 * Normalise a raw model string to one of the rank keys, or null if unknown.
 * Handles strings like "claude-3-haiku-20240307", "claude-sonnet-4-5", "opus", etc.
 * @param {string} raw
 * @returns {'haiku'|'sonnet'|'opus'|null}
 */
function normalizeModel(raw) {
  const s = (raw || '').toLowerCase();
  if (s.includes('haiku'))  return 'haiku';
  if (s.includes('opus'))   return 'opus';
  if (s.includes('sonnet')) return 'sonnet';
  return null;
}

/**
 * Find the newest .json pipeline-state file (excludes .metrics.json).
 * Returns parsed state object or null.
 * @param {string} projectDir
 * @returns {object|null}
 */
function loadNewestPipelineState(projectDir) {
  try {
    const statesDir = path.join(projectDir, '.claude', '.pipeline-states');
    if (!fs.existsSync(statesDir)) return null;

    const files = fs.readdirSync(statesDir)
      .filter(f => f.endsWith('.json') && !f.endsWith('.metrics.json'));
    if (files.length === 0) return null;

    // Sort by mtime descending, pick newest
    const sorted = files
      .map(f => {
        try {
          const fp = path.join(statesDir, f);
          return { f, mtime: fs.statSync(fp).mtimeMs, fp };
        } catch (_) {
          return null;
        }
      })
      .filter(Boolean)
      .sort((a, b) => b.mtime - a.mtime);

    if (sorted.length === 0) return null;

    const content = fs.readFileSync(sorted[0].fp, 'utf8');
    return JSON.parse(content);
  } catch (_) {
    return null;
  }
}

// No non-code signal matching — Haiku is restricted to Explore agents only.
// Everything involving analysis, judgment, or reasoning uses Sonnet or Opus.

/**
 * Determine the expected model and the human-readable reason.
 *
 * Model philosophy (quality-first):
 *   - Explore          → haiku  (mechanical file search, no judgment)
 *   - Feature pipeline → opus   (quality matters most)
 *   - Bugfix pipeline  → sonnet (focused fix)
 *   - Everything else  → sonnet (safe default for analysis, review, planning)
 *
 * @param {string} subagentType  e.g. 'Explore', 'general-purpose', 'Plan', 'Bash'
 * @param {string} description   Task description text
 * @param {object|null} state    Parsed pipeline state (or null)
 * @returns {{ expected: string, reason: string }}
 */
function determineExpected(subagentType, description, state) {
  const agentType = (subagentType || '').toLowerCase();

  // Rule 1: Explore is purely mechanical search → haiku
  if (agentType === 'explore') {
    return { expected: 'haiku', reason: 'Explore agents use haiku (mechanical search)' };
  }

  // Rule 2: Plan needs deep reasoning — bad plan = bad implementation → opus
  if (agentType === 'plan') {
    return { expected: 'opus', reason: 'Plan agents use opus (architectural reasoning)' };
  }

  // Rule 2.5: Description-verb override — analysis/review tasks at the start
  // of the description ("Review X", "Audit Y", "Validate Z") route to sonnet.
  // Opt-out: high-stakes keywords keep opus depth (security audits, critical
  // path verification, production-risk inspection).
  const descRaw = description || '';
  const descLower = descRaw.trim().toLowerCase();
  const isAnalysisVerb = /^(review|audit|validate|verify|check|inspect)\b/.test(descLower);
  const isHighStakes = /\b(security|critical|production)\b/i.test(descRaw);
  if (isAnalysisVerb && !isHighStakes) {
    return { expected: 'sonnet', reason: 'Analysis/review task — sonnet sufficient' };
  }

  // Rule 3: Active pipeline drives model
  if (state && state.type) {
    const pipelineType = (state.type || '').toLowerCase();

    if (pipelineType === 'feature') {
      return { expected: 'opus', reason: 'Feature pipelines use opus (quality-first)' };
    }

    if (pipelineType === 'bugfix') {
      return { expected: 'opus', reason: 'Bugfix pipelines use opus (diagnosis needs deep reasoning)' };
    }
  }

  // Default: sonnet for everything else (plan, review, audit, analysis, etc.)
  return { expected: 'sonnet', reason: 'Default model (analysis/review/planning)' };
}

// ── Mode resolution ──────────────────────────────────────────────────────────
function getMode() {
  const raw = (process.env.MUSTARD_MODEL_GATE_MODE || 'strict').toLowerCase();
  if (raw === 'strict' || raw === 'off' || raw === 'warn') return raw;
  return 'strict';
}

// ── Main ─────────────────────────────────────────────────────────────────────
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('model-routing-gate')) { process.exit(0); }

    const data       = JSON.parse(input);
    const toolName   = data.tool_name || '';

    // Only act on Task or Agent tool dispatches
    if (toolName !== 'Task' && toolName !== 'Agent') { process.exit(0); }

    const toolInput    = data.tool_input    || {};
    const rawModel     = toolInput.model    || '';
    const subagentType = toolInput.subagent_type || '';
    const description  = toolInput.description  || '';
    const projectDir   = process.env.CLAUDE_PROJECT_DIR || data.cwd || process.cwd();

    // If no model specified, check whether the agent is an explorer type.
    // Explorers (subagent_type === 'Explore' or containing 'explorer') MUST
    // specify model explicitly — inheriting the parent (usually opus) costs
    // ~3-10x more than the required haiku/sonnet.  All other agent types
    // fall back to the existing advisory path.
    if (!rawModel) {
      const state = loadNewestPipelineState(projectDir);
      const { expected, reason } = determineExpected(subagentType, description, state);
      const agentTypeLower = (subagentType || '').toLowerCase();
      const isExplorer = agentTypeLower === 'explore' || agentTypeLower.includes('explorer');

      if (isExplorer) {
        emitMetric('model-routing-gate', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'no-model-denied',
          extras: {
            expected,
            actual: 'inherited',
            pipeline_type: state ? (state.type || 'unknown') : 'none',
            scope:         state ? (state.scope || 'unknown') : 'none',
            reason,
            subagent_type: subagentType,
            category: 'prevention',
          },
        });

        process.stdout.write(JSON.stringify({
          permissionDecision: 'deny',
          permissionDecisionReason:
            `[Model Routing] Explorer agents must specify model explicitly (haiku or sonnet). ` +
            `Add model: "haiku" to your Task dispatch. ` +
            `${reason}.`,
        }) + '\n');
        process.exit(0);
      }

      // Non-explorer: when expected is sonnet the orchestrator is almost
      // certainly inheriting opus from the parent — deny and require the
      // model to be specified explicitly. When expected is opus the inherited
      // value matches, so allow silently. `warn` mode keeps the legacy
      // advisory behaviour for users who want the old escape valve.
      const mode = getMode();
      if (expected === 'sonnet' && mode === 'strict') {
        emitMetric('model-routing-gate', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'no-model-denied-sonnet',
          extras: {
            expected,
            actual: 'inherited',
            pipeline_type: state ? (state.type || 'unknown') : 'none',
            scope:         state ? (state.scope || 'unknown') : 'none',
            reason,
            subagent_type: subagentType,
            category: 'prevention',
          },
        });

        process.stdout.write(JSON.stringify({
          permissionDecision: 'deny',
          permissionDecisionReason:
            `[Model Routing] No model specified — this task should use model: '${expected}'. ` +
            `${reason}. Add model: '${expected}' to your Task dispatch ` +
            `(or set MUSTARD_MODEL_GATE_MODE=warn to downgrade to an advisory).`,
        }) + '\n');
        process.exit(0);
      }

      if (expected !== 'opus') {
        emitMetric('model-routing-gate', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'no-model-advisory',
          extras: {
            expected,
            actual: 'inherited',
            pipeline_type: state ? (state.type || 'unknown') : 'none',
            scope:         state ? (state.scope || 'unknown') : 'none',
            reason,
            subagent_type: subagentType,
            category: 'routing-advisory',
          },
        });

        process.stdout.write(JSON.stringify({
          hookSpecificOutput: {
            hookEventName: 'PreToolUse',
            additionalContext:
              `[Model Gate] No model specified — this task should use model: '${expected}'. ` +
              `${reason}. Add model: '${expected}' to reduce costs.`,
          },
        }) + '\n');
      }
      process.exit(0);
    }

    const model = normalizeModel(rawModel);
    if (!model) { process.exit(0); } // Unknown model name — can't rank, skip

    const mode = getMode();
    if (mode === 'off') { process.exit(0); }

    // Resolve expected model
    const state              = loadNewestPipelineState(projectDir);
    const { expected, reason } = determineExpected(subagentType, description, state);

    const modelRank    = MODEL_COST_RANK[model]    || 2; // unknown → sonnet tier
    const expectedRank = MODEL_COST_RANK[expected] || 2;

    const isViolation = modelRank > expectedRank;
    const noteLabel   = isViolation ? 'violation' : 'passed';

    // Emit metric on every gate check. tokens_saved stays 0 — routing is a
    // routing decision, not a recurring saving. Strict-mode blocks are tracked
    // via `category: 'prevention'`; non-blocking checks are 'routing'.
    emitMetric('model-routing-gate', {
      tokensAffected: 0,
      tokensSaved: 0,
      note: noteLabel,
      extras: {
        expected,
        actual: model,
        pipeline_type: state ? (state.type || 'unknown') : 'none',
        scope:         state ? (state.scope || 'unknown') : 'none',
        reason,
        mode,
        subagent_type: subagentType,
        category: isViolation && mode === 'strict' ? 'prevention' : 'routing',
      },
    });

    if (!isViolation) { process.exit(0); }

    // ── Gate violation ───────────────────────────────────────────────────────
    if (mode === 'warn') {
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          additionalContext:
            `[Model Gate] Expected ${expected} for this task (${reason}). ` +
            `Consider using model: '${expected}' to reduce costs.`,
        },
      }) + '\n');
      process.exit(0);
    }

    // mode === 'strict'
    process.stdout.write(JSON.stringify({
      permissionDecision: 'deny',
      permissionDecisionReason:
        `[Model Gate] Task requires '${expected}' model, not '${model}'. ` +
        `Reason: ${reason}. Re-dispatch with model: '${expected}'.`,
    }) + '\n');
    process.exit(0);

  } catch (err) {
    process.stderr.write('[model-routing-gate] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});

