#!/usr/bin/env node
'use strict';
/**
 * HARNESS-INIT: SessionStart bootstrap for the harness event bus (Wave 1).
 *
 * Responsibilities:
 *   1. Ensure `.claude/.harness/` and `.claude/.harness/sessions/` exist.
 *   2. Rotate any orphan `events.jsonl` (from a prior session that didn't end
 *      cleanly) into `.harness/sessions/{prevSessionId}.jsonl`.
 *      Wave 8: if MUSTARD_EPIC_COMPACT=1, also compact granular events of
 *      folded epics when rotating to sessions archive.
 *   3. Prune `.harness/sessions/*.jsonl` older than 30 days.
 *   4. Emit a `session.start` event via `harness-event.emit(...)`.
 *
 * Fail-open: any I/O or parse error → exit 0 with no output. Never blocks.
 *
 * @version 1.1.0 (Wave 8: epic compaction on rotation)
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const harness = require('./_lib/harness-event.js');

const RETENTION_MS = 30 * 24 * 60 * 60 * 1000; // 30 days

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('harness-init')) { process.exit(0); }

    let data = {};
    try { data = input ? JSON.parse(input) : {}; } catch (_) { data = {}; }

    const cwd = data.cwd || process.env.CLAUDE_PROJECT_DIR || process.cwd();
    const sessionId = harness.getCurrentSessionId(data);

    // (1) Ensure directories.
    const harnessDir = harness.getHarnessDir(cwd);
    const sessionsDir = harness.getSessionsDir(cwd);

    // (2) Rotate orphan events.jsonl if present and belongs to a prior session.
    // Wave 8: epic compaction applied during rotation when MUSTARD_EPIC_COMPACT=1
    rotateOrphanLog(cwd, sessionsDir, sessionId);

    // (3) Prune sessions older than retention window.
    pruneOldSessions(sessionsDir);

    // (4) Emit session.start.
    harness.emit('session.start', {
      cwd,
      source: data.source || data.matcher || null,
    }, {
      cwd,
      sessionId,
      wave: 0,
      actor: { kind: 'hook', id: 'harness-init' },
      hookInput: data,
    });

    process.exit(0);
  } catch (err) {
    try { process.stderr.write('[harness-init] Error: ' + err.message + '\n'); } catch (_) {}
    process.exit(0);
  }
});

/**
 * If `events.jsonl` exists and its first line references a sessionId different
 * from the current one, rotate the whole file into `sessions/{prevId}.jsonl`.
 * If rotation fails for any reason, leave the file alone (events will still
 * be appended — worst case, multi-session log is readable manually).
 */
function rotateOrphanLog(cwd, sessionsDir, currentSessionId) {
  try {
    const eventsFile = harness.getEventsFile(cwd);
    if (!fs.existsSync(eventsFile)) return;

    const prevSessionId = readFirstSessionId(eventsFile);
    if (!prevSessionId || prevSessionId === currentSessionId) {
      // Either empty/unknown (drop) or continuation of current session (keep).
      if (!prevSessionId) {
        try { fs.unlinkSync(eventsFile); } catch (_) {}
      }
      return;
    }

    const target = path.join(sessionsDir, prevSessionId + '.jsonl');
    try {
      // Wave 8: apply epic compaction when MUSTARD_EPIC_COMPACT=1 (default off)
      const epicCompact = process.env.MUSTARD_EPIC_COMPACT === '1';
      if (epicCompact) {
        // Compact the eventsFile before archiving
        const compacted = compactEpicEvents(eventsFile);
        if (compacted !== null) {
          // Write compacted content to target
          if (fs.existsSync(target)) {
            fs.appendFileSync(target, compacted);
          } else {
            fs.writeFileSync(target, compacted, 'utf8');
          }
          fs.unlinkSync(eventsFile);
        } else {
          // Compaction failed — fall through to normal rotate
          rotateFile(eventsFile, target);
        }
      } else {
        rotateFile(eventsFile, target);
      }
    } catch (_) {
      // Fallback: copy + unlink.
      try {
        const data = fs.readFileSync(eventsFile);
        fs.writeFileSync(target, data);
        fs.unlinkSync(eventsFile);
      } catch (_) {}
    }
  } catch (_) {}
}

/** Move or append eventsFile → target. */
function rotateFile(eventsFile, target) {
  if (fs.existsSync(target)) {
    const data = fs.readFileSync(eventsFile);
    fs.appendFileSync(target, data);
    fs.unlinkSync(eventsFile);
  } else {
    fs.renameSync(eventsFile, target);
  }
}

/**
 * Wave 8 — Epic Compaction
 *
 * Reads the eventsFile, finds all epic.fold events to determine which specs
 * have been folded, then filters out granular events (tool.use, agent.start,
 * agent.stop) for those specs while preserving important event types.
 *
 * Always preserves: spec.link, epic.complete, epic.fold, epic.ready,
 *   finding, decision, lesson, pipeline.phase, session.start, session.end,
 *   dispatch.failure
 *
 * Controlled by MUSTARD_EPIC_COMPACT=1 (default: 0 = keep everything).
 *
 * @param {string} eventsFile  Absolute path to events.jsonl
 * @returns {string|null}      Compacted NDJSON string, or null on failure.
 */
function compactEpicEvents(eventsFile) {
  try {
    if (!fs.existsSync(eventsFile)) return null;
    const raw = fs.readFileSync(eventsFile, 'utf8');
    const lines = raw.split(/\r?\n/).filter(l => l.trim());

    // Find all folded spec sets from epic.fold events
    const compactableSpecs = new Set();
    for (const line of lines) {
      try {
        const ev = JSON.parse(line);
        if (ev.event === 'epic.fold' && ev.payload && Array.isArray(ev.payload.compactable_specs)) {
          for (const s of ev.payload.compactable_specs) compactableSpecs.add(s);
        }
      } catch (_) {}
    }

    if (compactableSpecs.size === 0) {
      // Nothing to compact — return original content
      return raw;
    }

    // Event types to KEEP even for folded specs
    const KEEP_EVENTS = new Set([
      'spec.link', 'epic.complete', 'epic.fold', 'epic.ready',
      'finding', 'decision', 'lesson', 'pipeline.phase',
      'session.start', 'session.end', 'dispatch.failure',
    ]);

    // Event types to DROP for folded specs
    const DROP_FOR_FOLDED = new Set([
      'tool.use', 'agent.start', 'agent.stop',
    ]);

    const kept = [];
    for (const line of lines) {
      if (!line.trim()) continue;
      try {
        const ev = JSON.parse(line);
        const isFoldedSpec = ev.spec && compactableSpecs.has(ev.spec);
        if (isFoldedSpec && DROP_FOR_FOLDED.has(ev.event) && !KEEP_EVENTS.has(ev.event)) {
          // Skip this granular event for the folded epic
          continue;
        }
        kept.push(line);
      } catch (_) {
        // Keep unparseable lines as-is (conservative)
        kept.push(line);
      }
    }

    return kept.join('\n') + (kept.length > 0 ? '\n' : '');
  } catch (_) {
    return null;
  }
}

function readFirstSessionId(filePath) {
  try {
    const fd = fs.openSync(filePath, 'r');
    try {
      const buf = Buffer.alloc(4096);
      const bytes = fs.readSync(fd, buf, 0, buf.length, 0);
      const chunk = buf.slice(0, bytes).toString('utf8');
      const firstLine = chunk.split(/\r?\n/)[0];
      if (!firstLine) return null;
      const parsed = JSON.parse(firstLine);
      return (parsed && parsed.sessionId) ? String(parsed.sessionId) : null;
    } finally {
      try { fs.closeSync(fd); } catch (_) {}
    }
  } catch (_) {
    return null;
  }
}

function pruneOldSessions(sessionsDir) {
  try {
    if (!fs.existsSync(sessionsDir)) return;
    const cutoff = Date.now() - RETENTION_MS;
    const files = fs.readdirSync(sessionsDir).filter((f) => f.endsWith('.jsonl'));
    for (const f of files) {
      const fp = path.join(sessionsDir, f);
      try {
        const st = fs.statSync(fp);
        if (st.mtimeMs < cutoff) fs.unlinkSync(fp);
      } catch (_) {}
    }
  } catch (_) {}
}
