#!/usr/bin/env bun
'use strict';
/**
 * SESSION-KNOWLEDGE: Extracts patterns from session before cleanup.
 * Event: SessionEnd (must run BEFORE session-cleanup.js)
 * Fail-open: exit 0 on any error.
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const { shouldRun } = require('./_lib/hook-env.js');
const { extractPatternsFromStates, extractFrictionFromStates, saveFriction } = require('./_lib/knowledge-extract.js');

// ── Harness event bus (Wave 2 dual emission) ─────────────────────────────────
var harnessEmit = null;
var harnessGetSessionId = null;
var harnessGetWave = null;
var harnessGetEventsFile = null;
try {
  var he = require('./_lib/harness-event.js');
  harnessEmit = he.emit;
  harnessGetSessionId = he.getCurrentSessionId;
  harnessGetWave = he.getCurrentWave;
  harnessGetEventsFile = he.getEventsFile;
} catch (_) {} // fail-open: harness optional

function emitFinding(pattern, ctx) {
  try {
    if (!harnessEmit) return;
    harnessEmit('finding', {
      kind: pattern.type || 'pattern',
      content: pattern.description || pattern.name || '',
      confidence: typeof pattern.confidence === 'number' ? pattern.confidence : null,
      refs: Array.isArray(pattern.tags) ? pattern.tags : [],
    }, ctx);
  } catch (_) {} // fail-open
}

/**
 * Emit one `retry.attempt` event per hook-level retry recorded in a pipeline
 * state's `metrics.retries`. The dashboard Quality view counts these events to
 * fill the RETRIES column; without them the column is always 0.
 *
 * Idempotency: skip emission when the spec already has `retry.attempt` events
 * in the harness log — `session-knowledge.js` may run multiple times across a
 * spec's lifetime and metrics.retries is cumulative, so re-emitting would
 * double-count. Fail-open: any error is swallowed.
 *
 * @param {object} state  parsed .pipeline-states/*.json object
 * @param {object} data   SessionEnd hook input (for sessionId/wave inference)
 * @param {string} cwd    project root
 */
function emitRetryAttempts(state, data, cwd) {
  try {
    if (!harnessEmit || !state || typeof state !== 'object') return;
    var metrics = state.metrics || {};
    var retries = Number(metrics.retries) || 0;
    if (retries < 1) return;

    var spec = state.specName || state._file || null;
    if (!spec) return;

    // Idempotency: don't re-emit retry.attempt for a spec already counted.
    if (specHasRetryEvents(cwd, spec)) return;

    var ctx = {
      cwd: cwd,
      spec: spec,
      sessionId: harnessGetSessionId ? harnessGetSessionId(data) : null,
      wave: harnessGetWave ? harnessGetWave(data) : 0,
      actor: { kind: 'hook', id: 'session-knowledge' },
    };
    for (var n = 0; n < retries; n++) {
      harnessEmit('retry.attempt', {
        reason: 'hook-level',
        tool: null,
      }, ctx);
    }
  } catch (_) {} // fail-open
}

/**
 * Returns true when the harness log already has a `retry.attempt` event for
 * the given spec. Reads events.jsonl tail-to-head. Fail-soft: false on error.
 */
function specHasRetryEvents(cwd, spec) {
  try {
    if (!harnessGetEventsFile) return false;
    var eventsFile = harnessGetEventsFile(cwd);
    if (!fs.existsSync(eventsFile)) return false;
    var lines = fs.readFileSync(eventsFile, 'utf8').split('\n');
    for (var i = lines.length - 1; i >= 0; i--) {
      var raw = lines[i].trim();
      if (!raw) continue;
      var obj;
      try { obj = JSON.parse(raw); } catch (_) { continue; }
      if (obj && obj.event === 'retry.attempt' && obj.spec === spec) return true;
    }
  } catch (_) {}
  return false;
}

var input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', function (chunk) { input += chunk; });
process.stdin.on('end', function () {
  try {
    if (!shouldRun('session-knowledge')) { process.exit(0); }

    var data = JSON.parse(input);
    var cwd = data.cwd || process.cwd();
    var claudeDir = path.join(cwd, '.claude');
    var knowledgeScript = path.join(claudeDir, 'scripts', 'memory.js');

    // Bail if memory.js doesn't exist
    if (!fs.existsSync(knowledgeScript)) { process.exit(0); }

    // Skip if session-knowledge-inc ran recently (<5 min) — avoid redundant write
    try {
      var seenStat = fs.statSync(path.join(claudeDir, '.knowledge-seen.json'));
      if (Date.now() - seenStat.mtimeMs < 5 * 60 * 1000) { process.exit(0); }
    } catch (_) { /* file missing or unreadable — proceed */ }

    var patterns = [];

    // ── Source 1: Pipeline states (retries, tool usage) ───────────
    var statesDir = path.join(claudeDir, '.pipeline-states');
    if (fs.existsSync(statesDir)) {
      var stateFiles = fs.readdirSync(statesDir).filter(function (f) { return f.endsWith('.json'); });
      var stateObjects = [];
      for (var i = 0; i < stateFiles.length; i++) {
        try {
          var state = JSON.parse(fs.readFileSync(path.join(statesDir, stateFiles[i]), 'utf8'));
          // Attach filename as fallback label for the extractor
          state._file = stateFiles[i].replace('.json', '');
          stateObjects.push(state);
        } catch (e) { /* skip malformed state */ }
      }
      var statePatterns = extractPatternsFromStates(stateObjects);
      for (var si = 0; si < statePatterns.length; si++) { patterns.push(statePatterns[si]); }

      // ── Friction telemetry → .claude/.metrics/friction.json ────────────
      // high-hook-retry / heavy-pipeline signals are atrito, not knowledge.
      // They are written to friction.json (type: 'friction'), keeping
      // knowledge.json limited to real patterns/conventions/decisions.
      saveFriction(extractFrictionFromStates(stateObjects), claudeDir);

      // ── Emit retry.attempt events from measured hook-level retries ──────
      // The retry count lives in metrics.retries but was never a consumable
      // event — the dashboard's RETRIES column counts `retry.attempt` events,
      // so it stayed stuck at 0. Emit one event per retry occurrence here so
      // the signal becomes real telemetry. Knowledge entry labelling stays
      // untouched (owned by knowledge-extract.js / Wave 4).
      for (var ri = 0; ri < stateObjects.length; ri++) {
        emitRetryAttempts(stateObjects[ri], data, cwd);
      }
    }

    // ── Save patterns (max 5 per session) ────────────────────────
    var sessionId = harnessGetSessionId ? harnessGetSessionId(data) : null;
    var wave = harnessGetWave ? harnessGetWave(data) : 0;
    var emitCtx = {
      cwd: cwd,
      sessionId: sessionId,
      wave: wave,
      actor: { kind: 'hook', id: 'session-knowledge' },
    };

    var toSave = patterns.slice(0, 5);
    for (var k = 0; k < toSave.length; k++) {
      // ── Wave 2: emit finding event before persisting ──────────
      emitFinding(toSave[k], emitCtx);

      try {
        execFileSync(process.execPath, [knowledgeScript, 'knowledge'], {
          input: JSON.stringify(Object.assign({ cwd: cwd }, toSave[k])),
          timeout: 3000,
          stdio: ['pipe', 'pipe', 'pipe'],
        });
      } catch (e) { /* fail-open: skip this pattern */ }
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[session-knowledge] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
process.stdin.resume();
