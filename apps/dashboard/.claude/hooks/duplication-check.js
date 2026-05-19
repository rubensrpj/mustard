#!/usr/bin/env node
/**
 * DUPLICATION-CHECK: PostToolUse hook that warns when a newly written symbol
 * closely resembles an existing symbol in the entity-registry.
 *
 * Matcher: PostToolUse Write|Edit on code files (.ts, .js, .tsx, .jsx, .cs, .py, .go, .java)
 *
 * Heuristic: Levenshtein distance >= 0.85 OR name contains existing name as substring.
 * Shallow heuristic — false positives expected (e.g. UserService vs UserController
 * score ~0.7 → below threshold). Default warn, not strict.
 *
 * Env:
 *   MUSTARD_DUPLICATION_MODE=warn|strict|off  (default: warn)
 *
 * @version 1.0.0
 */

'use strict';

const fs = require('fs');
const path = require('path');

let emit;
try { emit = require('./_lib/harness-event.js').emit; } catch (_) { emit = () => false; }

let shouldRun;
try { shouldRun = require('./_lib/hook-env.js').shouldRun; } catch (_) { shouldRun = () => true; }

let emitMetric = () => {};
try { emitMetric = require('./_lib/metrics-emit.js').emitMetric; } catch (_) {}

const HOOK_NAME = 'duplication-check';

const CODE_EXTS = new Set(['.ts', '.js', '.tsx', '.jsx', '.cs', '.py', '.go', '.java']);

// ── Symbol extraction ─────────────────────────────────────────────────────────

/**
 * Extract top-level symbol names from source content.
 * Regexes are intentionally simple: class/function/const/interface/type declarations.
 */
function extractSymbols(content, ext) {
  const symbols = new Set();
  if (!content || typeof content !== 'string') return symbols;

  // Patterns shared across most languages
  const patterns = [
    /\bclass\s+([A-Za-z_$][A-Za-z0-9_$]*)/g,
    /\binterface\s+([A-Za-z_$][A-Za-z0-9_$]*)/g,
    /\btype\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=/g,
    /\bfunction\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*\(/g,
    /\bconst\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=/g,
    /\bexport\s+(?:default\s+)?(?:class|function|const|interface|type)\s+([A-Za-z_$][A-Za-z0-9_$]*)/g,
  ];

  // C# / Java additions
  if (ext === '.cs' || ext === '.java') {
    patterns.push(
      /\b(?:public|private|protected|internal|static)\s+(?:class|interface|enum|record|struct)\s+([A-Za-z_$][A-Za-z0-9_$]*)/g
    );
  }

  // Go
  if (ext === '.go') {
    patterns.push(
      /\bfunc\s+(?:\([^)]+\)\s+)?([A-Za-z_$][A-Za-z0-9_$]*)\s*\(/g,
      /\btype\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+(?:struct|interface)/g
    );
  }

  // Python
  if (ext === '.py') {
    patterns.push(
      /^\s*(?:class|def|async\s+def)\s+([A-Za-z_][A-Za-z0-9_]*)/gm
    );
  }

  for (const re of patterns) {
    let m;
    while ((m = re.exec(content)) !== null) {
      const sym = m[1];
      if (sym && sym.length >= 3) symbols.add(sym);
    }
  }

  return symbols;
}

// ── Levenshtein similarity ────────────────────────────────────────────────────

/**
 * Compute normalised Levenshtein similarity (0.0–1.0).
 * 1.0 = identical, 0.0 = completely different.
 * Uses simple DP — O(n*m) but names are short.
 */
function similarity(a, b) {
  const la = a.length;
  const lb = b.length;
  if (la === 0 && lb === 0) return 1;
  if (la === 0 || lb === 0) return 0;

  // Normalise case for comparison
  const sa = a.toLowerCase();
  const sb = b.toLowerCase();

  if (sa === sb) return 1;

  const row = new Array(lb + 1);
  for (let j = 0; j <= lb; j++) row[j] = j;

  for (let i = 1; i <= la; i++) {
    let prev = i;
    for (let j = 1; j <= lb; j++) {
      const cost = sa[i - 1] === sb[j - 1] ? 0 : 1;
      const next = Math.min(row[j] + 1, prev + 1, row[j - 1] + cost);
      row[j - 1] = prev;
      prev = next;
    }
    row[lb] = prev;
  }

  const dist = row[lb];
  return 1 - dist / Math.max(la, lb);
}

// ── Entity registry ───────────────────────────────────────────────────────────

/**
 * Read entity-registry.json and extract all known symbol names with their paths.
 * Returns Array<{ name: string, file?: string }>.
 */
function loadRegistrySymbols(cwd) {
  try {
    const registryPath = path.join(cwd, '.claude', 'entity-registry.json');
    if (!fs.existsSync(registryPath)) return null;
    const raw = fs.readFileSync(registryPath, 'utf8');
    const reg = JSON.parse(raw);

    const symbols = [];
    // Walk all keys that look like entity entries (not metadata keys starting with _)
    function walk(obj, filePath) {
      if (!obj || typeof obj !== 'object') return;
      for (const [key, val] of Object.entries(obj)) {
        if (key.startsWith('_')) continue;
        if (typeof val === 'object' && val !== null) {
          // Might be { file, refs, subs, ... }
          const name = val.name || key;
          const file = val.file || filePath || null;
          if (typeof name === 'string' && name.length >= 3) {
            symbols.push({ name, file });
          }
          // Recurse into subs/refs if they're objects
          if (val.subs && typeof val.subs === 'object') walk(val.subs, file);
        } else if (typeof val === 'string') {
          // key=name, val=file
          if (key.length >= 3) symbols.push({ name: key, file: val });
        }
      }
    }

    walk(reg, null);
    return symbols;
  } catch (_) {
    return null; // fail-open
  }
}

// ── Main logic ────────────────────────────────────────────────────────────────

const SIMILARITY_THRESHOLD = 0.85;

/**
 * For each new symbol, find the top-3 closest registry entries.
 * Returns Array<{ newSym, matches: [{name, file, score, reason}] }>
 */
function findDuplicates(newSymbols, registrySymbols, currentFilePath) {
  const dupes = [];
  const normalizedCurrentFile = currentFilePath ? currentFilePath.replace(/\\/g, '/') : null;

  for (const sym of newSymbols) {
    const matches = [];
    for (const reg of registrySymbols) {
      // Skip if the same file (we're editing the same file — no self-match)
      if (normalizedCurrentFile && reg.file) {
        const normalizedRegFile = reg.file.replace(/\\/g, '/');
        if (normalizedRegFile === normalizedCurrentFile) continue;
      }

      const score = similarity(sym, reg.name);
      let reason = null;

      if (score >= SIMILARITY_THRESHOLD) {
        reason = `similarity ${(score * 100).toFixed(0)}%`;
      } else {
        // Check substring containment (case-insensitive)
        const sl = sym.toLowerCase();
        const rl = reg.name.toLowerCase();
        if (sl.length >= 4 && (sl.includes(rl) || rl.includes(sl))) {
          reason = 'name contains existing symbol';
        }
      }

      if (reason) {
        matches.push({ name: reg.name, file: reg.file, score, reason });
      }
    }

    if (matches.length > 0) {
      // Sort by score desc, take top 3
      matches.sort((a, b) => b.score - a.score);
      dupes.push({ newSym: sym, matches: matches.slice(0, 3) });
    }
  }

  return dupes;
}

// ── Stdin → process ───────────────────────────────────────────────────────────

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun(HOOK_NAME)) process.exit(0);
  } catch (_) {}

  const mode = (process.env.MUSTARD_DUPLICATION_MODE || 'warn').toLowerCase();
  if (mode === 'off') process.exit(0);

  let data;
  try {
    data = JSON.parse(input);
  } catch (_) {
    process.exit(0); // fail-open
  }

  try {
    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || toolInput.path || '';
    const ext = path.extname(filePath).toLowerCase();

    // Only trigger on code files
    if (!CODE_EXTS.has(ext)) process.exit(0);

    // Extract content from Write (content) or Edit (new_string)
    const content = typeof toolInput.content === 'string' ? toolInput.content
      : typeof toolInput.new_string === 'string' ? toolInput.new_string
      : null;

    if (!content) process.exit(0);

    const cwd = data.cwd || process.cwd();
    const registrySymbols = loadRegistrySymbols(cwd);

    if (!registrySymbols) {
      // Registry missing → fail-open silently
      process.exit(0);
    }

    const newSymbols = extractSymbols(content, ext);
    if (newSymbols.size === 0) process.exit(0);

    const dupes = findDuplicates(newSymbols, registrySymbols, filePath);
    if (dupes.length === 0) process.exit(0);

    // Format warning message
    const lines = dupes.map(d => {
      const top = d.matches.map(m => `${m.name}${m.file ? ` (${path.basename(m.file)})` : ''} [${m.reason}]`).join(', ');
      return `  "${d.newSym}" → ${top}`;
    });

    const reason = `[duplication-check] Possible duplicate symbols detected:\n${lines.join('\n')}\nReview if a similar abstraction already exists.`;

    // Emit harness event
    try {
      emit('duplication.warn', {
        file: filePath,
        symbols: dupes.map(d => d.newSym),
        matches: dupes.flatMap(d => d.matches.map(m => m.name)),
      }, { cwd, hookInput: data });
    } catch (_) {}

    try {
      emitMetric('duplication-check', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: mode === 'strict' ? 'blocked' : 'warned',
        extras: { symbols: dupes.length, file: path.basename(filePath) },
        cwd,
      });
    } catch (_) {}

    if (mode === 'strict') {
      // block
      process.stdout.write(JSON.stringify({
        decision: 'block',
        reason,
      }) + '\n');
      process.exit(0);
    }

    // warn (default): emit advisory to stderr
    process.stderr.write(reason + '\n');
    process.exit(0);

  } catch (err) {
    // Bug in hook → fail-open
    process.stderr.write(`[duplication-check] Hook error (fail-open): ${err.message}\n`);
    process.exit(0);
  }
});
