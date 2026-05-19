#!/usr/bin/env node
'use strict';
/**
 * MEMORY-AUTO-EXTRACT: SessionEnd hook that extracts non-obvious decisions
 * and lessons from active spec.md files and persists them to
 * .claude/memory/decisions.json and lessons.json.
 *
 * Replaces the previously vaporware "Decision Log" promised in PRD §RF3 —
 * instead of relying on orchestrator discipline to invoke memory-persist.js,
 * this hook scans on session end and extracts opportunistically.
 *
 * Sources scanned:
 *   .claude/spec/active/**\/spec.md  →  ## Decisões não-óbvias  (PT)
 *                                       ## Decisions             (EN)
 *                                       ## Lições (lesson)       (PT)
 *                                       ## Lessons               (EN)
 *
 * Idempotency:
 *   .claude/.memory-seen.json holds SHA-256 hashes of (spec, type, content)
 *   tuples already persisted. Re-scanning the same content is a no-op.
 *
 * Throttle:
 *   Max 5 new entries per session to avoid flooding memory files.
 *
 * Fail-open: exits 0 on any error.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { spawnSync } = require('child_process');
const { shouldRun } = require('./_lib/hook-env.js');
let emitMetric = () => {};
try { emitMetric = require('./_lib/metrics-emit.js').emitMetric; } catch (_) {}

const MAX_ENTRIES_PER_SESSION = 5;
const SECTION_PATTERNS = [
  { type: 'decision', heading: /^##\s+(?:Decisões não-óbvias|Decisions|Decisões)\b/i },
  { type: 'lesson',   heading: /^##\s+(?:Lições|Lessons|Lições aprendidas)\b/i },
];
const BULLET_RE = /^\s*[-*]\s+(.*)$/;

function sha256(s) {
  return crypto.createHash('sha256').update(s).digest('hex').slice(0, 16);
}

function readSeen(seenPath) {
  try {
    if (fs.existsSync(seenPath)) {
      const j = JSON.parse(fs.readFileSync(seenPath, 'utf8'));
      if (j && typeof j === 'object' && Array.isArray(j.hashes)) return j;
    }
  } catch (_) { /* fail-open */ }
  return { hashes: [] };
}

function writeSeen(seenPath, seen) {
  try {
    fs.mkdirSync(path.dirname(seenPath), { recursive: true });
    fs.writeFileSync(seenPath, JSON.stringify(seen, null, 2), 'utf8');
  } catch (_) { /* fail-open */ }
}

/**
 * Extract bullet items under matching section headings.
 * Returns array of { type, content }.
 */
function extractFromSpec(specPath) {
  const out = [];
  let raw;
  try { raw = fs.readFileSync(specPath, 'utf8'); }
  catch (_) { return out; }

  const lines = raw.split('\n');
  let activeType = null;
  for (const line of lines) {
    // Section boundary: any other ## ends current section
    if (/^##\s/.test(line)) {
      activeType = null;
      for (const p of SECTION_PATTERNS) {
        if (p.heading.test(line)) { activeType = p.type; break; }
      }
      continue;
    }
    if (!activeType) continue;
    const m = line.match(BULLET_RE);
    if (!m) continue;
    const text = m[1].trim();
    if (!text) continue;
    // Skip placeholders that would inflate memory with noise
    if (/^(?:nenhuma?|none|n\/a|tbd|todo)$/i.test(text)) continue;
    if (text.length < 8) continue;
    out.push({ type: activeType, content: text });
  }
  return out;
}

function persist(entry, projectDir) {
  const persistScript = path.join(projectDir, '.claude', 'scripts', 'memory-persist.js');
  if (!fs.existsSync(persistScript)) return false;
  try {
    const input = JSON.stringify({
      type: entry.type,
      content: entry.content,
      source: entry.source,
      context: entry.context,
      cwd: projectDir,
    });
    const r = spawnSync(process.execPath, [persistScript], {
      input,
      encoding: 'utf8',
      timeout: 5000,
    });
    return r.status === 0;
  } catch (_) {
    return false;
  }
}

let stdinBuf = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', c => stdinBuf += c);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('memory-auto-extract')) { process.exit(0); }

    let data = {};
    try { data = JSON.parse(stdinBuf || '{}'); } catch (_) { /* tolerate empty */ }
    const cwd = data.cwd || process.cwd();
    const claudeDir = path.join(cwd, '.claude');
    const activeDir = path.join(claudeDir, 'spec', 'active');
    if (!fs.existsSync(activeDir)) { process.exit(0); }

    // Ensure memory/ exists so consumers don't fail
    try { fs.mkdirSync(path.join(claudeDir, 'memory'), { recursive: true }); } catch (_) {}

    const seenPath = path.join(claudeDir, '.memory-seen.json');
    const seen = readSeen(seenPath);
    const seenSet = new Set(seen.hashes);

    let persistedCount = 0;
    let bytesPersisted = 0;
    const newHashes = [];

    // Walk spec/active for spec.md files (non-recursive into wave dirs handled too)
    const specFiles = [];
    function walk(dir) {
      let entries;
      try { entries = fs.readdirSync(dir, { withFileTypes: true }); }
      catch (_) { return; }
      for (const e of entries) {
        const full = path.join(dir, e.name);
        if (e.isDirectory()) walk(full);
        else if (e.isFile() && e.name === 'spec.md') specFiles.push(full);
      }
    }
    walk(activeDir);

    for (const specPath of specFiles) {
      if (persistedCount >= MAX_ENTRIES_PER_SESSION) break;
      const specName = path.relative(activeDir, path.dirname(specPath)).replace(/\\/g, '/');
      const items = extractFromSpec(specPath);
      for (const it of items) {
        if (persistedCount >= MAX_ENTRIES_PER_SESSION) break;
        const hash = sha256(`${specName}|${it.type}|${it.content}`);
        if (seenSet.has(hash)) continue;
        const ok = persist({
          type: it.type,
          content: it.content,
          source: `spec:${specName}`,
          context: '',
        }, cwd);
        if (ok) {
          seenSet.add(hash);
          newHashes.push(hash);
          persistedCount++;
          bytesPersisted += Buffer.byteLength(it.content, 'utf8');
        }
      }
    }

    if (newHashes.length > 0) {
      seen.hashes = [...seen.hashes, ...newHashes].slice(-500); // keep last 500
      writeSeen(seenPath, seen);
    }

    if (persistedCount > 0) {
      // tokensSaved estimates the prompt re-reading cost avoided in future sessions
      // (extracted content is now in memory.json; sessions skip re-parsing spec.md).
      // 4 bytes/token conservative estimate.
      const tokensSaved = Math.round(bytesPersisted / 4);
      try {
        emitMetric('memory-auto-extract', {
          tokensAffected: bytesPersisted,
          tokensSaved,
          note: 'extracted-' + persistedCount,
          extras: { entries: persistedCount, category: 'extraction' },
          cwd,
        });
      } catch (_) {}
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[memory-auto-extract] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
