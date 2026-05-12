#!/usr/bin/env node
// runtime-shim: detect Bun vs Node at hook startup, expose pickRuntime() helper.
// Stays CommonJS for Node compat; Bun executes CJS natively.
'use strict';

/**
 * RUNTIME-SHIM: Cross-runtime detection helper for Mustard hooks.
 *
 * Env vars:
 *   MUSTARD_RUNTIME=node|bun   — force runtime identity (default: auto-detect)
 *   MUSTARD_RUNTIME_VERBOSE=1  — log detection result to stderr
 *
 * Phase 0 contract: detection only. SQLite fallback for Node is Phase 1.
 * @version 1.0.0
 */

let _cached = null;

function _detect() {
  const hasBunGlobal = typeof globalThis.Bun !== 'undefined';
  const hasBunVersion = !!(process.versions && process.versions.bun);
  const isBun = hasBunGlobal || hasBunVersion;

  let kind = isBun ? 'bun' : 'node';
  let version = isBun
    ? (process.versions && process.versions.bun) || (globalThis.Bun && globalThis.Bun.version) || 'unknown'
    : (process.versions && process.versions.node) || 'unknown';

  const override = (process.env.MUSTARD_RUNTIME || '').toLowerCase();
  if (override === 'node' || override === 'bun') {
    kind = override;
    if (override === 'node' && process.versions && process.versions.node) {
      version = process.versions.node;
    } else if (override === 'bun' && process.versions && process.versions.bun) {
      version = process.versions.bun;
    }
  }

  // bun:sqlite is only available when actually running under Bun (override
  // cannot synthesize the builtin module).
  const bunSqliteAvailable = isBun;

  return { kind, version, bunSqliteAvailable };
}

function pickRuntime() {
  if (_cached) return _cached;
  try {
    _cached = _detect();
  } catch (_e) {
    // Fail-open: assume node when detection blows up.
    _cached = { kind: 'node', version: 'unknown', bunSqliteAvailable: false };
  }
  if (process.env.MUSTARD_RUNTIME_VERBOSE === '1') {
    try {
      process.stderr.write(
        '[runtime-shim] ' + JSON.stringify(_cached) + '\n'
      );
    } catch (_e) { /* ignore stderr failure */ }
  }
  return _cached;
}

function isBun() {
  return pickRuntime().kind === 'bun';
}

function isNode() {
  return pickRuntime().kind === 'node';
}

function loadSqlite() {
  // Only attempt bun:sqlite when actually running under Bun. Phase 0 does not
  // ship a Node fallback (better-sqlite3 etc.) — that is Phase 1's problem.
  const rt = pickRuntime();
  if (!rt.bunSqliteAvailable) return null;
  try {
    // eslint-disable-next-line global-require
    const mod = require('bun:sqlite');
    return mod && (mod.Database || mod.default || mod);
  } catch (_e) {
    return null;
  }
}

// Test-only: reset memoization. Not part of public API.
function _resetCache() {
  _cached = null;
}

module.exports = { pickRuntime, isBun, isNode, loadSqlite, _resetCache };
