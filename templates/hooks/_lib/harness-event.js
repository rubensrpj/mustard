'use strict';
/**
 * HARNESS-EVENT: Append-only NDJSON event bus shared across hooks.
 *
 * Emits events to `.claude/.harness/events.jsonl`. Each line is a JSON object
 * matching the schema described in `.claude/plans/analise-como-funciona-a-scalable-lerdorf.md`
 * (Wave 1 of the harness migration).
 *
 * Guarantees:
 *   - Atomic line append (fs.appendFileSync, payloads ≤ ~4KB).
 *   - Fail-open: any I/O error is swallowed — callers must never crash.
 *   - Node built-ins only (no npm deps).
 *
 * API:
 *   emit(eventName, payload, context) → boolean  (true on disk write success)
 *   getCurrentSessionId(hookInput)    → string
 *   getCurrentWave(hookInput)         → number   (reads `.harness/index.json`, fallback 0)
 *   getHarnessDir(cwd?)               → string   (absolute path, ensures exists)
 *   getEventsFile(cwd?)               → string   (absolute path to events.jsonl)
 *
 * Environment:
 *   Respects `MUSTARD_DISABLED_HOOKS` / profile via `./hook-env.js#shouldRun('harness-event')`.
 *   Set `MUSTARD_DISABLED_HOOKS=harness-event` to fully disable emission.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

let hookEnv = null;
try {
  hookEnv = require('./hook-env.js');
} catch (_) {
  // hook-env optional; fail-open if unavailable.
  hookEnv = null;
}

const SCHEMA_VERSION = 1;
const HARNESS_DIR_NAME = '.harness';
const EVENTS_FILE_NAME = 'events.jsonl';
const INDEX_FILE_NAME = 'index.json';
const SESSIONS_DIR_NAME = 'sessions';

function isEnabled() {
  try {
    if (hookEnv && typeof hookEnv.shouldRun === 'function') {
      return hookEnv.shouldRun('harness-event');
    }
  } catch (_) {}
  return true;
}

function resolveProjectDir(context) {
  if (context && typeof context === 'object') {
    if (context.cwd && typeof context.cwd === 'string') return context.cwd;
    if (context.projectDir && typeof context.projectDir === 'string') return context.projectDir;
  }
  if (process.env.CLAUDE_PROJECT_DIR) return process.env.CLAUDE_PROJECT_DIR;
  return process.cwd();
}

function getHarnessDir(cwdOrContext) {
  const cwd = typeof cwdOrContext === 'string'
    ? cwdOrContext
    : resolveProjectDir(cwdOrContext);
  const dir = path.join(cwd, '.claude', HARNESS_DIR_NAME);
  try {
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }
  } catch (_) {}
  return dir;
}

function getSessionsDir(cwdOrContext) {
  const dir = path.join(getHarnessDir(cwdOrContext), SESSIONS_DIR_NAME);
  try {
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }
  } catch (_) {}
  return dir;
}

function getEventsFile(cwdOrContext) {
  return path.join(getHarnessDir(cwdOrContext), EVENTS_FILE_NAME);
}

function getIndexFile(cwdOrContext) {
  return path.join(getHarnessDir(cwdOrContext), INDEX_FILE_NAME);
}

function getCurrentSessionId(hookInput) {
  try {
    if (hookInput && typeof hookInput === 'object') {
      if (hookInput.session_id) return String(hookInput.session_id);
      if (hookInput.sessionId) return String(hookInput.sessionId);
    }
    if (process.env.MUSTARD_SESSION_ID) return process.env.MUSTARD_SESSION_ID;
    if (process.env.CLAUDE_SESSION_ID) return process.env.CLAUDE_SESSION_ID;
  } catch (_) {}
  // Generate a stable-ish fallback so events are still groupable within a run.
  try {
    return 's-' + crypto.randomBytes(6).toString('hex');
  } catch (_) {
    return 's-' + Date.now();
  }
}

// Default freshness window for "the newest pipeline-state is still the active one".
// Beyond this, an idle PLAN/BACKLOG spec sitting around must not steal hook events
// from unrelated work — phantom-tagging breaks Mustard Dashboard (standalone) `activeNow` and metrics.
const PIPELINE_STATE_FRESHNESS_MS = (() => {
  const raw = parseInt(process.env.MUSTARD_SPEC_FALLBACK_FRESHNESS_MS || '', 10);
  return Number.isFinite(raw) && raw > 0 ? raw : 10 * 60 * 1000;
})();

function readNewestPipelineState(hookInput, opts) {
  const requireFresh = !!(opts && opts.requireFresh);
  try {
    const cwd = (hookInput && hookInput.cwd) || process.cwd();
    const dir = path.join(cwd, '.claude', '.pipeline-states');
    if (!fs.existsSync(dir)) return null;
    const files = fs.readdirSync(dir).filter(f => f.endsWith('.json') && !f.endsWith('.metrics.json'));
    if (!files.length) return null;
    let best = null, bestT = 0;
    for (const f of files) {
      try {
        const p = path.join(dir, f);
        const st = fs.statSync(p);
        if (st.mtimeMs > bestT) { bestT = st.mtimeMs; best = p; }
      } catch (_) {}
    }
    if (!best) return null;
    if (requireFresh && (Date.now() - bestT) > PIPELINE_STATE_FRESHNESS_MS) return null;
    return JSON.parse(fs.readFileSync(best, 'utf8'));
  } catch (_) {
    return null;
  }
}

function getCurrentWave(hookInput) {
  try {
    if (hookInput && typeof hookInput === 'object') {
      if (typeof hookInput.wave === 'number') return hookInput.wave;
      if (hookInput.tool_input && typeof hookInput.tool_input.wave === 'number') {
        return hookInput.tool_input.wave;
      }
    }
  } catch (_) {}
  try {
    const indexFile = getIndexFile(hookInput);
    if (fs.existsSync(indexFile)) {
      const raw = fs.readFileSync(indexFile, 'utf8');
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed.wave === 'number') return parsed.wave;
    }
  } catch (_) {}
  const ps = readNewestPipelineState(hookInput);
  if (ps && typeof ps.currentWave === 'number') return ps.currentWave;
  return 0;
}

function getCurrentSpec(hookInput) {
  try {
    if (hookInput && typeof hookInput === 'object') {
      if (typeof hookInput.spec === 'string' && hookInput.spec) return hookInput.spec;
      if (hookInput.tool_input && typeof hookInput.tool_input.spec === 'string' && hookInput.tool_input.spec) {
        return hookInput.tool_input.spec;
      }
    }
  } catch (_) {}
  // Freshness gate prevents idle PLAN specs from phantom-tagging unrelated events.
  const ps = readNewestPipelineState(hookInput, { requireFresh: true });
  if (ps) return ps.specName || ps.spec || ps.name || null;
  return null;
}

function normalizeActor(context) {
  const ctx = (context && typeof context === 'object') ? context : {};
  const actor = ctx.actor || {};
  const kind = actor.kind || ctx.actorKind || 'hook';
  const record = { kind };
  if (actor.id || ctx.actorId) record.id = actor.id || ctx.actorId;
  if (actor.type || ctx.actorType) record.type = actor.type || ctx.actorType;
  return record;
}

/**
 * Append a single event to `events.jsonl`.
 *
 * @param {string} eventName  e.g. 'agent.start', 'agent.stop', 'tool.use',
 *                            'pipeline.phase', 'finding', 'decision', 'lesson',
 *                            'dispatch.failure', 'session.start'
 * @param {object} payload    Event-specific data (may be empty).
 * @param {object} context    Optional: { cwd, sessionId, wave, spec, actor, hookInput }
 * @returns {boolean}         true if write succeeded.
 */
function emit(eventName, payload, context) {
  if (!isEnabled()) return false;
  if (!eventName || typeof eventName !== 'string') return false;

  const ctx = (context && typeof context === 'object') ? context : {};
  const hookInput = ctx.hookInput || ctx;

  try {
    const line = {
      v: SCHEMA_VERSION,
      ts: new Date().toISOString(),
      sessionId: ctx.sessionId || getCurrentSessionId(hookInput),
      wave: typeof ctx.wave === 'number' ? ctx.wave : getCurrentWave(hookInput),
      actor: normalizeActor(ctx),
      event: eventName,
      payload: (payload && typeof payload === 'object') ? payload : {},
    };
    const specVal = ctx.spec || getCurrentSpec(hookInput);
    if (specVal) line.spec = String(specVal);

    const file = getEventsFile(ctx.cwd || hookInput);
    fs.appendFileSync(file, JSON.stringify(line) + '\n', 'utf8');

    // Dual-emit to SQLite EventStore (opt-in via MUSTARD_HARNESS_DUAL_EMIT=1).
    // JSONL remains source of truth; SQLite is a projection for live queries.
    // Fail-silent: any storage error is swallowed — the JSONL write already
    // succeeded above, so the event is durable.
    if (process.env.MUSTARD_HARNESS_DUAL_EMIT === '1') {
      try {
        const projectDir = resolveProjectDir(ctx.cwd || hookInput);
        const claudeDir = path.join(projectDir, '.claude');
        const { getStore } = require('./event-store.js');
        const store = getStore(claudeDir);
        if (store && typeof store.append === 'function') {
          store.append(line);
        }
      } catch (_) { /* fail-silent — JSONL is the source of truth */ }
    }

    return true;
  } catch (_) {
    return false;
  }
}

module.exports = {
  emit,
  getCurrentSessionId,
  getCurrentWave,
  getCurrentSpec,
  getHarnessDir,
  getSessionsDir,
  getEventsFile,
  getIndexFile,
  SCHEMA_VERSION,
  EVENTS_FILE_NAME,
  INDEX_FILE_NAME,
  SESSIONS_DIR_NAME,
};
