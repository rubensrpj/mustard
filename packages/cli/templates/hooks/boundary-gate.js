#!/usr/bin/env bun
'use strict';
/**
 * BOUNDARY-GATE: PreToolUse(Write|Edit) — flags edits outside the active
 * spec's `## Files` / `## Boundaries` declaration.
 *
 * Reads the newest pipeline-state (fresh window: 10 min) to find specName,
 * opens .claude/spec/active/{specName}/spec.md, parses backtick-wrapped paths
 * inside `## Files` and `## Boundaries` headings, then compares `tool_input.file_path`
 * to those patterns. If unmatched, emit `boundary.expansion` event + WARN.
 *
 * Modes (env MUSTARD_BOUNDARY_MODE):
 *   - off    — disabled
 *   - warn   — default; stderr + harness event, never blocks
 *   - strict — denies the edit via permissionDecision
 *
 * Always allows meta paths (.claude/, dist/, node_modules/, .git/) — these
 * are infrastructure edits the spec rarely lists.
 *
 * Fail-open: any internal error exits 0 without affecting the tool call.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { headingRegex } = require('../scripts/_lib/spec-sections.js');
const { shouldRun } = require('./_lib/hook-env.js');
const { emit, getCurrentSessionId, getCurrentWave } = require('./_lib/harness-event.js');

const META_PREFIXES = [
  '.claude/', '.claude\\',
  'dist/', 'dist\\',
  'node_modules/',
  '.git/',
];

function isMetaPath(rel) {
  if (!rel) return true;
  for (const p of META_PREFIXES) {
    if (rel.startsWith(p)) return true;
  }
  return false;
}

function getMode() {
  return (process.env.MUSTARD_BOUNDARY_MODE || 'warn').toLowerCase();
}

function readNewestFreshState(cwd, freshnessMs) {
  try {
    const dir = path.join(cwd, '.claude', '.pipeline-states');
    if (!fs.existsSync(dir)) return null;
    const files = fs.readdirSync(dir)
      .filter(f => f.endsWith('.json') && !f.endsWith('.metrics.json'));
    if (!files.length) return null;
    let best = null, bestT = 0;
    for (const f of files) {
      try {
        const fp = path.join(dir, f);
        const st = fs.statSync(fp);
        if (st.mtimeMs > bestT) { bestT = st.mtimeMs; best = fp; }
      } catch (_) {}
    }
    if (!best) return null;
    if ((Date.now() - bestT) > freshnessMs) return null;
    return JSON.parse(fs.readFileSync(best, 'utf8'));
  } catch (_) { return null; }
}

function resolveSpecFile(cwd, state) {
  const specName = state && state.specName;
  if (!specName) return null;
  const base = path.join(cwd, '.claude', 'spec', 'active', specName);
  if (!fs.existsSync(base)) return null;

  if (state.isWavePlan && typeof state.currentWave === 'number') {
    try {
      const entries = fs.readdirSync(base);
      const wavePrefix = `wave-${state.currentWave}-`;
      for (const e of entries) {
        if (e.startsWith(wavePrefix)) {
          const cand = path.join(base, e, 'spec.md');
          if (fs.existsSync(cand)) return cand;
        }
      }
    } catch (_) {}
  }
  const root = path.join(base, 'spec.md');
  return fs.existsSync(root) ? root : null;
}

/**
 * Parse `## Files` markdown table + `## Boundaries` bullet list. Returns
 * an array of backtick-wrapped paths/patterns. Globs supported: `*`, `**`,
 * trailing `/` (directory prefix).
 */
function extractAllowedPatterns(specText) {
  const patterns = new Set();
  const lines = specText.split('\n');
  // Recognize EN ("## Files" / "## Boundaries") and PT ("## Arquivos" /
  // "## Limites") headings via the single-source spec-sections module.
  const filesHeading = headingRegex('files');
  const boundariesHeading = headingRegex('boundaries');
  let inFiles = false;
  let inBoundaries = false;

  for (const line of lines) {
    if (filesHeading.test(line)) { inFiles = true; inBoundaries = false; continue; }
    if (boundariesHeading.test(line)) { inBoundaries = true; inFiles = false; continue; }
    if (/^##\s+\S/.test(line)) { inFiles = false; inBoundaries = false; continue; }

    if (!(inFiles || inBoundaries)) continue;

    const re = /`([^`\n]+?)`/g;
    let m;
    while ((m = re.exec(line)) !== null) {
      const candidate = m[1].trim();
      if (!candidate || candidate.length > 200) continue;
      // Reject obvious non-paths
      if (/^[a-z]+\s+--?\w/.test(candidate)) continue;     // cmd with flag
      if (/^[A-Z][A-Z0-9_]*=/.test(candidate)) continue;   // env var assignment
      if (!/[\/.]/.test(candidate)) continue;              // no slash/dot → likely a label
      patterns.add(candidate);
    }
  }
  return Array.from(patterns);
}

function patternMatches(rel, pattern) {
  const r = rel.replace(/\\/g, '/');
  const p = pattern.replace(/\\/g, '/');

  if (r === p) return true;
  if (p.endsWith('/') && r.startsWith(p)) return true;
  // Treat `dir/*` shorthand as one-segment glob
  if (p.includes('*')) {
    const escaped = p
      .replace(/[.+^${}()|[\]\\]/g, '\\$&')
      .replace(/\*\*/g, '__DOUBLESTAR__')
      .replace(/\*/g, '[^/]*')
      .replace(/__DOUBLESTAR__/g, '.*');
    return new RegExp('^' + escaped + '$').test(r);
  }
  return false;
}

let buf = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => { buf += c; });
process.stdin.on('end', () => {
  try {
    if (!shouldRun('boundary-gate')) { process.exit(0); }
    const mode = getMode();
    if (mode === 'off') process.exit(0);

    const data = JSON.parse(buf);
    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || toolInput.path;
    if (!filePath) process.exit(0);

    const cwd = data.cwd || process.cwd();
    const abs = path.isAbsolute(filePath) ? filePath : path.resolve(cwd, filePath);
    const rel = path.relative(cwd, abs).replace(/\\/g, '/');

    if (rel.startsWith('../') || isMetaPath(rel)) process.exit(0);

    const state = readNewestFreshState(cwd, 10 * 60 * 1000);
    if (!state || !state.specName) process.exit(0);
    if (state.phaseName === 'CLOSE' || state.status === 'completed') process.exit(0);

    const specFile = resolveSpecFile(cwd, state);
    if (!specFile) process.exit(0);

    let specText = '';
    try { specText = fs.readFileSync(specFile, 'utf8'); } catch (_) { process.exit(0); }

    const patterns = extractAllowedPatterns(specText);
    if (patterns.length === 0) process.exit(0);

    if (patterns.some(p => patternMatches(rel, p))) process.exit(0);

    const sessionId = getCurrentSessionId(data);
    const wave = getCurrentWave(data);
    try {
      emit('boundary.expansion', {
        file: rel,
        spec: state.specName,
        wave,
        mode,
        sample_patterns: patterns.slice(0, 6),
      }, {
        cwd, sessionId, wave, spec: state.specName,
        actor: { kind: 'hook', id: 'boundary-gate' },
      });
    } catch (_) {}

    if (mode === 'strict') {
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason:
            `[boundary-gate] ${rel} not in spec '${state.specName}' ## Files / ## Boundaries. ` +
            `Update the spec's Files table to include this path, or set MUSTARD_BOUNDARY_MODE=warn.`,
        },
      }) + '\n');
      process.exit(0);
    }

    process.stderr.write(
      `[boundary-gate] WARN: editing ${rel} outside spec '${state.specName}' boundary. ` +
      `If intentional cascade, add it to the spec ## Files. Set MUSTARD_BOUNDARY_MODE=strict to block.\n`
    );
    process.exit(0);
  } catch (_) {
    process.exit(0); // fail-open
  }
});
