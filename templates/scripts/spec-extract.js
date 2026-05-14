#!/usr/bin/env bun
/**
 * SPEC-EXTRACT: Cut a single wave slice (or AC block) from a spec.md
 *
 * Why: wave N+1 agent only needs its own section + the previous wave's diff.
 * Re-sending the full spec to every wave is the main driver of prompt bloat.
 *
 * Usage:
 *   bun .claude/scripts/spec-extract.js --spec <path> --wave <N>
 *   bun .claude/scripts/spec-extract.js --spec <path> --ac
 *
 * Programmatic:
 *   const { extractWave, extractAcceptanceCriteria } = require('./spec-extract.js');
 *
 * Exit codes:
 *   0  success (stdout = slice, possibly truncated)
 *   1  spec missing or section not found
 *
 * Style: fail-graceful, no external deps. Mirrors diff-context.js.
 *
 * @version 1.0.0
 */

const fs = require('fs');

const MAX_CHARS = 4000;
const TRUNCATE_TAIL = '\n...[truncated]';

function readSpec(specPath) {
  try {
    return fs.readFileSync(specPath, 'utf8');
  } catch {
    return null;
  }
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
 * Extract a wave section. The header is agnostic to role name:
 *   ### Implementation Agent (Wave 2)
 *   ### Backend Agent (Wave 2)
 *   ### Frontend Agent (Wave 2)
 *
 * Stops at the next `### ` heading or EOF.
 *
 * @param {string} specPath
 * @param {number} n
 * @returns {string|null}
 */
function extractWave(specPath, n) {
  const text = readSpec(specPath);
  if (text === null) return null;
  const num = Number(n);
  if (!Number.isInteger(num) || num < 1) return null;
  // Case-insensitive H3 with any role name, ending in "(Wave N)" possibly followed by text.
  const heading = new RegExp(`^###\\s+[^\\n]*\\(Wave\\s+${num}\\)[^\\n]*$`, 'mi');
  const nextHeading = /^### /m;
  return sliceFromHeading(text, heading, nextHeading);
}

/**
 * Extract the `## Acceptance Criteria` section (until next `## ` heading).
 *
 * @param {string} specPath
 * @returns {string|null}
 */
function extractAcceptanceCriteria(specPath) {
  const text = readSpec(specPath);
  if (text === null) return null;
  const heading = /^##\s+Acceptance\s+Criteria[^\n]*$/mi;
  const nextHeading = /^## /m;
  return sliceFromHeading(text, heading, nextHeading);
}

function cap(s) {
  if (typeof s !== 'string') return '';
  if (s.length <= MAX_CHARS) return s;
  return s.slice(0, MAX_CHARS - TRUNCATE_TAIL.length) + TRUNCATE_TAIL;
}

function parseArgs(argv) {
  const out = { spec: null, wave: null, ac: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--spec' && argv[i + 1]) { out.spec = argv[++i]; }
    else if (a === '--wave' && argv[i + 1]) { out.wave = Number(argv[++i]); }
    else if (a === '--ac') { out.ac = true; }
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

  let out = null;
  if (args.ac) {
    out = extractAcceptanceCriteria(args.spec);
    if (out === null) {
      process.stderr.write('[spec-extract] ## Acceptance Criteria section not found\n');
      process.exit(1);
      return;
    }
  } else if (Number.isInteger(args.wave)) {
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

  process.stdout.write(cap(out) + '\n');
  process.exit(0);
}

if (require.main === module) {
  try { main(); } catch (err) {
    process.stderr.write(`[spec-extract] Error: ${err && err.message}\n`);
    process.exit(0);
  }
}

module.exports = { extractWave, extractAcceptanceCriteria };
