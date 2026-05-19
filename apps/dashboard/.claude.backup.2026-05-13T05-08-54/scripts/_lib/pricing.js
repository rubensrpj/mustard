#!/usr/bin/env node
'use strict';
/**
 * PRICING (scripts side): CJS wrapper around dist/telemetry/pricing.js (ESM).
 *
 * Mustard 2.0 Phase 2 — dashboard needs costUsd() to compute USD totals
 * from real telemetry spans. The compiled pricing module lives in
 * `dist/telemetry/pricing.js` and is ESM. This wrapper mirrors the strategy
 * used by `_lib/event-store.js`: walk up from this file to find the Mustard
 * install (`bin/mustard.js`), then require the compiled ESM via Node 22+'s
 * native `require(esm)` (or Bun).
 *
 * Fail-open contract: any resolution/load error returns a stub that yields
 * 0 for every model. This keeps the dashboard usable when running inside a
 * user project where Mustard isn't an ancestor (e.g. Sialia today), at the
 * price of "—"/$0 cost columns until the user installs Mustard locally.
 *
 * @version 1.0.0
 */

const fs = require('node:fs');
const path = require('node:path');

let _resolved = null;
let _attempted = false;

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

function load() {
  if (_attempted) return _resolved;
  _attempted = true;
  try {
    const mustardBin = findUp(__dirname, path.join('bin', 'mustard.js'));
    if (!mustardBin) return null;
    const mustardRoot = path.dirname(path.dirname(mustardBin));
    const distPath = path.join(mustardRoot, 'dist', 'telemetry', 'pricing.js');
    if (!fs.existsSync(distPath)) return null;
    const mod = require(distPath);
    if (mod && typeof mod.costUsd === 'function') {
      _resolved = mod;
    }
    return _resolved;
  } catch (err) {
    try { process.stderr.write('[pricing] load failed: ' + err.message + '\n'); } catch (_) {}
    return null;
  }
}

/**
 * Compute USD cost for a token usage tuple. Returns 0 when the underlying
 * pricing table isn't reachable (Mustard not installed as ancestor) or when
 * the model is unknown. Callers should treat 0 as "unpriced", not "free".
 */
function costUsd(model, inputTokens, outputTokens) {
  const m = load();
  if (!m || typeof m.costUsd !== 'function') return 0;
  try { return m.costUsd(model, inputTokens || 0, outputTokens || 0); }
  catch (_) { return 0; }
}

module.exports = { costUsd };
