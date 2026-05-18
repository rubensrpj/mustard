#!/usr/bin/env bun
/**
 * AC #4: compare EventStore.metrics(spec) (rebuilt from events) against
 * buildPipelineState(events, {spec}).metrics (live computation over the same
 * NDJSON log). Runs over recoverable specs from the sialia harness snapshot.
 *
 * Run under Bun (EventStore requires bun:sqlite):
 *   bun tests/integration/event-store-vs-buildpipelinestate.js
 *
 * Tolerance policy (per task spec): start at 0% (strict). On mismatch, print
 * exactly what diverged so the parent agent can decide if the delta is a real
 * bug in rebuild()/buildPipelineState() or a known semantic difference.
 *
 * NOTE on semantic differences (documented, not fixed here):
 *   - apiCalls:   EventStore counts `event === 'api.call'`;
 *                 buildPipelineState counts every `tool.use` event except Read.
 *   - retries:    EventStore counts `event === 'agent.retry'`;
 *                 buildPipelineState counts `dispatch.failure` events.
 *   - agentCount: EventStore counts unique actor.id of agent.start;
 *                 buildPipelineState counts every agent.start.
 * These divergences are expected — the test surfaces them so the comparison
 * can be tightened or the projections aligned in a follow-up.
 */
'use strict';

const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
const { execSync } = require('node:child_process');

const SIALIA_HARNESS = 'C:/Atiz/Competi/projetos/sialia/.claude/.harness';
const REPO_ROOT = path.resolve(__dirname, '..', '..');
const TMP = path.join(os.tmpdir(), 'mustard-event-store-vs-bps');

// ─ Setup ─────────────────────────────────────────────────────────────────────
function setup() {
  if (!fs.existsSync(SIALIA_HARNESS)) {
    console.error('SKIP: sialia harness not found at ' + SIALIA_HARNESS);
    process.exit(0);
  }
  fs.rmSync(TMP, { recursive: true, force: true });
  fs.mkdirSync(TMP, { recursive: true });
  fs.cpSync(SIALIA_HARNESS, TMP, { recursive: true });

  const migrate = path.join(REPO_ROOT, 'dist', 'migrate', 'jsonl-to-sqlite.js');
  execSync('bun ' + JSON.stringify(migrate) + ' ' + JSON.stringify(TMP), {
    stdio: 'pipe',
    cwd: REPO_ROOT,
  });
}

// ─ Loaders ───────────────────────────────────────────────────────────────────
async function loadEventStore() {
  const mod = await import(
    'file://' + path.join(REPO_ROOT, 'dist', 'runtime', 'event-store.js').replace(/\\/g, '/')
  );
  const store = new mod.EventStore(path.join(TMP, 'mustard.db'));
  store.init();
  return store;
}

function loadHarnessViews() {
  return require(path.join(REPO_ROOT, 'templates', 'scripts', 'event-projections.js'));
}

function readEventsJsonl() {
  const raw = fs.readFileSync(path.join(TMP, 'events.jsonl'), 'utf8');
  return raw
    .split('\n')
    .filter((l) => l.trim())
    .map((l) => {
      try {
        return JSON.parse(l);
      } catch (_) {
        return null;
      }
    })
    .filter(Boolean);
}

// ─ Spec discovery ────────────────────────────────────────────────────────────
function findRecoverableSpecs(events, minEvents = 5, max = 3) {
  const counts = Object.create(null);
  for (const e of events) {
    if (e && typeof e.spec === 'string' && e.spec) {
      counts[e.spec] = (counts[e.spec] || 0) + 1;
    }
  }
  return Object.entries(counts)
    .filter(([, n]) => n >= minEvents)
    .sort((a, b) => b[1] - a[1])
    .slice(0, max)
    .map(([s]) => s);
}

// ─ Comparison ────────────────────────────────────────────────────────────────
const FIELDS = ['apiCalls', 'retries', 'agentCount'];

function compareSpec(specName, store, harness, events) {
  const dbMetrics = store.metrics(specName) || {};
  const hvState = harness.buildPipelineState(events, { spec: specName }) || {};
  const hvMetrics = hvState.metrics || {};

  const mismatches = [];
  for (const f of FIELDS) {
    const a = dbMetrics[f];
    const b = hvMetrics[f];
    if (a == null && b == null) continue;
    if (a !== b) {
      mismatches.push(f + ': db=' + a + ' hv=' + b);
    }
  }
  return { spec: specName, mismatches, dbMetrics, hvMetrics };
}

// ─ Main ──────────────────────────────────────────────────────────────────────
async function main() {
  setup();
  const events = readEventsJsonl();
  const store = await loadEventStore();
  const harness = loadHarnessViews();

  // Populate specs + metrics_projection from events (migration of a pure jsonl
  // dataset only inserts events; rebuild derives the projections).
  store.rebuild();

  const specs = findRecoverableSpecs(events);
  if (specs.length === 0) {
    console.log('SKIP: no recoverable specs found');
    process.exit(0);
  }

  console.log('Comparing ' + specs.length + ' spec(s):');
  let failures = 0;
  for (const s of specs) {
    const r = compareSpec(s, store, harness, events);
    if (r.mismatches.length === 0) {
      console.log('  PASS ' + s);
    } else {
      console.log('  FAIL ' + s);
      for (const m of r.mismatches) console.log('    - ' + m);
      failures += 1;
    }
  }

  store.close();
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((err) => {
  console.error('ERROR: ' + (err && err.stack ? err.stack : err));
  process.exit(2);
});
