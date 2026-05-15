'use strict';
/**
 * metrics-emit — shared helper for appending enforcement metrics to JSONL.
 *
 * Schema (one line per call):
 *   { ts, event, tokens_affected, tokens_saved, note, ...extras }
 *
 * Files live under `.claude/.metrics/{event}.jsonl`. `metrics-report.js`
 * iterates every `*.jsonl` in that dir, so per-event sharding is compatible.
 *
 * Fail-silent: ANY error (mkdir, append, JSON stringify) is swallowed so
 * hooks calling this never observe a throw. Hooks remain fail-open.
 */

const fs = require('fs');
const path = require('path');

/**
 * Append a metric line.
 *
 * @param {string} event  e.g. "budget-check", "spec-hygiene-move", "rtk-rewrite"
 * @param {object} opts
 * @param {number} [opts.tokensAffected=0]  Conservative tokens touched by this event.
 * @param {number} [opts.tokensSaved=0]     Tokens prevented from entering context.
 * @param {string} [opts.note='']           Short human label (e.g. "blocked", "passed").
 * @param {object} [opts.extras={}]         Extra fields merged into the JSONL line.
 * @param {string} [opts.cwd]               Override project dir (defaults to process.cwd()).
 */
function emitMetric(event, opts = {}) {
  try {
    if (!event || typeof event !== 'string') return;
    const cwd = opts.cwd || process.cwd();
    const dir = path.join(cwd, '.claude', '.metrics');
    const file = path.join(dir, `${event}.jsonl`);
    const line = {
      ts: new Date().toISOString(),
      event,
      tokens_affected: Number.isFinite(opts.tokensAffected) ? opts.tokensAffected : 0,
      tokens_saved: Number.isFinite(opts.tokensSaved) ? opts.tokensSaved : 0,
      note: typeof opts.note === 'string' ? opts.note : '',
      ...(opts.extras && typeof opts.extras === 'object' ? opts.extras : {}),
    };
    // Defense in depth: stringify inside a nested try so a malformed extras
    // object (e.g. circular ref) can't escape the outer try either.
    let serialized;
    try {
      serialized = JSON.stringify(line);
    } catch (_) {
      return; // bail silently — better to drop a metric than crash a hook
    }
    if (typeof serialized !== 'string' || !serialized) return;
    fs.mkdirSync(dir, { recursive: true });
    fs.appendFileSync(file, serialized + '\n');
  } catch (_) {
    // fail-silent — never throw out of a hook
  }
}

module.exports = { emitMetric };
