#!/usr/bin/env node
// metrics-report — aggregate enforcement metrics from .claude/.metrics/*.jsonl
// Usage: node metrics-report.js [--since <ISO>] [--event <type>]
'use strict';
const fs = require('fs');
const path = require('path');

const METRICS_DIR = path.join(process.cwd(), '.claude', '.metrics');

// Parse CLI args
const args = process.argv.slice(2);
let sinceFilter = null;
let eventFilter = null;
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--since' && args[i + 1]) sinceFilter = new Date(args[++i]);
  if (args[i] === '--event' && args[i + 1]) eventFilter = args[++i];
}

// Collect all .jsonl files
if (!fs.existsSync(METRICS_DIR)) {
  console.log('No metrics data yet');
  process.exit(0);
}

const files = fs.readdirSync(METRICS_DIR).filter(f => f.endsWith('.jsonl'));
if (files.length === 0) {
  console.log('No metrics data yet');
  process.exit(0);
}

// Aggregate: { event -> { count, tokensAffected, tokensSaved, notes: Set } }
const agg = {};

for (const file of files) {
  const filePath = path.join(METRICS_DIR, file);
  let content;
  try { content = fs.readFileSync(filePath, 'utf8'); } catch (_) { continue; }
  for (const raw of content.split('\n')) {
    const line = raw.trim();
    if (!line) continue;
    let entry;
    try { entry = JSON.parse(line); } catch (_) { continue; } // skip malformed
    if (!entry.event) continue;
    if (sinceFilter && entry.ts && new Date(entry.ts) < sinceFilter) continue;
    if (eventFilter && entry.event !== eventFilter) continue;
    const key = entry.event;
    if (!agg[key]) agg[key] = { count: 0, tokensAffected: 0, tokensSaved: 0, notes: new Set() };
    agg[key].count++;
    if (typeof entry.tokens_affected === 'number') agg[key].tokensAffected += entry.tokens_affected;
    if (typeof entry.tokens_saved === 'number') agg[key].tokensSaved += entry.tokens_saved;
    if (entry.note) agg[key].notes.add(entry.note);
  }
}

const events = Object.keys(agg);
if (events.length === 0) {
  console.log('No metrics data yet');
  process.exit(0);
}

// Render markdown table
const header = '| Event | Count | Tokens Affected | Tokens Saved | Notes |';
const sep    = '|-------|-------|-----------------|--------------|-------|';
console.log(header);
console.log(sep);
for (const evt of events.sort()) {
  const { count, tokensAffected, tokensSaved, notes } = agg[evt];
  const noteStr = [...notes].slice(0, 2).join('; ') || '-';
  console.log(`| ${evt} | ${count} | ${tokensAffected || '-'} | ${tokensSaved || '-'} | ${noteStr} |`);
}
