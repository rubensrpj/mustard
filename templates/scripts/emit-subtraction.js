#!/usr/bin/env bun
'use strict';
/**
 * emit-subtraction — record a "Mustard intentionally omitted N bytes" event.
 *
 * Mustard performs prompt-shrinking subtractions at orchestration time:
 *   - wave-slice         orchestrator injects only the wave-N section instead
 *                        of the full spec (specs are small — marginal economy)
 *   - diff-vs-full       between waves, the next agent receives the previous
 *                        wave's `git diff` instead of the full touched files.
 *                        This is the BIG one — code files dwarf spec markdown.
 *   - review-diff-first  review agent receives `git diff` inline instead of
 *                        opening files via Read tool
 *   - analyze-diff-skip  ANALYZE phase skips diff-context.js (diff is always
 *                        empty before any work — purely disciplinary, no bytes
 *                        actually omitted from a future call)
 *
 * Each subtraction is a *counterfactual* economy: the bytes were never sent
 * to the API, so they don't appear in Claude Code's OTEL `claude_code.token.usage`
 * stream. Only Mustard knows about them — hence this script.
 *
 * Emits an `mustard.subtraction.applied` event via the harness event bus.
 * Cross-shell (no inline `bun -e` quoting): the orchestrator calls it via
 * `bun .claude/scripts/emit-subtraction.js --type wave-slice --bytes-omitted 10000 --wave 3`.
 *
 * `--measure-spec <path>` replaces the literal `--bytes-omitted`: it runs
 * spec-extract.js's measure() against the spec and uses the computed
 * `omitted_bytes`. This is the reliable path for wave-plan layouts where the
 * orchestrator cannot eyeball the slice delta (the slice is a whole sub-spec
 * file, the omission is the wave-plan index + sibling wave specs).
 *
 * Exit codes:
 *   0  emitted successfully (or fail-silent on internal error)
 *   1  bad CLI arguments
 */
'use strict';

const fs = require('node:fs');
const path = require('node:path');

const VALID_TYPES = new Set(['wave-slice', 'diff-vs-full', 'review-diff-first', 'analyze-diff-skip']);

function parseArgs(argv) {
  const out = { type: null, bytesOmitted: 0, wave: null, spec: null, measureSpec: null, measureDiff: null, diffRoot: null, extras: {} };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const next = argv[i + 1];
    switch (flag) {
      case '--type':
        out.type = next; i++; break;
      case '--bytes-omitted':
        out.bytesOmitted = Number.parseInt(next, 10) || 0; i++; break;
      case '--wave':
        out.wave = Number.parseInt(next, 10); i++; break;
      case '--spec':
        out.spec = next; i++; break;
      case '--measure-spec':
        out.measureSpec = next; i++; break;
      case '--measure-diff':
        out.measureDiff = next; i++; break;
      case '--diff-root':
        out.diffRoot = next; i++; break;
      case '--note':
        out.extras.note = next; i++; break;
      case '-h':
      case '--help':
        printHelp();
        process.exit(0);
        break;
      default:
        // ignore unknown flags rather than failing — fail-silent ethos
        break;
    }
  }
  return out;
}

function printHelp() {
  process.stdout.write(`emit-subtraction — record a Mustard prompt-shrinking subtraction.

Usage:
  bun emit-subtraction.js --type <wave-slice|diff-vs-full|review-diff-first|analyze-diff-skip>
                          [--bytes-omitted N | --measure-spec PATH | --measure-diff PATH]
                          [--wave N] [--spec NAME] [--note STR]

  --measure-spec PATH   measure omitted bytes via spec-extract.js measure()
                        (preferred for wave-slice — handles wave-plan layouts)
  --measure-diff PATH   measure omitted bytes by comparing a git-diff file to
                        the full size of every file it touches (for diff-vs-full)
  --diff-root PATH      root to resolve the diff's file paths against (defaults
                        to CLAUDE_PROJECT_DIR; use it when the diff is from a
                        submodule or a sub-directory)

Exit: 0 on emit (or silent skip), 1 on bad args.
`);
}

/**
 * Measure the diff-vs-full subtraction: when an agent receives a `git diff`
 * instead of the full content of every file the diff touches, the omission is
 * (sum of full file sizes) - (diff size). This is the dominant economy in a
 * multi-wave pipeline — code files are an order of magnitude larger than the
 * spec markdown that `wave-slice` measures.
 *
 * Returns { mode, diff_bytes, full_bytes, omitted_bytes, file_count, resolved,
 * missing } or null (fail-soft).
 */
function measureDiffVsFull(diffPath, fileRoot) {
  try {
    if (!fs.existsSync(diffPath)) return null;
    const diffText = fs.readFileSync(diffPath, 'utf8');
    const diffBytes = Buffer.byteLength(diffText, 'utf8');

    // Extract touched paths. `diff --git a/X b/X` is the canonical header;
    // fall back to `+++ b/X` lines if the diff was produced without it.
    const files = new Set();
    let m;
    const reGit = /^diff --git a\/(.+?) b\//gm;
    while ((m = reGit.exec(diffText)) !== null) files.add(m[1].trim());
    if (files.size === 0) {
      const rePlus = /^\+\+\+ b\/(.+)$/gm;
      while ((m = rePlus.exec(diffText)) !== null) {
        const f = m[1].trim();
        if (f && f !== '/dev/null') files.add(f);
      }
    }

    let fullBytes = 0, resolved = 0, missing = 0;
    for (const f of files) {
      const abs = path.isAbsolute(f) ? f : path.join(fileRoot, f);
      try {
        if (fs.existsSync(abs)) { fullBytes += fs.statSync(abs).size; resolved++; }
        else missing++;
      } catch (_) { missing++; }
    }

    return {
      mode: 'diff-vs-full',
      diff_bytes: diffBytes,
      full_bytes: fullBytes,
      omitted_bytes: Math.max(0, fullBytes - diffBytes),
      file_count: files.size,
      resolved,
      missing,
    };
  } catch (_) {
    return null;
  }
}

/**
 * Load spec-extract.js's measure() from the same scripts dir. Returns the
 * measurement object or null (fail-soft — caller falls back to --bytes-omitted).
 */
function measureViaSpecExtract(measureSpec, wave) {
  try {
    const extractPath = path.join(__dirname, 'spec-extract.js');
    if (!fs.existsSync(extractPath)) return null;
    const { measure } = require(extractPath);
    if (typeof measure !== 'function') return null;
    return measure(measureSpec, wave);
  } catch (_) {
    return null;
  }
}

function resolveProjectDir() {
  if (process.env.CLAUDE_PROJECT_DIR) return process.env.CLAUDE_PROJECT_DIR;
  // Heuristic: script sits at .claude/scripts/, two levels up is project root.
  return path.resolve(__dirname, '..', '..');
}

function loadHarness(projectDir) {
  const harnessLib = path.join(projectDir, '.claude', 'hooks', '_lib', 'harness-event.js');
  if (!fs.existsSync(harnessLib)) return null;
  try {
    return require(harnessLib);
  } catch (_) {
    return null;
  }
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  if (!args.type) {
    process.stderr.write('error: --type required\n');
    printHelp();
    process.exit(1);
  }
  if (!VALID_TYPES.has(args.type)) {
    process.stderr.write(`error: invalid --type "${args.type}" (expected: wave-slice | review-diff-first | analyze-diff-skip)\n`);
    process.exit(1);
  }
  if (args.bytesOmitted < 0) {
    process.stderr.write('error: --bytes-omitted must be non-negative\n');
    process.exit(1);
  }

  const projectDir = resolveProjectDir();
  const harness = loadHarness(projectDir);
  if (!harness) {
    // Fail-silent: harness not installed yet. This is OK during bootstrap.
    process.exit(0);
  }

  // --measure-spec / --measure-diff override --bytes-omitted: compute the
  // omission instead of trusting a hand-passed number.
  let bytesOmitted = args.bytesOmitted;
  let measureDetail = null;
  if (args.measureSpec) {
    const m = measureViaSpecExtract(args.measureSpec, args.wave);
    if (m && Number.isFinite(m.omitted_bytes)) {
      bytesOmitted = m.omitted_bytes;
      measureDetail = m;
    } else {
      process.stderr.write(`[emit-subtraction] measure failed for ${args.measureSpec} — falling back to --bytes-omitted ${args.bytesOmitted}\n`);
    }
  } else if (args.measureDiff) {
    const m = measureDiffVsFull(args.measureDiff, args.diffRoot || projectDir);
    if (m && Number.isFinite(m.omitted_bytes)) {
      bytesOmitted = m.omitted_bytes;
      measureDetail = m;
    } else {
      process.stderr.write(`[emit-subtraction] diff measure failed for ${args.measureDiff} — falling back to --bytes-omitted ${args.bytesOmitted}\n`);
    }
  }

  const payload = {
    type: args.type,
    bytes_omitted: bytesOmitted,
  };
  if (args.wave !== null && Number.isFinite(args.wave)) payload.wave = args.wave;
  if (args.spec) payload.spec = args.spec;
  if (args.extras.note) payload.note = args.extras.note;
  if (measureDetail) {
    payload.measured = true;
    // Copy whatever the measurer produced — fields differ per measurer
    // (spec-extract: slice_bytes/omitted_detail; diff: diff_bytes/file_count).
    for (const k of ['mode', 'full_bytes', 'slice_bytes', 'omitted_detail',
                      'diff_bytes', 'file_count', 'resolved', 'missing']) {
      if (measureDetail[k] !== undefined) payload[k] = measureDetail[k];
    }
  }

  const ctx = {
    cwd: projectDir,
    actor: { kind: 'orchestrator', id: 'emit-subtraction' },
  };
  if (args.spec) ctx.spec = args.spec;

  harness.emit('mustard.subtraction.applied', payload, ctx);
  process.exit(0);
}

main();
