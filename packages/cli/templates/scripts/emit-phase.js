#!/usr/bin/env bun
'use strict';
/**
 * emit-phase — record a `pipeline.phase` transition event from a SKILL.
 *
 * Problem this solves: `pipeline-phase.js` (PostToolUse hook) only emits
 * `pipeline.phase` when a `.claude/.pipeline-states/{spec}.json` file is
 * written and the phase changes. But ANALYZE runs in the parent context
 * BEFORE any pipeline-state file exists — so ANALYZE never produced
 * telemetry, and the dashboard showed `0` ANALYZE events forever.
 *
 * The SKILL is the only place that knows ANALYZE has started. This script
 * lets a SKILL emit the marker explicitly:
 *   bun .claude/scripts/emit-phase.js --spec add-login --to ANALYZE
 *
 * The emitted event is shape-identical to what `pipeline-phase.js` produces:
 *   event:   pipeline.phase
 *   payload: { from, to }
 *   spec:    {spec}
 * — so every downstream consumer (dashboard, metrics) treats both sources
 * uniformly.
 *
 * Idempotency: the script scans `.claude/.harness/events.jsonl` for the most
 * recent `pipeline.phase` event of the same spec. If its `to` already equals
 * the requested `--to` phase, the script silently skips the emit. This keeps
 * dedup logic in ONE place — SKILLs just call the script unconditionally.
 *
 * Cross-shell: no inline `bun -e` quoting. Fail-open: any internal error
 * exits 0 without emitting (telemetry must never break a pipeline).
 *
 * Exit codes:
 *   0  emitted, or idempotent skip, or fail-silent on internal error
 *   1  bad CLI arguments
 */

const fs = require('node:fs');
const path = require('node:path');

function parseArgs(argv) {
  const out = { spec: null, from: null, to: null };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const next = argv[i + 1];
    switch (flag) {
      case '--spec':
        out.spec = next; i++; break;
      case '--from':
        out.from = next; i++; break;
      case '--to':
        out.to = next; i++; break;
      case '-h':
      case '--help':
        printHelp();
        process.exit(0);
        break;
      default:
        // ignore unknown flags rather than failing — fail-silent ethos
        break;
    }
  }
  return out;
}

function printHelp() {
  process.stdout.write(`emit-phase — record a pipeline.phase transition event.

Usage:
  bun emit-phase.js --spec <name> --to <PHASE> [--from <PHASE>]

  --spec NAME   spec identifier (required)
  --to PHASE    phase being entered, e.g. ANALYZE (required)
  --from PHASE  prior phase (optional; defaults to null)

Idempotent: skips the emit if the spec's latest pipeline.phase already
matches --to. Exit: 0 on emit/skip/silent-error, 1 on bad args.
`);
}

function resolveProjectDir() {
  if (process.env.CLAUDE_PROJECT_DIR) return process.env.CLAUDE_PROJECT_DIR;
  // Heuristic: script sits at .claude/scripts/, two levels up is project root.
  return path.resolve(__dirname, '..', '..');
}

function loadHarness(projectDir) {
  const harnessLib = path.join(projectDir, '.claude', 'hooks', '_lib', 'harness-event.js');
  if (!fs.existsSync(harnessLib)) return null;
  try {
    return require(harnessLib);
  } catch (_) {
    return null;
  }
}

/**
 * Returns the `to` phase of the most recent `pipeline.phase` event for the
 * given spec, or null if none found. Reads the events.jsonl tail-to-head so
 * the freshest record wins. Fail-soft: returns null on any error.
 */
function lastPhaseForSpec(eventsFile, spec) {
  try {
    if (!fs.existsSync(eventsFile)) return null;
    const lines = fs.readFileSync(eventsFile, 'utf8').split('\n');
    for (let i = lines.length - 1; i >= 0; i--) {
      const raw = lines[i].trim();
      if (!raw) continue;
      let obj;
      try { obj = JSON.parse(raw); } catch (_) { continue; }
      if (obj && obj.event === 'pipeline.phase' && obj.spec === spec) {
        return (obj.payload && obj.payload.to) || null;
      }
    }
  } catch (_) {}
  return null;
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  if (!args.spec) {
    process.stderr.write('error: --spec required\n');
    printHelp();
    process.exit(1);
  }
  if (!args.to) {
    process.stderr.write('error: --to required\n');
    printHelp();
    process.exit(1);
  }

  const projectDir = resolveProjectDir();
  const harness = loadHarness(projectDir);
  if (!harness) {
    // Fail-silent: harness not installed yet. This is OK during bootstrap.
    process.exit(0);
  }

  // Idempotency: don't emit the same phase twice for one spec. If the spec's
  // latest pipeline.phase already lands on --to, this is a no-op.
  const eventsFile = harness.getEventsFile(projectDir);
  const last = lastPhaseForSpec(eventsFile, args.spec);
  if (last === args.to) {
    process.exit(0);
  }

  // `from` defaults to the spec's last known phase (null if none) — so a
  // SKILL only needs to pass --to. An explicit --from overrides this.
  const fromPhase = args.from || last || null;

  harness.emit('pipeline.phase', { from: fromPhase, to: args.to }, {
    cwd: projectDir,
    spec: args.spec,
    actor: { kind: 'orchestrator', id: 'emit-phase' },
  });
  process.exit(0);
}

main();
