#!/usr/bin/env bun
// runtime-shim: asserts Bun runtime and exposes bun:sqlite loader.
// Mustard 2.0+ is Bun-only; no Node fallback path.
'use strict';

/**
 * RUNTIME-SHIM: Bun-only runtime helper for Mustard hooks/scripts.
 *
 * Mustard requires Bun >= 1.2.0. This module asserts that on first call
 * and exposes bun:sqlite via loadSqlite(). The legacy pickRuntime() API
 * is preserved for backward compatibility with hooks that still call it,
 * but it always reports kind='bun'.
 *
 * @version 2.0.0
 */

let _cached = null;

function _detect() {
  const isBun = !!(process.versions && process.versions.bun) || typeof globalThis.Bun !== 'undefined';
  if (!isBun) {
    process.stderr.write(
      '[mustard:runtime] FATAL: Bun runtime required (>= 1.2.0). ' +
      'Install: https://bun.sh — Windows: `scoop install bun` — Unix: `curl -fsSL https://bun.sh/install | bash`\n'
    );
    process.exit(1);
  }
  const version =
    (process.versions && process.versions.bun) ||
    (globalThis.Bun && globalThis.Bun.version) ||
    'unknown';
  return { kind: 'bun', version, bunSqliteAvailable: true };
}

function pickRuntime() {
  if (_cached) return _cached;
  _cached = _detect();
  if (process.env.MUSTARD_RUNTIME_VERBOSE === '1') {
    try {
      process.stderr.write('[runtime-shim] ' + JSON.stringify(_cached) + '\n');
    } catch (_e) { /* ignore */ }
  }
  return _cached;
}

function isBun() { return true; }

function loadSqlite() {
  pickRuntime();
  try {
    const mod = require('bun:sqlite');
    return mod && (mod.Database || mod.default || mod);
  } catch (_e) {
    return null;
  }
}

function _resetCache() { _cached = null; }

module.exports = { pickRuntime, isBun, loadSqlite, _resetCache };
