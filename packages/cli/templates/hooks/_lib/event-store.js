#!/usr/bin/env bun
'use strict';
/**
 * EVENT-STORE: CJS wrapper around dist/runtime/event-store.js (ESM).
 *
 * Hooks are CommonJS modules with no build step. The compiled EventStore class
 * lives in `dist/runtime/event-store.js` and is ESM (Mustard's package.json has
 * "type": "module"). This wrapper bridges them:
 *
 *   - Under Node 22+, `require(esm)` works natively.
 *   - Under Bun, ESM modules are loadable via require unconditionally.
 *
 * Resolution order (first that yields a valid dist/runtime/event-store.js wins):
 *   1. `MUSTARD_HOME` env var (explicit override).
 *   2. `<claudeDir>/mustard.json#mustardHome` (persisted by `mustard init|update`).
 *   3. findUp(__dirname, 'bin/mustard.js') — legacy fallback when the consumer
 *      project is nested inside the Mustard repo.
 *
 * Fail-open contract: any error returns `null`. Callers MUST handle null by
 * falling back to direct events.jsonl reads. This wrapper never throws.
 *
 * Singleton: one EventStore per process. Hooks spawn as child processes, so
 * sharing is per-tool-call only — adequate for the few hooks that query twice.
 *
 * @version 1.1.0
 */

const fs = require('node:fs');
const path = require('node:path');

let _EventStoreClass = null;
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

/** True if `root` looks like a Mustard install (has dist/runtime/event-store.js). */
function hasEventStoreDist(root) {
  if (!root || typeof root !== 'string') return false;
  try { return fs.existsSync(path.join(root, 'dist', 'runtime', 'event-store.js')); } catch (_) { return false; }
}

/**
 * Read `mustardHome` from `<__dirname>/../../mustard.json` (the consumer's
 * `.claude/mustard.json`). Returns null when missing/malformed.
 */
function readPersistedMustardHome() {
  try {
    const cfgPath = path.join(__dirname, '..', '..', 'mustard.json');
    if (!fs.existsSync(cfgPath)) return null;
    const cfg = JSON.parse(fs.readFileSync(cfgPath, 'utf-8'));
    return typeof cfg.mustardHome === 'string' ? cfg.mustardHome : null;
  } catch (_) { return null; }
}

/** Resolve Mustard install root by env → mustard.json → findUp. */
function resolveMustardRoot() {
  const fromEnv = process.env.MUSTARD_HOME;
  if (hasEventStoreDist(fromEnv)) return fromEnv;

  const fromCfg = readPersistedMustardHome();
  if (hasEventStoreDist(fromCfg)) return fromCfg;

  const mustardBin = findUp(__dirname, path.join('bin', 'mustard.js'));
  if (mustardBin) {
    const root = path.dirname(path.dirname(mustardBin));
    if (hasEventStoreDist(root)) return root;
  }
  return null;
}

/**
 * Locate and load the EventStore class from dist/runtime/event-store.js.
 * Returns null on any failure. Memoised after first call (success or failure).
 */
function getEventStoreClass() {
  if (_resolveAttempted) return _EventStoreClass;
  _resolveAttempted = true;
  try {
    const mustardRoot = resolveMustardRoot();
    if (!mustardRoot) return null;
    const distPath = path.join(mustardRoot, 'dist', 'runtime', 'event-store.js');

    // require(esm) is supported in Node 22+ and Bun.
    const mod = require(distPath);
    _EventStoreClass = mod && (mod.EventStore || mod.default || null);
    return _EventStoreClass;
  } catch (err) {
    try { process.stderr.write('[event-store] load failed: ' + err.message + '\n'); } catch (_) {}
    return null;
  }
}

/**
 * Get a singleton EventStore bound to `<claudeDir>/.harness/mustard.db`.
 * Calls `init()` once. Returns null if the class cannot be loaded or `init()`
 * throws (typical when running under plain Node without Bun + no driver).
 *
 * @param {string} claudeDir  Absolute path to the project's .claude directory.
 * @returns {object|null}     EventStore instance or null on any failure.
 */
function getStore(claudeDir) {
  if (_instance) return _instance;
  if (!claudeDir || typeof claudeDir !== 'string') return null;
  const ES = getEventStoreClass();
  if (!ES) return null;
  try {
    const harnessDir = path.join(claudeDir, '.harness');
    try { fs.mkdirSync(harnessDir, { recursive: true }); } catch (_) {}
    const dbPath = path.join(harnessDir, 'mustard.db');
    const store = new ES(dbPath);
    store.init();
    _instance = store;
    return _instance;
  } catch (err) {
    try { process.stderr.write('[event-store] init failed: ' + err.message + '\n'); } catch (_) {}
    return null;
  }
}

/** Close + drop the singleton. Safe to call when none exists. */
function closeStore() {
  if (!_instance) return;
  try { if (typeof _instance.close === 'function') _instance.close(); } catch (_) {}
  _instance = null;
}

module.exports = { getStore, closeStore };
