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
const { extractPatternsFromStates } = require('./_lib/knowledge-extract.js');

// ── Harness event bus (Wave 2 dual emission) ─────────────────────────────────
var harnessEmit = null;
var harnessGetSessionId = null;
var harnessGetWave = null;
try {
  var he = require('./_lib/harness-event.js');
  harnessEmit = he.emit;
  harnessGetSessionId = he.getCurrentSessionId;
  harnessGetWave = he.getCurrentWave;
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

var input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', function (chunk) { input += chunk; });
process.stdin.on('end', function () {
  try {
    if (!shouldRun('session-knowledge')) { process.exit(0); }

    var data = JSON.parse(input);
    var cwd = data.cwd || process.cwd();
    var claudeDir = path.join(cwd, '.claude');
    var knowledgeScript = path.join(claudeDir, 'scripts', 'knowledge-update.js');

    // Bail if knowledge-update.js doesn't exist
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
        execFileSync(process.execPath, [knowledgeScript], {
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
