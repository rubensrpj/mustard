#!/usr/bin/env node
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

    // ── Source 2: Agent memories (findings) ───────────────────────
    var memDir = path.join(claudeDir, '.agent-memory');
    var indexPath = path.join(memDir, '_index.json');
    if (fs.existsSync(indexPath)) {
      try {
        var index = JSON.parse(fs.readFileSync(indexPath, 'utf8'));
        var entries = Array.isArray(index) ? index : (index.entries || []);
        // Take last 10 entries from this session
        var recent = entries.slice(-10);
        for (var j = 0; j < recent.length; j++) {
          var mem = recent[j];
          if (mem.summary && mem.summary.length > 30) {
            patterns.push({
              type: 'pattern',
              name: 'agent-finding-' + (mem.agent_type || 'unknown'),
              description: mem.summary.substring(0, 200),
              source: 'session-knowledge/' + (mem.agent_type || 'unknown'),
              tags: ['agent', mem.agent_type || 'unknown'],
            });
          }
        }
      } catch (e) { /* skip malformed index */ }
    }

    // ── Save patterns (max 5 per session) ────────────────────────
    var toSave = patterns.slice(0, 5);
    for (var k = 0; k < toSave.length; k++) {
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
