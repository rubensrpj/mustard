#!/usr/bin/env node
// _metrics-write — append-only metrics helper for enforcement hooks.
// Usage: require('./_metrics-write').append({event: 'budget-block', role: 'explorer', ...})
// Hooks opt-in by requiring this module — zero instrumentation by default.
'use strict';
const fs = require('fs');
const path = require('path');

const METRICS_DIR = path.join(process.cwd(), '.claude', '.metrics');
const METRICS_FILE = path.join(METRICS_DIR, 'enforcement.jsonl');
const ROTATE_SIZE = 10 * 1024 * 1024; // 10MB

function append(event) {
  try {
    if (!fs.existsSync(METRICS_DIR)) fs.mkdirSync(METRICS_DIR, { recursive: true });
    // rotate if large
    if (fs.existsSync(METRICS_FILE) && fs.statSync(METRICS_FILE).size > ROTATE_SIZE) {
      fs.renameSync(METRICS_FILE, METRICS_FILE + '.1');
    }
    const line = JSON.stringify({ ts: new Date().toISOString(), ...event }) + '\n';
    fs.appendFileSync(METRICS_FILE, line);
  } catch (_) {
    // fail-silent: never block the caller
  }
}

module.exports = { append };
