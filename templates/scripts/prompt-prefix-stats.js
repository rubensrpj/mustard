#!/usr/bin/env bun
/**
 * PROMPT-PREFIX-STATS: Aggregate prompt-prefix metrics for dashboard / CLI.
 *
 * Reads `.claude/.metrics/prompt-prefix-hit.jsonl` and
 * `.claude/.metrics/prompt-prefix-miss.jsonl` — `metrics-emit.js` writes one
 * file per event name. Each line is a JSON object carrying `event`,
 * `tokens_saved`, and optionally `prefix_hash` (sha256 of the cached prefix).
 *
 * Output: JSON to stdout. Always exit 0 (callers tolerate missing file).
 * Cap: ~2000 chars.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

const MAX_CHARS = 2000;
const HIT = 'prompt-prefix-hit';
const MISS = 'prompt-prefix-miss';

function emptyResult() {
  return {
    total_dispatches: 0,
    hits: 0,
    misses: 0,
    hit_rate: 0,
    tokens_saved_total: 0,
    top_prefix_hashes: [],
  };
}

function parseLines(raw) {
  const out = [];
  const lines = raw.split('\n');
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try { out.push(JSON.parse(trimmed)); } catch { /* skip malformed */ }
  }
  return out;
}

function aggregate(entries) {
  const result = emptyResult();
  const hashCounts = new Map();
  for (const e of entries) {
    if (!e || typeof e !== 'object') continue;
    const evt = e.event;
    if (evt !== HIT && evt !== MISS) continue;
    result.total_dispatches++;
    if (evt === HIT) {
      result.hits++;
      if (Number.isFinite(e.tokens_saved)) result.tokens_saved_total += e.tokens_saved;
      const h = typeof e.prefix_hash === 'string' && e.prefix_hash ? e.prefix_hash : null;
      if (h) hashCounts.set(h, (hashCounts.get(h) || 0) + 1);
    } else {
      result.misses++;
    }
  }
  if (result.total_dispatches > 0) {
    result.hit_rate = Math.round((result.hits / result.total_dispatches) * 1000) / 1000;
  }
  result.top_prefix_hashes = [...hashCounts.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([hash, count]) => ({ hash, count }));
  return result;
}

function main() {
  const cwd = process.cwd();
  const metricsDir = path.join(cwd, '.claude', '.metrics');
  const sources = ['prompt-prefix-hit.jsonl', 'prompt-prefix-miss.jsonl'];
  const entries = [];
  for (const name of sources) {
    const file = path.join(metricsDir, name);
    try {
      if (fs.existsSync(file)) {
        const raw = fs.readFileSync(file, 'utf8');
        if (raw) entries.push(...parseLines(raw));
      }
    } catch { /* ignore per-file errors */ }
  }
  const result = entries.length ? aggregate(entries) : emptyResult();
  let out;
  try { out = JSON.stringify(result); } catch { out = JSON.stringify(emptyResult()); }
  if (out.length > MAX_CHARS) {
    // shouldn't happen with top-3 cap, but defend anyway.
    const fallback = { ...result, top_prefix_hashes: [] };
    out = JSON.stringify(fallback);
  }
  process.stdout.write(out + '\n');
  process.exit(0);
}

if (require.main === module) {
  try { main(); } catch {
    process.stdout.write(JSON.stringify(emptyResult()) + '\n');
    process.exit(0);
  }
}

module.exports = { aggregate, parseLines };
