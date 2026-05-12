#!/usr/bin/env node
'use strict';
/**
 * SPAN-EMITTER: CJS wrapper around dist/telemetry/token-tracker.js (ESM).
 *
 * Mirrors event-store.js — hooks are CommonJS modules with no build step.
 * The compiled TokenTracker class lives in `dist/telemetry/token-tracker.js`
 * and is ESM (Mustard's package.json has "type": "module"). This wrapper
 * bridges them:
 *
 *   - Under Node 22+, `require(esm)` works natively.
 *   - Under Bun, ESM modules are loadable via require unconditionally.
 *
 * Resolution: walks up the filesystem from this file looking for a sibling
 * `bin/mustard.js` (the Mustard install). When a hook runs inside a USER
 * project (e.g. sialia), the Mustard repo is elsewhere — this resolver will
 * fail and the wrapper returns `null`, signalling callers to skip span
 * emission silently.
 *
 * Fail-open contract: any error returns `null`. Callers MUST handle null by
 * skipping the emit. This wrapper never throws.
 *
 * Singleton: one TokenTracker per process. Hooks spawn as child processes,
 * so sharing is per-tool-call only — adequate for the one or two calls a
 * hook makes (startSpan in PreToolUse OR endSpan in PostToolUse — never both
 * in the same process).
 *
 * @version 1.0.0
 */

const fs = require('node:fs');
const path = require('node:path');

let _TokenTrackerClass = null;
let _resolveAttempted = false;
let _instance = null;

/** Walk ancestor directories looking for a marker file. */
function findUp(startDir, marker) {
  let cur = startDir;
  while (cur) {
    const candidate = path.join(cur, marker);
    try { if (fs.existsSync(candidate)) return candidate; } catch (_) {}
    const parent = path.dirname(cur);
    if (parent === cur) return null;
    cur = parent;
  }
  return null;
}

/**
 * Locate and load the TokenTracker class from dist/telemetry/token-tracker.js.
 * Returns null on any failure. Memoised after first call (success or failure).
 */
function getTokenTrackerClass() {
  if (_resolveAttempted) return _TokenTrackerClass;
  _resolveAttempted = true;
  try {
    const mustardBin = findUp(__dirname, path.join('bin', 'mustard.js'));
    if (!mustardBin) return null;
    const mustardRoot = path.dirname(path.dirname(mustardBin));
    const distPath = path.join(mustardRoot, 'dist', 'telemetry', 'token-tracker.js');
    if (!fs.existsSync(distPath)) return null;

    // require(esm) is supported in Node 22+ and Bun.
    const mod = require(distPath);
    _TokenTrackerClass = mod && (mod.TokenTracker || mod.default || null);
    return _TokenTrackerClass;
  } catch (err) {
    try { process.stderr.write('[span-emitter] load failed: ' + err.message + '\n'); } catch (_) {}
    return null;
  }
}

/**
 * Get a singleton TokenTracker bound to `<claudeDir>/.harness/spans.jsonl`.
 * Returns null if the class cannot be loaded or instantiation throws.
 *
 * @param {string} claudeDir  Absolute path to the project's .claude directory.
 * @returns {object|null}     TokenTracker instance or null on any failure.
 */
function getTracker(claudeDir) {
  if (_instance) return _instance;
  if (!claudeDir || typeof claudeDir !== 'string') return null;
  const TT = getTokenTrackerClass();
  if (!TT) return null;
  try {
    const harnessDir = path.join(claudeDir, '.harness');
    try { fs.mkdirSync(harnessDir, { recursive: true }); } catch (_) {}
    const spansPath = path.join(harnessDir, 'spans.jsonl');
    _instance = new TT(spansPath);
    return _instance;
  } catch (err) {
    try { process.stderr.write('[span-emitter] init failed: ' + err.message + '\n'); } catch (_) {}
    return null;
  }
}

module.exports = { getTracker };
