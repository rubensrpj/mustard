#!/usr/bin/env bun
'use strict';
/**
 * verify-emit — confirm that a specific event was emitted to the harness bus
 * within a time window. Used by the orchestrator after each "emit-and-continue"
 * step to detect when an emit silently failed (file not writable, hook bug,
 * etc.) instead of trusting the emitter's fail-open semantics blindly.
 *
 * Reads `.claude/.harness/events.jsonl` and scans backward for the most recent
 * matching event. Exits 0 on match, 1 on no match within window.
 *
 * Usage:
 *   bun verify-emit.js --event mustard.subtraction.applied --since 10s
 *   bun verify-emit.js --event close-gate.check --within 60s --payload-key result --payload-value pass
 *
 * Flags:
 *   --event <name>           required: event name to match
 *   --since <duration>       look back this far (default: 30s)
 *   --within <duration>      alias for --since (kebab convention)
 *   --payload-key <key>      optional: also require payload[key] to exist
 *   --payload-value <value>  optional: with --payload-key, require equality
 *   --spec <name>            optional: also filter by spec field
 *   --quiet                  suppress stdout on success
 *
 * Duration accepts: `30s`, `1m`, `500ms`. Default unit ms if numeric only.
 *
 * Exit codes:
 *   0  event found
 *   1  event not found within window
 *   2  bad arguments
 */
'use strict';

const fs = require('node:fs');
const path = require('node:path');

function parseArgs(argv) {
  const out = { event: null, since: 30_000, payloadKey: null, payloadValue: null, spec: null, quiet: false };
  for (let i = 0; i < argv.length; i++) {
    const f = argv[i], n = argv[i + 1];
    switch (f) {
      case '--event': out.event = n; i++; break;
      case '--since':
      case '--within': out.since = parseDuration(n); i++; break;
      case '--payload-key': out.payloadKey = n; i++; break;
      case '--payload-value': out.payloadValue = n; i++; break;
      case '--spec': out.spec = n; i++; break;
      case '--quiet': out.quiet = true; break;
      case '-h':
      case '--help': printHelp(); process.exit(0); break;
    }
  }
  return out;
}

function parseDuration(s) {
  if (!s) return 30_000;
  if (/^\d+ms$/.test(s)) return Number.parseInt(s, 10);
  if (/^\d+s$/.test(s)) return Number.parseInt(s, 10) * 1000;
  if (/^\d+m$/.test(s)) return Number.parseInt(s, 10) * 60_000;
  if (/^\d+h$/.test(s)) return Number.parseInt(s, 10) * 3_600_000;
  const n = Number.parseInt(s, 10);
  return Number.isFinite(n) ? n : 30_000;
}

function printHelp() {
  process.stdout.write(`verify-emit — confirm a harness event landed recently.

Usage:
  bun verify-emit.js --event NAME [--since 30s] [--payload-key K [--payload-value V]] [--spec NAME] [--quiet]

Exit: 0 found, 1 not found, 2 bad args.
`);
}

function resolveProjectDir() {
  if (process.env.CLAUDE_PROJECT_DIR) return process.env.CLAUDE_PROJECT_DIR;
  return path.resolve(__dirname, '..', '..');
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.event) {
    process.stderr.write('error: --event required\n');
    printHelp();
    process.exit(2);
  }

  const projectDir = resolveProjectDir();
  const file = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  if (!fs.existsSync(file)) {
    if (!args.quiet) process.stderr.write(`[verify-emit] events.jsonl not found: ${file}\n`);
    process.exit(1);
  }

  const cutoff = Date.now() - args.since;
  let raw;
  try { raw = fs.readFileSync(file, 'utf8'); } catch (e) {
    process.stderr.write(`[verify-emit] read error: ${e.message}\n`);
    process.exit(1);
  }

  const lines = raw.split('\n');
  // Scan backward — most-recent match wins early exit.
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i];
    if (!line.trim()) continue;
    let ev;
    try { ev = JSON.parse(line); } catch (_) { continue; }
    if (ev.event !== args.event) continue;
    if (args.spec && ev.spec !== args.spec) continue;

    const tsMs = ev.ts ? Date.parse(ev.ts) : NaN;
    if (!Number.isFinite(tsMs)) continue;
    if (tsMs < cutoff) {
      // Older than window — since we're scanning backward, anything earlier
      // is also out of window. Bail.
      break;
    }

    if (args.payloadKey) {
      const payloadVal = ev.payload && ev.payload[args.payloadKey];
      if (payloadVal === undefined) continue;
      if (args.payloadValue !== null && String(payloadVal) !== args.payloadValue) continue;
    }

    // match found
    if (!args.quiet) {
      const age = Math.round((Date.now() - tsMs) / 1000);
      process.stdout.write(`[verify-emit] OK: ${args.event} ${age}s ago${args.spec ? ' (spec=' + args.spec + ')' : ''}\n`);
    }
    process.exit(0);
  }

  if (!args.quiet) {
    const winSec = Math.round(args.since / 1000);
    process.stderr.write(`[verify-emit] MISS: ${args.event} not found in last ${winSec}s${args.spec ? ' (spec=' + args.spec + ')' : ''}\n`);
  }
  process.exit(1);
}

main();
