#!/usr/bin/env bun
/**
 * CONTEXT-SLICE: Cut the relevant term blocks from a CONTEXT.md glossary.
 *
 * Why: CONTEXT.md (built by the `grill-with-docs` skill) is the project's
 * shared-language glossary. Dumping the whole file into every agent prompt is
 * the same prompt-bloat anti-pattern the entity-registry rule already bans
 * ("Grep the registry for the specific entity, NEVER read the full JSON").
 * This script applies that principle to CONTEXT.md: given the active spec, it
 * returns ONLY the term blocks whose term or definition matches entities, file
 * names, or significant key-tokens of that spec.
 *
 * A "term block" is a markdown section keyed by a heading (`## Term`, `### Term`)
 * or a definition-list line (`**Term** — definition` / `- **Term**: definition`).
 * The block runs until the next sibling heading / definition line.
 *
 * Matching heuristic (relevance):
 *   - exact entity names from the spec's `## Entidades` / `## Entities` section
 *   - exact file basenames from the spec's `## Arquivos` / `## Files` section
 *   - significant tokens from the spec body — derived, NOT a hardcoded list:
 *     a token is significant when it is long enough AND not over-frequent
 *     (common short words show up everywhere; they carry no signal).
 *
 * Backstop: the slice is capped at MUSTARD_GLOSSARY_MAX_LINES lines (default
 * 250). If the relevant slice still exceeds the cap, an actionable warning is
 * printed to stderr and the slice is truncated.
 *
 * Multi-context: `--context` may be given multiple times, or point at a
 * `CONTEXT-MAP.md` (Matt's multi-context index). All resolved CONTEXT.md files
 * are sliced independently and the slices are concatenated + deduped.
 *
 * Usage:
 *   bun .claude/scripts/context-slice.js --context <CONTEXT.md> --spec <spec.md>
 *   bun .claude/scripts/context-slice.js --context a/CONTEXT.md --context b/CONTEXT.md --spec <spec.md>
 *   bun .claude/scripts/context-slice.js --context docs/CONTEXT-MAP.md --spec <spec.md>
 *
 * Programmatic:
 *   const { sliceContext, extractRelevanceTerms, parseTermBlocks } = require('./context-slice.js');
 *
 * Exit codes:
 *   0  always (fail-graceful — missing files never throw, just yield empty)
 *
 * Style: fail-graceful, no external deps. Mirrors spec-extract.js.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

const DEFAULT_MAX_LINES = 250;
const TRUNCATE_TAIL = '\n...[truncated — glossary slice exceeded cap]';

// Token significance thresholds — derived heuristics, not a fixed stopword list.
const MIN_TOKEN_LEN = 4;        // shorter tokens are almost always common words
const MAX_TOKEN_FREQUENCY = 0.04; // a token in >4% of body tokens carries no signal

function readFileSafe(filePath) {
  try {
    return fs.readFileSync(filePath, 'utf8');
  } catch {
    return null;
  }
}

function resolveMaxLines() {
  const raw = Number(process.env.MUSTARD_GLOSSARY_MAX_LINES);
  if (Number.isInteger(raw) && raw > 0) return raw;
  return DEFAULT_MAX_LINES;
}

/**
 * Extract a `## Heading` section body (until next `## ` or `### ` heading).
 * Returns '' when the heading is absent. `names` is a list of accepted
 * heading texts (case-insensitive), e.g. ['Entidades', 'Entities'].
 */
function extractSection(text, names) {
  for (const name of names) {
    const re = new RegExp(`^#{2,3}\\s+${name}\\s*$`, 'mi');
    const start = text.search(re);
    if (start < 0) continue;
    const rest = text.slice(start);
    const nextRel = rest.slice(1).search(/^#{2,3}\s/m);
    const end = nextRel < 0 ? rest.length : nextRel + 1;
    return rest.slice(0, end);
  }
  return '';
}

/**
 * Pull explicit entity/file names from the spec's structured sections plus
 * significant tokens from the body. Returns a lowercase Set.
 *
 * @param {string} specText
 * @returns {Set<string>}
 */
function extractRelevanceTerms(specText) {
  const terms = new Set();
  if (!specText) return terms;

  const addWord = (w) => {
    const t = String(w || '').toLowerCase().trim();
    if (t.length >= 2) terms.add(t);
  };

  // 1. Exact entity names — `## Entidades` / `## Entities`.
  const entitySection = extractSection(specText, ['Entidades', 'Entities']);
  // Identifier-like tokens (PascalCase / camelCase / snake_case words).
  for (const m of entitySection.matchAll(/\b[A-Za-z][A-Za-z0-9_]{1,}\b/g)) {
    addWord(m[0]);
  }

  // 2. Exact file basenames — `## Arquivos` / `## Files`.
  const fileSection = extractSection(specText, ['Arquivos', 'Files']);
  // Match path-ish tokens, then keep both the basename and the stem.
  for (const m of fileSection.matchAll(/[A-Za-z0-9_\-./\\]+\.[A-Za-z0-9]+/g)) {
    const base = path.basename(String(m[0]).replace(/\\/g, '/'));
    addWord(base);
    addWord(base.replace(/\.[A-Za-z0-9]+$/, '')); // stem without extension
  }

  // 3. Significant body tokens — frequency-derived, no hardcoded stopwords.
  const bodyTokens = (specText.toLowerCase().match(/[a-z][a-z0-9_]{2,}/g)) || [];
  if (bodyTokens.length > 0) {
    const freq = new Map();
    for (const tok of bodyTokens) freq.set(tok, (freq.get(tok) || 0) + 1);
    const total = bodyTokens.length;
    for (const [tok, count] of freq) {
      if (tok.length < MIN_TOKEN_LEN) continue;          // too short → common word
      if (count / total > MAX_TOKEN_FREQUENCY) continue; // too frequent → no signal
      terms.add(tok);
    }
  }

  return terms;
}

/**
 * Parse a CONTEXT.md into term blocks.
 *
 * A block starts at either:
 *   - a markdown heading: `## Term` or `### Term`
 *   - a definition line:  `**Term** — ...`, `- **Term**: ...`, `* **Term** ...`
 * and runs until the next block start (or EOF).
 *
 * Returns [{ term, text }]. Content before the first recognised block is
 * dropped (it is preamble, not a term).
 *
 * @param {string} contextText
 * @returns {{term: string, text: string}[]}
 */
function parseTermBlocks(contextText) {
  if (!contextText) return [];
  const lines = contextText.split(/\r?\n/);
  const blocks = [];
  let current = null;

  const headingRe = /^#{2,3}\s+(.+?)\s*$/;
  const defLineRe = /^\s*(?:[-*]\s+)?\*\*(.+?)\*\*/;

  for (const line of lines) {
    const hMatch = line.match(headingRe);
    const dMatch = hMatch ? null : line.match(defLineRe);
    const match = hMatch || dMatch;
    if (match) {
      if (current) blocks.push(current);
      current = { term: match[1].trim(), lines: [line] };
    } else if (current) {
      current.lines.push(line);
    }
  }
  if (current) blocks.push(current);

  return blocks.map((b) => ({
    term: b.term,
    text: b.lines.join('\n').replace(/\s+$/, ''),
  }));
}

/**
 * True when a term block is relevant to the spec's term set.
 * A block matches when any relevance term appears as a whole-word substring
 * of either the block's term name or its full text (case-insensitive).
 */
function blockMatches(block, terms) {
  const hayTerm = block.term.toLowerCase();
  const hayText = block.text.toLowerCase();
  for (const t of terms) {
    if (!t) continue;
    // Whole-word-ish: bounded by non-alphanumeric or string edges.
    const bounded = new RegExp(`(^|[^a-z0-9])${escapeRe(t)}([^a-z0-9]|$)`, 'i');
    if (bounded.test(hayTerm) || bounded.test(hayText)) return true;
  }
  return false;
}

function escapeRe(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * Slice one or more CONTEXT.md files against a spec.
 *
 * @param {string[]} contextPaths  one or many CONTEXT.md / CONTEXT-MAP.md paths
 * @param {string} specPath
 * @param {number} [maxLines]
 * @returns {{ slice: string, lineCount: number, truncated: boolean, blockCount: number }}
 */
function sliceContext(contextPaths, specPath, maxLines) {
  const cap = Number.isInteger(maxLines) && maxLines > 0 ? maxLines : resolveMaxLines();
  const specText = readFileSafe(specPath);
  // No spec → nothing to match against. Graceful empty.
  if (!specText) return { slice: '', lineCount: 0, truncated: false, blockCount: 0 };

  const resolved = resolveContextFiles(contextPaths);
  if (resolved.length === 0) {
    return { slice: '', lineCount: 0, truncated: false, blockCount: 0 };
  }

  const terms = extractRelevanceTerms(specText);
  const seen = new Set(); // dedupe by term name (lowercase) across files
  const matched = [];

  for (const cPath of resolved) {
    const contextText = readFileSafe(cPath);
    if (!contextText) continue;
    for (const block of parseTermBlocks(contextText)) {
      const key = block.term.toLowerCase();
      if (seen.has(key)) continue;
      if (blockMatches(block, terms)) {
        seen.add(key);
        matched.push(block.text);
      }
    }
  }

  if (matched.length === 0) {
    return { slice: '', lineCount: 0, truncated: false, blockCount: 0 };
  }

  const joined = matched.join('\n\n');
  const allLines = joined.split('\n');
  if (allLines.length <= cap) {
    return {
      slice: joined,
      lineCount: allLines.length,
      truncated: false,
      blockCount: matched.length,
    };
  }

  // Backstop: still over cap after relevance filtering — truncate + warn.
  const kept = allLines.slice(0, cap).join('\n') + TRUNCATE_TAIL;
  return {
    slice: kept,
    lineCount: cap,
    truncated: true,
    blockCount: matched.length,
  };
}

/**
 * Resolve --context inputs into a flat list of CONTEXT.md file paths.
 * A `CONTEXT-MAP.md` is expanded: every markdown link / path-ish token inside
 * it that ends in `CONTEXT.md` is followed (resolved relative to the map).
 * Missing files are silently skipped (fail-graceful). Result is deduped.
 *
 * @param {string[]} contextPaths
 * @returns {string[]}
 */
function resolveContextFiles(contextPaths) {
  const out = [];
  const seen = new Set();
  const push = (p) => {
    const norm = path.resolve(p);
    if (seen.has(norm)) return;
    if (!fs.existsSync(norm)) return;
    seen.add(norm);
    out.push(norm);
  };

  for (const raw of contextPaths || []) {
    if (!raw) continue;
    const base = path.basename(String(raw).replace(/\\/g, '/')).toLowerCase();
    if (base === 'context-map.md') {
      const mapText = readFileSafe(raw);
      if (!mapText) continue;
      const mapDir = path.dirname(raw);
      for (const m of mapText.matchAll(/[A-Za-z0-9_\-./\\]*context\.md/gi)) {
        const ref = m[0].replace(/\\/g, '/');
        push(path.isAbsolute(ref) ? ref : path.join(mapDir, ref));
      }
    } else {
      push(raw);
    }
  }
  return out;
}

function parseArgs(argv) {
  const out = { context: [], spec: null, maxLines: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--context' && argv[i + 1]) { out.context.push(argv[++i]); }
    else if (a === '--spec' && argv[i + 1]) { out.spec = argv[++i]; }
    else if (a === '--max-lines' && argv[i + 1]) { out.maxLines = Number(argv[++i]); }
  }
  return out;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.spec) {
    process.stderr.write('[context-slice] --spec <path> is required\n');
    process.exit(0);
    return;
  }
  if (args.context.length === 0) {
    // No CONTEXT.md provided → graceful empty (degrade, never error).
    process.stderr.write('[context-slice] no --context given; emitting empty slice\n');
    process.exit(0);
    return;
  }

  const result = sliceContext(args.context, args.spec, args.maxLines);

  if (result.truncated) {
    process.stderr.write(
      `[context-slice] WARN: relevant glossary slice is ${result.blockCount} blocks ` +
      `and exceeds the ${resolveMaxLines()}-line cap (MUSTARD_GLOSSARY_MAX_LINES). ` +
      `Truncated. Narrow the spec's scope or raise the cap if every block is needed.\n`
    );
  }

  if (result.slice) {
    process.stdout.write(result.slice + '\n');
  }
  process.exit(0);
}

if (require.main === module) {
  try { main(); } catch (err) {
    // Fail-graceful: a script bug must never break a dispatch.
    process.stderr.write(`[context-slice] Error: ${err && err.message}\n`);
    process.exit(0);
  }
}

module.exports = {
  sliceContext,
  extractRelevanceTerms,
  parseTermBlocks,
  resolveContextFiles,
};
