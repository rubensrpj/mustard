#!/usr/bin/env bun
/**
 * SPEC-EXTRACT: Cut a single wave slice (or AC block) from a spec.md
 *
 * Why: wave N+1 agent only needs its own section + the previous wave's diff.
 * Re-sending the full spec to every wave is the main driver of prompt bloat.
 *
 * Two spec layouts are supported:
 *   - monolithic     One spec.md with `### {Role} Agent (Wave N)` sub-headers.
 *                    Slice = that section. Omitted = rest of the spec.
 *   - wave-plan      A wave-plan dir: `{specName}/wave-N-{role}/spec.md` per wave
 *                    plus a `wave-plan.md` index. Each wave's spec.md IS already
 *                    the slice — the agent needs it whole. Omitted = wave-plan.md
 *                    + every sibling wave's spec.md (never sent to this agent).
 *
 * The layout is auto-detected from the spec path. `--wave N` on a wave-plan
 * sub-spec returns the sub-spec whole (capped generously) instead of failing
 * because no `(Wave N)` sub-header exists.
 *
 * Usage:
 *   bun .claude/scripts/spec-extract.js --spec <path> --wave <N>
 *   bun .claude/scripts/spec-extract.js --spec <path> --ac
 *   bun .claude/scripts/spec-extract.js --spec <path> --wave <N> --measure
 *
 * `--measure` prints a JSON line instead of the slice text:
 *   {"mode","full_bytes","slice_bytes","omitted_bytes","omitted_detail"}
 *
 * Programmatic:
 *   const { extractWave, extractAcceptanceCriteria, measure, detectMode } = require('./spec-extract.js');
 *
 * Exit codes:
 *   0  success (stdout = slice or JSON)
 *   1  spec missing or section not found
 *
 * Style: fail-graceful, no external deps. Mirrors diff-context.js.
 *
 * @version 2.0.0
 */

const fs = require('fs');
const path = require('path');

const MAX_CHARS = 4000;             // monolithic wave section cap (one section)
const WAVE_PLAN_SOFT_LIMIT = 50000; // advisory only — a per-wave sub-spec is NEVER
                                    // truncated (truncation drops Tasks/AC/Boundaries
                                    // and silently corrupts the dispatch); we warn
                                    // instead so the wave can be split.
const TRUNCATE_TAIL = '\n...[truncated]';

function readSpec(specPath) {
  try {
    return fs.readFileSync(specPath, 'utf8');
  } catch {
    return null;
  }
}

/**
 * Detect spec layout from the path.
 *   .../{specName}/wave-{N}-{role}/spec.md  → 'wave-plan'
 *   anything else                           → 'monolithic'
 * Returns { mode, waveNum } — waveNum is the external wave number for wave-plan.
 */
function detectMode(specPath) {
  const norm = String(specPath).replace(/\\/g, '/');
  const m = norm.match(/\/wave-(\d+)-[^/]+\/spec\.md$/i);
  if (m) return { mode: 'wave-plan', waveNum: Number(m[1]) };
  return { mode: 'monolithic', waveNum: null };
}

function sliceFromHeading(text, headingRegex, nextHeadingRegex) {
  const start = text.search(headingRegex);
  if (start < 0) return null;
  const rest = text.slice(start);
  const nextRel = rest.slice(1).search(nextHeadingRegex);
  const end = nextRel < 0 ? rest.length : nextRel + 1;
  return rest.slice(0, end).replace(/\s+$/, '');
}

/**
 * Extract a wave section.
 *
 * monolithic:  finds `### {Role} Agent (Wave N)`, returns that section.
 * wave-plan:   the sub-spec IS the slice — returns the whole file (capped at
 *              WAVE_PLAN_MAX_CHARS). `n` is informational only here.
 *
 * @param {string} specPath
 * @param {number} n
 * @returns {string|null}
 */
function extractWave(specPath, n) {
  const text = readSpec(specPath);
  if (text === null) return null;

  const { mode } = detectMode(specPath);
  if (mode === 'wave-plan') {
    // The per-wave spec.md is already the natural slice. Hand it over whole.
    return text.replace(/\s+$/, '');
  }

  const num = Number(n);
  if (!Number.isInteger(num) || num < 1) return null;
  // Case-insensitive H3 with any role name, ending in "(Wave N)".
  const heading = new RegExp(`^###\\s+[^\\n]*\\(Wave\\s+${num}\\)[^\\n]*$`, 'mi');
  const nextHeading = /^### /m;
  return sliceFromHeading(text, heading, nextHeading);
}

/**
 * Extract the `## Acceptance Criteria` section (until next `## ` heading).
 */
function extractAcceptanceCriteria(specPath) {
  const text = readSpec(specPath);
  if (text === null) return null;
  const heading = /^##\s+Acceptance\s+Criteria[^\n]*$/mi;
  const nextHeading = /^## /m;
  return sliceFromHeading(text, heading, nextHeading);
}

/**
 * Measure the counterfactual omission for a wave dispatch.
 *
 * monolithic:  omitted = full spec bytes - extracted section bytes.
 * wave-plan:   omitted = wave-plan.md bytes + every sibling wave spec.md bytes
 *              (content the dispatched agent never receives).
 *
 * @param {string} specPath
 * @param {number} n
 * @returns {{mode,full_bytes,slice_bytes,omitted_bytes,omitted_detail}|null}
 */
function measure(specPath, n) {
  const text = readSpec(specPath);
  if (text === null) return null;
  const fullBytes = Buffer.byteLength(text, 'utf8');
  const { mode } = detectMode(specPath);

  if (mode === 'wave-plan') {
    const slice = extractWave(specPath, n) || '';
    const sliceBytes = Buffer.byteLength(slice, 'utf8');
    const waveDir = path.dirname(specPath);
    const specRoot = path.dirname(waveDir);
    const detail = { wave_plan_md: 0, sibling_specs: 0, sibling_count: 0 };

    try {
      const wavePlan = path.join(specRoot, 'wave-plan.md');
      if (fs.existsSync(wavePlan)) detail.wave_plan_md = fs.statSync(wavePlan).size;
    } catch (_) {}

    try {
      for (const entry of fs.readdirSync(specRoot)) {
        if (!/^wave-\d+-/i.test(entry)) continue;
        const sibDir = path.join(specRoot, entry);
        if (sibDir === waveDir) continue;
        const sibSpec = path.join(sibDir, 'spec.md');
        if (fs.existsSync(sibSpec)) {
          detail.sibling_specs += fs.statSync(sibSpec).size;
          detail.sibling_count++;
        }
      }
    } catch (_) {}

    const omitted = detail.wave_plan_md + detail.sibling_specs;
    return {
      mode,
      full_bytes: fullBytes,
      slice_bytes: sliceBytes,
      omitted_bytes: omitted,
      omitted_detail: detail,
    };
  }

  // monolithic
  const slice = extractWave(specPath, n) || '';
  const sliceBytes = Buffer.byteLength(slice, 'utf8');
  return {
    mode,
    full_bytes: fullBytes,
    slice_bytes: sliceBytes,
    omitted_bytes: Math.max(0, fullBytes - sliceBytes),
    omitted_detail: { rest_of_spec: Math.max(0, fullBytes - sliceBytes) },
  };
}

function cap(s, limit) {
  if (typeof s !== 'string') return '';
  const max = limit || MAX_CHARS;
  if (s.length <= max) return s;
  return s.slice(0, max - TRUNCATE_TAIL.length) + TRUNCATE_TAIL;
}

function parseArgs(argv) {
  const out = { spec: null, wave: null, ac: false, measure: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--spec' && argv[i + 1]) { out.spec = argv[++i]; }
    else if (a === '--wave' && argv[i + 1]) { out.wave = Number(argv[++i]); }
    else if (a === '--ac') { out.ac = true; }
    else if (a === '--measure') { out.measure = true; }
  }
  return out;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.spec) {
    process.stderr.write('[spec-extract] --spec <path> is required\n');
    process.exit(0);
    return;
  }
  if (!fs.existsSync(args.spec)) {
    process.stderr.write(`[spec-extract] spec not found: ${args.spec}\n`);
    process.exit(1);
    return;
  }

  // --measure: emit JSON, no slice text.
  if (args.measure) {
    const m = measure(args.spec, args.wave);
    if (!m) {
      process.stderr.write('[spec-extract] could not measure spec\n');
      process.exit(1);
      return;
    }
    process.stdout.write(JSON.stringify(m) + '\n');
    process.exit(0);
    return;
  }

  const { mode } = detectMode(args.spec);
  let out = null;
  if (args.ac) {
    out = extractAcceptanceCriteria(args.spec);
    if (out === null) {
      process.stderr.write('[spec-extract] ## Acceptance Criteria section not found\n');
      process.exit(1);
      return;
    }
  } else if (Number.isInteger(args.wave) || mode === 'wave-plan') {
    out = extractWave(args.spec, args.wave);
    if (out === null) {
      process.stderr.write(`[spec-extract] Wave ${args.wave} section not found\n`);
      process.exit(1);
      return;
    }
  } else {
    process.stderr.write('[spec-extract] provide --wave <N> or --ac\n');
    process.exit(0);
    return;
  }

  if (mode === 'wave-plan') {
    // A per-wave sub-spec is the agent's whole working contract — every
    // section matters. Truncating it mid-file would silently drop Tasks,
    // Acceptance Criteria or Boundaries. So: never truncate here. If the
    // spec is unusually large, warn (the wave should be split into smaller
    // waves) but still emit it whole.
    if (out.length > WAVE_PLAN_SOFT_LIMIT) {
      process.stderr.write(`[spec-extract] WARN: wave spec is ${out.length} chars (soft limit ${WAVE_PLAN_SOFT_LIMIT}) — consider splitting this wave. Emitting whole (not truncated).\n`);
    }
    process.stdout.write(out + '\n');
  } else {
    process.stdout.write(cap(out, MAX_CHARS) + '\n');
  }
  process.exit(0);
}

if (require.main === module) {
  try { main(); } catch (err) {
    process.stderr.write(`[spec-extract] Error: ${err && err.message}\n`);
    process.exit(0);
  }
}

module.exports = { extractWave, extractAcceptanceCriteria, measure, detectMode };
