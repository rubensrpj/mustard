#!/usr/bin/env bun
'use strict';
/**
 * checklist-auto-mark: PostToolUse hook (Edit|Write) that silently marks
 * Checklist items in the active spec when the edited file matches an item.
 *
 * Matching strategies (first match wins, item-by-item):
 *   1. **Arrow target** — item ends with ` → <path-or-basename>`. The file_path
 *      from the Edit/Write must contain that path, or its basename must equal
 *      the item's basename.
 *   2. **Basename pista** — the item text contains the basename of the edited
 *      file (e.g. item mentions `UserService.cs` and the Edit hits
 *      `src/Services/UserService.cs`).
 *
 * Items without any of these pistas are not touched; close-gate.js will
 * surface them at CLOSE so the user (or agent) can mark them manually.
 *
 * Fail-open: any error → exit 0, no stderr noise. Hook never blocks.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

let shouldRun;
try { ({ shouldRun } = require('./_lib/hook-env.js')); }
catch (_) { shouldRun = () => true; }

let emitMetric;
try { ({ emitMetric } = require('./_lib/metrics-emit.js')); }
catch (_) { emitMetric = () => {}; }

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('checklist-auto-mark')) { process.exit(0); }
    const data = JSON.parse(input);

    const toolName = data.tool_name || '';
    if (toolName !== 'Edit' && toolName !== 'Write') { process.exit(0); }

    const filePath = (data.tool_input && (data.tool_input.file_path || data.tool_input.path)) || '';
    if (!filePath) { process.exit(0); }

    const cwd = data.cwd || process.cwd();
    const specInfo = findActiveSpec(cwd);
    if (!specInfo) { process.exit(0); }

    // Don't auto-mark when the edited file IS the spec itself (avoid loops).
    if (path.resolve(filePath) === path.resolve(specInfo.path)) { process.exit(0); }

    const raw = safeRead(specInfo.path);
    if (!raw) { process.exit(0); }
    const lines = raw.split('\n');
    const section = findChecklistSection(lines);
    if (!section) { process.exit(0); }

    const editedBase = path.basename(filePath);
    const normEdited = filePath.replace(/\\/g, '/').toLowerCase();

    let dirty = false;
    const markedItems = [];
    for (let i = section.startIdx; i < section.endIdx; i++) {
      const m = lines[i].match(/^(\s*-\s+)\[ \](\s+)(.*)$/);
      if (!m) continue;
      const text = m[3];

      let matched = false;

      // Strategy 1: arrow target
      const arrowMatch = text.match(/[→>]\s*([^\s].*?)\s*$/);
      if (arrowMatch) {
        const target = arrowMatch[1].replace(/\\/g, '/').toLowerCase();
        if (normEdited.endsWith(target) || normEdited.includes('/' + target) || normEdited === target) {
          matched = true;
        } else if (path.basename(target) === editedBase.toLowerCase()) {
          matched = true;
        }
      }

      // Strategy 2: basename pista anywhere in item text
      if (!matched) {
        if (editedBase && text.toLowerCase().includes(editedBase.toLowerCase())) {
          matched = true;
        }
      }

      if (matched) {
        lines[i] = `${m[1]}[x]${m[2]}${m[3]}`;
        dirty = true;
        markedItems.push(text);
        // Don't break — multiple items might share the same file pista (rare,
        // but harmless to mark them together since the file was indeed edited).
      }
    }

    if (dirty) {
      try { fs.writeFileSync(specInfo.path, lines.join('\n'), 'utf8'); }
      catch (_) { /* fail-open */ }
      for (const itemText of markedItems) {
        emitMetric('checklist-auto-mark', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'auto-marked',
          extras: { specName: specInfo.name, itemMatched: itemText.slice(0, 60), category: 'workflow' },
        });
      }
    }

    process.exit(0);
  } catch (_) {
    process.exit(0); // fail-open
  }
});

/**
 * Locate the active spec. Strategy:
 *   1. Read the most recently modified `.claude/.pipeline-states/*.json`
 *      and use its `spec` (or `specName`) field.
 *   2. If that fails, look for any `.claude/spec/active/*\/spec.md` and pick
 *      the most recently modified one.
 * Returns { path, name } or null.
 */
function findActiveSpec(cwd) {
  const claudeDir = path.join(cwd, '.claude');
  if (!fs.existsSync(claudeDir)) return null;

  // Strategy 1: pipeline-state
  const statesDir = path.join(claudeDir, '.pipeline-states');
  if (fs.existsSync(statesDir)) {
    let newest = null;
    let newestMtime = 0;
    try {
      for (const f of fs.readdirSync(statesDir)) {
        if (!f.endsWith('.json') || f.endsWith('.metrics.json')) continue;
        const fp = path.join(statesDir, f);
        try {
          const stat = fs.statSync(fp);
          if (stat.mtimeMs > newestMtime) { newestMtime = stat.mtimeMs; newest = fp; }
        } catch (_) {}
      }
    } catch (_) {}
    if (newest) {
      try {
        const obj = JSON.parse(fs.readFileSync(newest, 'utf8'));
        const name = obj.spec || obj.specName;
        if (name) {
          const candidate = path.join(claudeDir, 'spec', 'active', name, 'spec.md');
          if (fs.existsSync(candidate)) return { path: candidate, name };
        }
      } catch (_) {}
    }
  }

  // Strategy 2: scan active/
  const activeDir = path.join(claudeDir, 'spec', 'active');
  if (!fs.existsSync(activeDir)) return null;
  let newest = null;
  let newestMtime = 0;
  try {
    for (const dirName of fs.readdirSync(activeDir)) {
      const candidate = path.join(activeDir, dirName, 'spec.md');
      if (!fs.existsSync(candidate)) continue;
      try {
        const stat = fs.statSync(candidate);
        if (stat.mtimeMs > newestMtime) { newestMtime = stat.mtimeMs; newest = { path: candidate, name: dirName }; }
      } catch (_) {}
    }
  } catch (_) {}
  return newest;
}

function findChecklistSection(lines) {
  let startIdx = -1;
  for (let i = 0; i < lines.length; i++) {
    if (/^##\s+Checklist\b/.test(lines[i])) { startIdx = i + 1; break; }
  }
  if (startIdx === -1) return null;
  let endIdx = lines.length;
  for (let i = startIdx; i < lines.length; i++) {
    if (/^##\s/.test(lines[i])) { endIdx = i; break; }
  }
  return { startIdx, endIdx };
}

function safeRead(p) {
  try { return fs.readFileSync(p, 'utf8'); }
  catch (_) { return null; }
}
