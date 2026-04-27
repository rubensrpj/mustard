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

function getCurrentWave(hookInput) {
  // Allow explicit override from hook input.
  try {
    if (hookInput && typeof hookInput === 'object') {
      if (typeof hookInput.wave === 'number') return hookInput.wave;
      if (hookInput.tool_input && typeof hookInput.tool_input.wave === 'number') {
        return hookInput.tool_input.wave;
      }
    }
  } catch (_) {}
  // Read `.harness/index.json` if present.
  try {
    const indexFile = getIndexFile(hookInput);
    if (fs.existsSync(indexFile)) {
      const raw = fs.readFileSync(indexFile, 'utf8');
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed.wave === 'number') return parsed.wave;
    }
  } catch (_) {}
  return 0;
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
    if (ctx.spec) line.spec = String(ctx.spec);

    const file = getEventsFile(ctx.cwd || hookInput);
    fs.appendFileSync(file, JSON.stringify(line) + '\n', 'utf8');
    return true;
  } catch (_) {
    return false;
  }
}

module.exports = {
  emit,
  getCurrentSessionId,
  getCurrentWave,
  getHarnessDir,
  getSessionsDir,
  getEventsFile,
  getIndexFile,
  SCHEMA_VERSION,
  EVENTS_FILE_NAME,
  INDEX_FILE_NAME,
  SESSIONS_DIR_NAME,
};
