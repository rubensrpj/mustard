#!/usr/bin/env bun
'use strict';
/**
 * EPIC-FOLD: Wave 8 — Consolidate and compact events when an epic completes.
 *
 * Exported API:
 *   detectCompletedEpics({ cwd? }) → string[]
 *   foldEpic({ epic, cwd? })       → boolean
 *
 * CLI:
 *   node epic-fold.js --detect
 *   node epic-fold.js --epic <name> [--cwd <path>]
 *
 * Rules:
 *   - Fail-open: any error → warning to stderr, returns false / empty array.
 *   - Idempotent: calling foldEpic twice is safe (skips if already CLOSE or
 *     epic.complete event already exists in log).
 *   - Node built-ins only. No npm deps.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

// ── Harness event emitter (optional — fail-open if missing) ──────────────────

let harnessEvent = null;
try {
  harnessEvent = require('./../hooks/_lib/harness-event.js');
} catch (_) {}

// ── Helpers ───────────────────────────────────────────────────────────────────

function readJsonFile(filePath) {
  try {
    if (!fs.existsSync(filePath)) return null;
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (_) {
    return null;
  }
}

function writeJsonFile(filePath, obj) {
  try {
    const dir = path.dirname(filePath);
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(filePath, JSON.stringify(obj, null, 2) + '\n', 'utf8');
    return true;
  } catch (err) {
    process.stderr.write(`[epic-fold] warn: could not write ${filePath}: ${err.message}\n`);
    return false;
  }
}

/**
 * Parse an NDJSON file synchronously. Invalid lines are skipped.
 */
function readEventsSync(filePath) {
  const out = [];
  try {
    if (!fs.existsSync(filePath)) return out;
    const raw = fs.readFileSync(filePath, 'utf8');
    for (const line of raw.split(/\r?\n/)) {
      const trimmed = line && line.trim();
      if (!trimmed) continue;
      try { out.push(JSON.parse(trimmed)); } catch (_) {}
    }
  } catch (_) {}
  return out;
}

function emitEvent(eventName, payload, opts) {
  if (!harnessEvent || typeof harnessEvent.emit !== 'function') return false;
  try {
    return harnessEvent.emit(eventName, payload, {
      cwd: opts.cwd,
      spec: opts.spec || undefined,
      actor: { kind: 'script', id: 'epic-fold' },
    });
  } catch (_) {
    return false;
  }
}

// ── detectCompletedEpics ──────────────────────────────────────────────────────

/**
 * Scan .pipeline-states/*.json and return epic names that are candidates for fold:
 *   - parent_spec === null (root spec)
 *   - children_specs.length > 0
 *   - ALL children have phase === "CLOSE"
 *   - Root itself NOT yet phase === "CLOSE"
 *
 * Specs without parent/children fields (pre-Wave 7) are ignored.
 *
 * @param {object} opts  { cwd? }
 * @returns {string[]}   Array of epic spec names ready to fold.
 */
function detectCompletedEpics(opts) {
  const options = opts || {};
  const cwd = typeof options.cwd === 'string' ? options.cwd : process.cwd();
  const statesDir = path.join(cwd, '.claude', '.pipeline-states');

  const candidates = [];
  try {
    if (!fs.existsSync(statesDir)) return candidates;

    const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
    for (const file of files) {
      try {
        const filePath = path.join(statesDir, file);
        const state = readJsonFile(filePath);
        if (!state) continue;

        // Must be a root spec (Wave 7 field)
        if (state.parent_spec !== null && state.parent_spec !== undefined) continue;
        const children = Array.isArray(state.children_specs) ? state.children_specs : [];
        if (children.length === 0) continue;

        // Root must NOT already be CLOSE
        const rootPhase = String(state.phaseName || state.phase || '').toUpperCase();
        if (rootPhase === 'CLOSE') continue;

        // All children must be CLOSE
        let allClosed = true;
        for (const child of children) {
          const childFile = path.join(statesDir, child + '.json');
          const childState = readJsonFile(childFile);
          if (!childState) { allClosed = false; break; }
          const childPhase = String(childState.phaseName || childState.phase || '').toUpperCase();
          if (childPhase !== 'CLOSE') { allClosed = false; break; }
        }

        if (allClosed) {
          const specName = state.spec || file.replace(/\.json$/, '');
          candidates.push(specName);
        }
      } catch (_) {}
    }
  } catch (err) {
    process.stderr.write(`[epic-fold] warn: detectCompletedEpics error: ${err.message}\n`);
  }
  return candidates;
}

// ── foldEpic ──────────────────────────────────────────────────────────────────

/**
 * Consolidate an epic that has all children in CLOSE:
 *  1. Check idempotency (already folded? skip).
 *  2. Read all harness events for epic + children.
 *  3. Aggregate stats.
 *  4. Emit epic.complete event.
 *  5. Write epic-summary entry to knowledge.json.
 *  6. Transition root to phase: "CLOSE".
 *  7. Emit epic.fold tombstone (marks granular events as compactable).
 *
 * @param {object} opts  { epic, cwd? }
 * @returns {boolean}    true if fold succeeded (or was already done).
 */
function foldEpic(opts) {
  const options = opts || {};
  const epic = typeof options.epic === 'string' ? options.epic.trim() : '';
  const cwd = typeof options.cwd === 'string' ? options.cwd : process.cwd();

  if (!epic) {
    process.stderr.write('[epic-fold] warn: --epic is required\n');
    return false;
  }

  try {
    const statesDir = path.join(cwd, '.claude', '.pipeline-states');
    const epicFile = path.join(statesDir, epic + '.json');
    const epicState = readJsonFile(epicFile);
    const eventsFile = path.join(cwd, '.claude', '.harness', 'events.jsonl');

    // ── Guard: root must exist ───────────────────────────────────────────────
    if (!epicState) {
      process.stderr.write(`[epic-fold] warn: pipeline-state not found for epic "${epic}"\n`);
      return false;
    }

    const children = Array.isArray(epicState.children_specs) ? epicState.children_specs : [];

    // ── Idempotency check 1: root already CLOSE ──────────────────────────────
    const currentPhase = String(epicState.phaseName || epicState.phase || '').toUpperCase();
    if (currentPhase === 'CLOSE') {
      // Already folded — silently succeed
      return true;
    }

    // ── Idempotency check 2: epic.complete event already in log ─────────────
    const existingEvents = readEventsSync(eventsFile);
    const alreadyComplete = existingEvents.some(
      ev => ev.event === 'epic.complete' && ev.payload && ev.payload.epic === epic
    );
    if (alreadyComplete) {
      // Transition root state if it somehow wasn't updated
      const state = readJsonFile(epicFile) || epicState;
      state.phase = 'CLOSE';
      state.phaseName = 'CLOSE';
      writeJsonFile(epicFile, state);
      return true;
    }

    // ── Step 2: Aggregate events for epic + children ─────────────────────────
    const specSet = new Set([epic, ...children]);
    const epicEvents = existingEvents.filter(ev => ev.spec && specSet.has(ev.spec));

    let findingsCount = 0;
    let decisionsCount = 0;
    let lessonsCount = 0;
    let toolCallsTotal = 0;
    let agentsTotal = 0;
    let minTs = null;
    let maxTs = null;

    const findingEvents = [];

    for (const ev of epicEvents) {
      if (ev.ts) {
        if (!minTs || ev.ts < minTs) minTs = ev.ts;
        if (!maxTs || ev.ts > maxTs) maxTs = ev.ts;
      }
      switch (ev.event) {
        case 'finding':
          findingsCount++;
          findingEvents.push(ev);
          break;
        case 'decision':
          decisionsCount++;
          break;
        case 'lesson':
          lessonsCount++;
          break;
        case 'tool.use':
          toolCallsTotal++;
          break;
        case 'agent.start':
          agentsTotal++;
          break;
        default:
          break;
      }
    }

    // Duration in ms
    let durationMs = 0;
    if (minTs && maxTs) {
      try {
        durationMs = new Date(maxTs).getTime() - new Date(minTs).getTime();
        if (!Number.isFinite(durationMs) || durationMs < 0) durationMs = 0;
      } catch (_) { durationMs = 0; }
    }

    const startedAt = minTs || new Date().toISOString();
    const endedAt = maxTs || new Date().toISOString();

    // Top 3 findings by confidence
    findingEvents.sort((a, b) => {
      const ca = (a.payload && typeof a.payload.confidence === 'number') ? a.payload.confidence : 0;
      const cb = (b.payload && typeof b.payload.confidence === 'number') ? b.payload.confidence : 0;
      return cb - ca;
    });
    const top3Findings = findingEvents.slice(0, 3);

    // ── Step 3: Emit epic.complete ───────────────────────────────────────────
    const completePayload = {
      epic,
      children: [...children],
      findings_count: findingsCount,
      decisions_count: decisionsCount,
      lessons_count: lessonsCount,
      tool_calls_total: toolCallsTotal,
      agents_total: agentsTotal,
      duration_ms: durationMs,
      started_at: startedAt,
      ended_at: endedAt,
    };

    emitEvent('epic.complete', completePayload, { cwd, spec: epic });

    // ── Step 4: Write to knowledge.json ─────────────────────────────────────
    // Build content string from top findings + decision/lesson counts
    const findingLines = top3Findings.map((fev, i) => {
      const content = (fev.payload && fev.payload.content) || '';
      const conf = (fev.payload && typeof fev.payload.confidence === 'number')
        ? fev.payload.confidence.toFixed(2)
        : '?';
      return `${i + 1}. [conf=${conf}] ${content}`;
    });

    const contentParts = [];
    if (findingLines.length > 0) {
      contentParts.push('Top findings:\n' + findingLines.join('\n'));
    }
    contentParts.push(`Decisions: ${decisionsCount}`);
    contentParts.push(`Lessons: ${lessonsCount}`);

    const knowledgeEntry = {
      type: 'epic-summary',
      name: epic,
      description: `Epic concluded with ${children.length} child spec(s): ${children.join(', ')}`,
      source: 'epic-fold',
      tags: ['epic', 'summary'],
      confidence: 0.85,
      cwd,
      // Extra fields stored via content field as extended description
      content: contentParts.join('\n\n'),
      spec_children: [...children],
      concluded_at: endedAt,
    };

    writeKnowledgeEntry(knowledgeEntry, cwd);

    // ── Step 5: Transition root to CLOSE ─────────────────────────────────────
    const updatedState = Object.assign({}, epicState);
    updatedState.phase = 'CLOSE';
    updatedState.phaseName = 'CLOSE';
    updatedState.folded_at = new Date().toISOString();
    writeJsonFile(epicFile, updatedState);

    // ── Step 6: Emit epic.fold tombstone ─────────────────────────────────────
    emitEvent('epic.fold', {
      epic,
      compactable_specs: [epic, ...children],
      folded_at: new Date().toISOString(),
    }, { cwd, spec: epic });

    return true;
  } catch (err) {
    process.stderr.write(`[epic-fold] warn: foldEpic("${epic}") error: ${err.message}\n`);
    return false;
  }
}

// ── writeKnowledgeEntry: sync write using knowledge-update.js protocol ────────

/**
 * Write an epic-summary entry to knowledge.json.
 * Uses the same dedup logic as knowledge-update.js (same name+type = update).
 * Falls back to direct file write if knowledge-update.js is unavailable.
 */
function writeKnowledgeEntry(entry, cwd) {
  // Try via knowledge-update.js (spawning it synchronously via stdin pipe)
  // After delegating to knowledge-update.js, patch extra epic-specific fields
  // directly since knowledge-update.js only handles the standard schema.
  try {
    const scriptPath = path.join(__dirname, 'memory.js');
    if (fs.existsSync(scriptPath)) {
      const input = JSON.stringify(entry);
      execFileSync(process.execPath, [scriptPath, 'knowledge'], {
        input,
        stdio: ['pipe', 'pipe', 'pipe'],
        timeout: 5000,
      });
      // Patch epic-specific fields that knowledge-update.js doesn't store
      patchEpicFields(entry, cwd);
      return true;
    }
  } catch (_) {}

  // Fallback: write directly to knowledge.json with same dedup logic
  try {
    const kbPath = path.join(cwd, '.claude', 'knowledge.json');
    let kb = { version: 1, entries: [] };
    try {
      if (fs.existsSync(kbPath)) {
        kb = JSON.parse(fs.readFileSync(kbPath, 'utf8'));
        if (!kb.entries) kb.entries = [];
      }
    } catch (_) {
      kb = { version: 1, entries: [] };
    }

    const ts = new Date().toISOString();
    const existIdx = kb.entries.findIndex(e => e.name === entry.name && e.type === entry.type);
    if (existIdx >= 0) {
      const ex = kb.entries[existIdx];
      ex.description = entry.description;
      ex.source = entry.source;
      ex.tags = entry.tags || [];
      ex.updatedAt = ts;
      ex.lastSeen = ts;
      ex.occurrences = (ex.occurrences || 1) + 1;
      ex.confidence = Math.min(1.0, 0.3 + ex.occurrences * 0.1);
      // Store extra epic fields
      if (entry.content) ex.content = entry.content;
      if (entry.spec_children) ex.spec_children = entry.spec_children;
      if (entry.concluded_at) ex.concluded_at = entry.concluded_at;
    } else {
      kb.entries.push({
        id: `epic-summary-${Date.now()}`,
        type: entry.type,
        name: entry.name,
        description: entry.description,
        source: entry.source,
        tags: entry.tags || [],
        confidence: 0.85,
        occurrences: 1,
        createdAt: ts,
        updatedAt: ts,
        lastSeen: ts,
        content: entry.content || '',
        spec_children: entry.spec_children || [],
        concluded_at: entry.concluded_at || ts,
      });
    }

    const dir = path.dirname(kbPath);
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(kbPath, JSON.stringify(kb, null, 2), 'utf8');
    return true;
  } catch (err) {
    process.stderr.write(`[epic-fold] warn: writeKnowledgeEntry error: ${err.message}\n`);
    return false;
  }
}

/**
 * Patch epic-specific fields (spec_children, concluded_at, content) onto the
 * knowledge.json entry after knowledge-update.js has written the base entry.
 * This is a no-op if the fields are already present.
 */
function patchEpicFields(entry, cwd) {
  try {
    const kbPath = path.join(cwd, '.claude', 'knowledge.json');
    if (!fs.existsSync(kbPath)) return;
    const kb = JSON.parse(fs.readFileSync(kbPath, 'utf8'));
    if (!Array.isArray(kb.entries)) return;
    const idx = kb.entries.findIndex(e => e.name === entry.name && e.type === entry.type);
    if (idx < 0) return;
    const ex = kb.entries[idx];
    if (entry.spec_children) ex.spec_children = entry.spec_children;
    if (entry.concluded_at) ex.concluded_at = entry.concluded_at;
    if (entry.content) ex.content = entry.content;
    if (typeof entry.confidence === 'number'
        && (typeof ex.confidence !== 'number' || ex.confidence < entry.confidence)) {
      ex.confidence = entry.confidence;
    }
    fs.writeFileSync(kbPath, JSON.stringify(kb, null, 2), 'utf8');
  } catch (_) {}
}

module.exports = { detectCompletedEpics, foldEpic };

// ── CLI ───────────────────────────────────────────────────────────────────────
if (require.main === module) {
  (function () {
    try {
      const args = process.argv.slice(2);

      function getArg(name) {
        const idx = args.indexOf('--' + name);
        return idx >= 0 ? args[idx + 1] : null;
      }
      function hasFlag(name) {
        return args.includes('--' + name);
      }

      const cwd = getArg('cwd') || process.cwd();

      if (hasFlag('detect')) {
        const epics = detectCompletedEpics({ cwd });
        process.stdout.write(JSON.stringify({ epics_ready: epics }, null, 2) + '\n');
        process.exit(0);
      }

      const epicArg = getArg('epic');
      if (epicArg) {
        const ok = foldEpic({ epic: epicArg, cwd });
        process.stdout.write(JSON.stringify({ ok, epic: epicArg }) + '\n');
        process.exit(0);
      }

      // No recognised flag
      process.stderr.write('Usage:\n');
      process.stderr.write('  node epic-fold.js --detect [--cwd <path>]\n');
      process.stderr.write('  node epic-fold.js --epic <name> [--cwd <path>]\n');
    } catch (err) {
      process.stderr.write(`[epic-fold] error: ${err.message}\n`);
    }
    process.exit(0);
  })();
}
