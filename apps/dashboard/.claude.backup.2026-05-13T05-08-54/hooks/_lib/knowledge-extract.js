'use strict';
/**
 * KNOWLEDGE-EXTRACT: Shared pattern-detection logic for session-knowledge hooks.
 * Accepts pre-parsed state objects; caller is responsible for reading files.
 *
 * Emits both descriptive entries (backward-compat `description` field) and, when
 * heuristics fire, an additional `prescription` field with actionable guidance
 * plus a `prescriptive` tag so downstream readers can filter for them.
 *
 * @version 1.1.0
 */

/**
 * Build a prescription string based on metrics/toolBreakdown heuristics.
 * Returns null when no heuristic matches.
 *
 * Heuristics (non-mutually-exclusive; first-match-wins on name collisions, but
 * callers iterate per-state so each state produces at most one prescription).
 *
 * 1. L0 violation pattern: Bash + Edit heavily dominates Agent AND retries high
 *    → parent context was implementing instead of delegating.
 * 2. Scope-creep / fragmentation: high apiCalls with high retries
 *    → pipeline should have been split into smaller pipelines.
 * 3. Reactive trial-and-error: high Edit with low Write
 *    → insufficient upfront investigation (Read/Grep) before editing.
 *
 * @param {object} metrics  { retries, apiCalls, toolBreakdown: {Tool: count} }
 * @returns {string|null}
 */
function derivePrescription(metrics) {
  if (!metrics || typeof metrics !== 'object') return null;

  var breakdown = metrics.toolBreakdown || {};
  var bash = Number(breakdown.Bash) || 0;
  var edit = Number(breakdown.Edit) || 0;
  var write = Number(breakdown.Write) || 0;
  var agent = Number(breakdown.Agent) || 0;
  var retries = Number(metrics.retries) || 0;
  var apiCalls = Number(metrics.apiCalls) || 0;

  // Heuristic 1: L0 violation — parent did heavy Bash+Edit work that should
  // have been delegated via Task(general-purpose).
  if (bash + edit > 3 * agent && retries > 2) {
    return 'Next similar pipeline: delegate investigation via Task(general-purpose) ' +
      'BEFORE editing files in sequence. Dominant Bash+Edit without Agent indicates ' +
      'the parent did work that should have been delegated.';
  }

  // Heuristic 2: fragmentation needed — single pipeline ballooned past the
  // comfortable API/retry budget.
  if (apiCalls > 50 && retries > 3) {
    return 'Next similar pipeline: split into at least 2 smaller pipelines. ' +
      'A single scope with >50 API calls and >3 retries indicates scope-creep.';
  }

  // Heuristic 3: reactive iteration — lots of Edits with barely any Writes
  // suggests tweaking the same files repeatedly instead of planning first.
  if (edit > 15 && write < 3) {
    return 'Next similar pipeline: investigate with Read+Grep BEFORE editing. ' +
      'High Edit with low Write count indicates trial-and-error iteration.';
  }

  return null;
}

/**
 * Extract candidate knowledge patterns from an array of pipeline state objects.
 *
 * @param {object[]} stateObjects  Parsed .pipeline-states/*.json objects.
 *                                 Each may have: specName, metrics.retries,
 *                                 metrics.apiCalls, metrics.toolBreakdown
 * @returns {{ type: string, name: string, description: string, source: string, tags: string[], prescription?: string }[]}
 */
function extractPatternsFromStates(stateObjects) {
  var patterns = [];

  for (var i = 0; i < stateObjects.length; i++) {
    var state = stateObjects[i];
    if (!state || typeof state !== 'object') continue;

    var metrics = state.metrics || {};
    var label = state.specName || state._file || 'unknown';
    var prescription = derivePrescription(metrics);

    // High hook-retry count → lesson. Counts hook/sandbox events, not agent
    // redispatches — a clean Pass@1 pipeline can still accumulate dozens.
    if (metrics.retries && metrics.retries > 2) {
      var retryEntry = {
        type: 'convention',
        name: 'high-hook-retry-' + label,
        description: 'Pipeline triggered ' + metrics.retries + ' hook-level retries ' +
          '(sandbox/stash-pop/re-prompts — not agent redispatches). Tool breakdown: ' +
          JSON.stringify(metrics.toolBreakdown || {}),
        source: 'session-knowledge',
        tags: ['hook-retry', 'pipeline', 'lesson'],
      };
      if (prescription) {
        retryEntry.prescription = prescription;
        retryEntry.tags = retryEntry.tags.concat(['prescriptive']);
      }
      patterns.push(retryEntry);
    }

    // Heavy tool usage → optimization pattern
    var totalCalls = metrics.apiCalls || 0;
    if (totalCalls > 50) {
      var heavyEntry = {
        type: 'pattern',
        name: 'heavy-pipeline-' + label,
        description: 'Pipeline used ' + totalCalls + ' API calls. Consider splitting into smaller scope.',
        source: 'session-knowledge',
        tags: ['optimization', 'pipeline'],
      };
      if (prescription) {
        heavyEntry.prescription = prescription;
        heavyEntry.tags = heavyEntry.tags.concat(['prescriptive']);
      }
      patterns.push(heavyEntry);
    }
  }

  return patterns;
}

module.exports = { extractPatternsFromStates, derivePrescription };
