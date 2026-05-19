#!/usr/bin/env bun
'use strict';
/**
 * mark-checklist-item: marks a single `- [ ]` item as `- [x]` in a spec's
 * `## Checklist` section. Idempotent. Cross-shell (Node only — no shell).
 *
 * Usage:
 *   bun .claude/scripts/mark-checklist-item.js --spec <name> --item "<substring>"
 *   bun .claude/scripts/mark-checklist-item.js --spec <name> --line <N>
 *
 * Resolution:
 *   --spec  Looks up `.claude/spec/active/<name>/spec.md`. Accepts the bare
 *           directory name (e.g. `2026-05-08-foo`) or an absolute path to a
 *           spec.md file.
 *   --item  Substring match — case-sensitive — against the text portion of
 *           each `- [ ] <text>` line within the Checklist section.
 *           First match wins. Errors if no match.
 *   --line  1-based line number within the file (alternative to --item, used
 *           when the item text contains characters that are inconvenient on
 *           the shell). The line at that index must be a `- [ ]` checkbox
 *           inside the Checklist section.
 *
 * Exit codes:
 *   0  Item marked, OR item already `[x]` (no-op).
 *   1  Spec not found, no Checklist section, or item not located.
 *   2  Bad invocation (missing required args, conflicting args).
 *
 * Output:
 *   stdout: one line — `marked` | `already-marked` | `error: <reason>`
 *   stderr: only on error / unexpected conditions.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

function parseArgs(argv) {
  const out = { spec: null, item: null, line: null, cwd: null };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--spec') { out.spec = argv[++i]; continue; }
    if (a === '--item') { out.item = argv[++i]; continue; }
    if (a === '--line') { out.line = parseInt(argv[++i], 10); continue; }
    if (a === '--cwd') { out.cwd = argv[++i]; continue; }
    if (a === '--help' || a === '-h') { out.help = true; continue; }
  }
  return out;
}

function die(code, msg) {
  process.stdout.write(`error: ${msg}\n`);
  process.exit(code);
}

function resolveSpecPath(spec, cwd) {
  if (!spec) return null;
  // Absolute path to spec.md
  if (path.isAbsolute(spec) && spec.endsWith('.md') && fs.existsSync(spec)) {
    return spec;
  }
  // Bare name → .claude/spec/active/<name>/spec.md
  const active = path.join(cwd, '.claude', 'spec', 'active', spec, 'spec.md');
  if (fs.existsSync(active)) return active;
  // Maybe user passed full directory
  const asDir = path.join(spec, 'spec.md');
  if (fs.existsSync(asDir)) return asDir;
  return null;
}

/**
 * Locate the `## Checklist` section. Returns { startIdx, endIdx } where
 * startIdx is the line index AFTER the `## Checklist` header (the first body
 * line of the section) and endIdx is the line index of the next `## ` header
 * (exclusive) or the end of file.
 */
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

const CHECKBOX_RE = /^(\s*-\s+)\[([ xX])\](\s+)(.*)$/;

function main() {
  const args = parseArgs(process.argv);
  if (args.help) {
    process.stdout.write('Usage: mark-checklist-item.js --spec <name> (--item <text> | --line <N>) [--cwd <dir>]\n');
    process.exit(0);
  }
  if (!args.spec) die(2, '--spec is required');
  if (!args.item && !args.line) die(2, 'either --item or --line is required');
  if (args.item && args.line) die(2, '--item and --line are mutually exclusive');

  const cwd = args.cwd || process.cwd();
  const specPath = resolveSpecPath(args.spec, cwd);
  if (!specPath) die(1, `spec not found: ${args.spec}`);

  let raw;
  try { raw = fs.readFileSync(specPath, 'utf8'); }
  catch (e) { die(1, `cannot read spec: ${e.message}`); }

  const lines = raw.split('\n');
  const section = findChecklistSection(lines);
  if (!section) die(1, 'no `## Checklist` section in spec');

  let targetIdx = -1;

  if (args.line) {
    const idx = args.line - 1;
    if (idx < section.startIdx || idx >= section.endIdx) {
      die(1, `--line ${args.line} is outside the Checklist section (lines ${section.startIdx + 1}-${section.endIdx})`);
    }
    if (!CHECKBOX_RE.test(lines[idx])) {
      die(1, `--line ${args.line} is not a checkbox`);
    }
    targetIdx = idx;
  } else {
    for (let i = section.startIdx; i < section.endIdx; i++) {
      const m = lines[i].match(CHECKBOX_RE);
      if (!m) continue;
      if (m[2] === ' ' && m[4].includes(args.item)) { targetIdx = i; break; }
    }
    if (targetIdx === -1) {
      // Maybe the only match was already [x] — detect for idempotency.
      for (let i = section.startIdx; i < section.endIdx; i++) {
        const m = lines[i].match(CHECKBOX_RE);
        if (!m) continue;
        if ((m[2] === 'x' || m[2] === 'X') && m[4].includes(args.item)) {
          process.stdout.write('already-marked\n');
          process.exit(0);
        }
      }
      die(1, `no `+'`- [ ]`'+` item matching: ${args.item}`);
    }
  }

  const m = lines[targetIdx].match(CHECKBOX_RE);
  if (m[2] === 'x' || m[2] === 'X') {
    process.stdout.write('already-marked\n');
    process.exit(0);
  }
  lines[targetIdx] = `${m[1]}[x]${m[3]}${m[4]}`;

  try { fs.writeFileSync(specPath, lines.join('\n'), 'utf8'); }
  catch (e) { die(1, `cannot write spec: ${e.message}`); }

  process.stdout.write('marked\n');
  process.exit(0);
}

try { main(); }
catch (err) {
  process.stderr.write(`[mark-checklist-item] ${err.stack || err.message}\n`);
  process.exit(1);
}
