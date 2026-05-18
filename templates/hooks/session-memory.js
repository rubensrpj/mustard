#!/usr/bin/env bun
'use strict';
/**
 * SESSION-MEMORY: Injects persistent memory into session context
 *
 * Loads sources with priority: knowledge > cross-session-timeline > decisions > lessons.
 * Knowledge entries are ranked by confidence × recency (not just "last N").
 *
 * Wave 3: adds cross-session timeline from .harness/sessions/*.jsonl (fail-open).
 *
 * @version 3.0.0
 */
const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

// ── Harness views (Wave 3) ────────────────────────────────────────────────────
let harnessViews = null;
try {
  harnessViews = require('../scripts/event-projections.js');
} catch (_) {} // fail-open

const MAX_CHARS = 2000;
const KB_MIN_CONFIDENCE = 0.5;
const KB_MAX_ENTRIES = 5;

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('session-memory')) { process.exit(0); }
    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();
    const claudeDir = path.join(cwd, '.claude');
    const memDir = path.join(claudeDir, 'memory');

    const parts = [];

    // Priority 1: Knowledge base (confidence × recency ranked)
    const kbEntries = loadKnowledge(path.join(claudeDir, 'knowledge.json'));
    if (kbEntries.length > 0) {
      parts.push('## Project Knowledge');
      kbEntries.forEach(e => parts.push(`- [${e.type}] ${e.name}: ${e.description}`));
    }

    // Priority 2: Cross-session timeline (Wave 3 — from harness event log)
    // Synchronous variant: stream archived session files from .harness/sessions/
    try {
      if (harnessViews) {
        const sessionsDir = path.join(claudeDir, '.harness', 'sessions');
        const timeline = buildCrossSessionTimelineSync(sessionsDir, { limit: 3 });
        if (timeline.length > 0) {
          parts.push('## Recent Sessions');
          for (const s of timeline) {
            const shortId = (s.sessionId || 'unknown').slice(-6);
            const date = s.endedAt ? s.endedAt.slice(0, 10) : '?';
            const spec = (s.specs || []).join(',') || 'none';
            const decisionsCount = (s.decisions || []).length;
            parts.push(`- Session ${shortId} (${date}): spec=${spec}, decisions=${decisionsCount}`);
          }
        }
      }
    } catch (_) {} // fail-open: timeline is advisory

    // Priority 3: Decisions (most actionable)
    const decisions = loadEntries(path.join(memDir, 'decisions.json'), 5);
    if (decisions.length > 0) {
      parts.push('## Recent Decisions');
      decisions.forEach(d => parts.push(`- [${d.source}] ${d.content}`));
    }

    // Priority 4: Lessons learned
    const lessons = loadEntries(path.join(memDir, 'lessons.json'), 5);
    if (lessons.length > 0) {
      parts.push('## Lessons Learned');
      lessons.forEach(l => parts.push(`- [${l.source}] ${l.content}`));
    }

    if (parts.length > 0) {
      let context = parts.join('\n');
      if (context.length > MAX_CHARS) context = context.slice(0, MAX_CHARS) + '\n...truncated';

      // Measurement: bytes of persistent memory pre-injected into the session
      // context. Without this hook, the agent would need to read the raw
      // sources (knowledge.json + memory/*.json + sessions/*.jsonl) via Read
      // tool calls to retrieve the same condensed view. The injected payload
      // is the byte count we save the agent from re-fetching.
      const payload = `[Persistent Memory]\n${context}`;
      const bytes = Buffer.byteLength(payload, 'utf8');
      try {
        emitMetric('session-memory', {
          tokensAffected: bytes,
          tokensSaved: Math.round(bytes / 4),
          note: 'knowledge-injected',
          extras: {
            kb_entries: kbEntries.length,
            category: 'extraction',
          },
          cwd,
        });
      } catch (_) {}

      console.log(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'SessionStart',
          additionalContext: payload,
        }
      }));
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[session-memory] Error: ${err.message}\n`);
    process.exit(0);
  }
});

/**
 * Synchronous wrapper for cross-session timeline.
 * Reads .harness/sessions/*.jsonl files and returns summaries (most recent first).
 * Uses readEventsSync from event-projections so this stays synchronous (hook-friendly).
 */
function buildCrossSessionTimelineSync(sessionsDir, opts) {
  if (!sessionsDir || !fs.existsSync(sessionsDir)) return [];
  const limit = (opts && opts.limit) || 3;
  try {
    const files = fs.readdirSync(sessionsDir)
      .filter(f => f.endsWith('.jsonl'))
      .map(f => {
        const full = path.join(sessionsDir, f);
        let mtime = 0;
        try { mtime = fs.statSync(full).mtimeMs; } catch (_) {}
        return { file: full, mtime };
      })
      .sort((a, b) => b.mtime - a.mtime)
      .slice(0, limit);

    const results = [];
    for (const entry of files) {
      try {
        const events = harnessViews.readEventsSync(entry.file);
        if (events.length === 0) continue;
        const summary = harnessViews.buildSessionSummary(events);
        summary.file = entry.file;
        summary.mtime = entry.mtime;
        results.push(summary);
      } catch (_) {}
    }
    return results;
  } catch (_) {
    return [];
  }
}

function loadEntries(filePath, max) {
  try {
    if (!fs.existsSync(filePath)) return [];
    const data = JSON.parse(fs.readFileSync(filePath, 'utf8'));
    const entries = data.entries || [];
    return entries.slice(-max);
  } catch { return []; }
}

/**
 * Load knowledge entries filtered by confidence and ranked by confidence × recency.
 * Returns top KB_MAX_ENTRIES entries with confidence >= KB_MIN_CONFIDENCE.
 */
function loadKnowledge(kbPath) {
  try {
    if (!fs.existsSync(kbPath)) return [];
    const kb = JSON.parse(fs.readFileSync(kbPath, 'utf8'));
    const entries = kb.entries || [];
    if (entries.length === 0) return [];

    const now = Date.now();
    // Score: confidence × recency factor (newer = higher)
    // Recency: 1.0 for today, decays to 0.1 over 30 days
    const scored = entries
      .filter(e => (e.confidence || 0) >= KB_MIN_CONFIDENCE)
      .map(e => {
        const ageMs = now - new Date(e.updatedAt || e.createdAt || 0).getTime();
        const ageDays = ageMs / (24 * 60 * 60 * 1000);
        const recency = Math.max(0.1, 1.0 - (ageDays / 30) * 0.9);
        return { ...e, score: (e.confidence || 0) * recency };
      })
      .sort((a, b) => b.score - a.score);

    return scored.slice(0, KB_MAX_ENTRIES);
  } catch { return []; }
}
