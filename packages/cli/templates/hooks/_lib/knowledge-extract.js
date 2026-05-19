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
 * NOTE: Friction telemetry (high hook-retry counts, heavy API usage) is NOT a
 * knowledge pattern — it is measured noise. Those signals are produced by
 * `extractFrictionFromStates` instead and persisted to `.claude/.metrics/
 * friction.json`, keeping `knowledge.json` limited to real patterns/conventions/
 * decisions. This function currently emits no entries but is kept as the
 * extension point for genuine pattern detection.
 *
 * @param {object[]} stateObjects  Parsed .pipeline-states/*.json objects.
 * @returns {{ type: string, name: string, description: string, source: string, tags: string[], prescription?: string }[]}
 */
function extractPatternsFromStates(stateObjects) {
  // Intentionally empty: friction signals moved to extractFrictionFromStates.
  // Real knowledge-pattern heuristics can be added here later.
  return [];
}

/**
 * Extract friction telemetry from an array of pipeline state objects.
 *
 * Friction is measured atrito (hook-level retries, heavy API usage) — it is
 * telemetry, not knowledge. Entries carry `type: 'friction'` and are written to
 * `.claude/.metrics/friction.json` by the session-knowledge hooks, never to
 * `knowledge.json`.
 *
 * The honest count is `retryCount` (the actual measured retries) — there is no
 * `occurrences` field, since "how many times the extractor re-read the same
 * state" is a meaningless number.
 *
 * @param {object[]} stateObjects  Parsed .pipeline-states/*.json objects.
 *                                 Each may have: specName, metrics.retries,
 *                                 metrics.apiCalls, metrics.toolBreakdown
 * @returns {{ type: string, name: string, description: string, source: string, tags: string[], retryCount?: number, apiCalls?: number, prescription?: string }[]}
 */
function extractFrictionFromStates(stateObjects) {
  var friction = [];

  for (var i = 0; i < stateObjects.length; i++) {
    var state = stateObjects[i];
    if (!state || typeof state !== 'object') continue;

    var metrics = state.metrics || {};
    var label = state.specName || state._file || 'unknown';
    var prescription = derivePrescription(metrics);

    // High hook-retry count → friction signal. Counts hook/sandbox events, not
    // agent redispatches — a clean Pass@1 pipeline can still accumulate dozens.
    if (metrics.retries && metrics.retries > 2) {
      var retryEntry = {
        type: 'friction',
        name: 'high-hook-retry-' + label,
        description: 'Pipeline triggered ' + metrics.retries + ' hook-level retries ' +
          '(sandbox/stash-pop/re-prompts — not agent redispatches). Tool breakdown: ' +
          JSON.stringify(metrics.toolBreakdown || {}),
        source: 'session-knowledge',
        tags: ['hook-retry', 'pipeline', 'friction'],
        retryCount: Number(metrics.retries) || 0,
      };
      if (prescription) {
        retryEntry.prescription = prescription;
        retryEntry.tags = retryEntry.tags.concat(['prescriptive']);
      }
      friction.push(retryEntry);
    }

    // Heavy tool usage → friction signal.
    var totalCalls = metrics.apiCalls || 0;
    if (totalCalls > 50) {
      var heavyEntry = {
        type: 'friction',
        name: 'heavy-pipeline-' + label,
        description: 'Pipeline used ' + totalCalls + ' API calls. Consider splitting into smaller scope.',
        source: 'session-knowledge',
        tags: ['optimization', 'pipeline', 'friction'],
        apiCalls: Number(totalCalls) || 0,
      };
      if (prescription) {
        heavyEntry.prescription = prescription;
        heavyEntry.tags = heavyEntry.tags.concat(['prescriptive']);
      }
      friction.push(heavyEntry);
    }
  }

  return friction;
}

/**
 * Persist friction telemetry to `.claude/.metrics/friction.json`.
 *
 * Friction (high hook-retry, heavy API usage) is measured atrito — telemetry,
 * not knowledge. Entries are keyed by `name`; re-running updates the existing
 * entry in place rather than appending duplicates. There is no `occurrences`
 * field — the honest count is `retryCount` / `apiCalls`. Fail-open: any error
 * is swallowed.
 *
 * @param {object[]} frictionEntries  output of extractFrictionFromStates
 * @param {string}   claudeDir        absolute path to the project .claude dir
 */
function saveFriction(frictionEntries, claudeDir) {
  var fs = require('fs');
  var path = require('path');
  try {
    if (!Array.isArray(frictionEntries) || frictionEntries.length === 0) return;

    var metricsDir = path.join(claudeDir, '.metrics');
    if (!fs.existsSync(metricsDir)) { fs.mkdirSync(metricsDir, { recursive: true }); }

    var frictionPath = path.join(metricsDir, 'friction.json');
    var store = { version: 1, entries: [] };
    try {
      if (fs.existsSync(frictionPath)) {
        store = JSON.parse(fs.readFileSync(frictionPath, 'utf8'));
        if (!Array.isArray(store.entries)) store.entries = [];
      }
    } catch (_) { store = { version: 1, entries: [] }; }

    var ts = new Date().toISOString();
    for (var i = 0; i < frictionEntries.length; i++) {
      var entry = frictionEntries[i];
      if (!entry || !entry.name) continue;
      var idx = -1;
      for (var j = 0; j < store.entries.length; j++) {
        if (store.entries[j] && store.entries[j].name === entry.name) { idx = j; break; }
      }
      var record = Object.assign({}, entry, { updatedAt: ts });
      if (idx >= 0) {
        record.createdAt = store.entries[idx].createdAt || ts;
        store.entries[idx] = record;
      } else {
        record.createdAt = ts;
        store.entries.push(record);
      }
    }

    // Keep newest 100 friction entries — bound the file size.
    store.entries.sort(function (a, b) {
      return new Date(b.updatedAt || 0) - new Date(a.updatedAt || 0);
    });
    store.entries = store.entries.slice(0, 100);

    fs.writeFileSync(frictionPath, JSON.stringify(store, null, 2), 'utf8');
  } catch (_) {} // fail-open
}

module.exports = { extractPatternsFromStates, extractFrictionFromStates, derivePrescription, saveFriction };
