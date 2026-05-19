'use strict';
/**
 * description-enricher — extracts doc-comment descriptions for registry entities.
 *
 * Stack-agnostic: scans the entity's `ref` file (or first ref file when array)
 * for the doc-comment block immediately preceding the entity declaration.
 * Sets `entry.description` (truncated to ~200 chars, single line).
 *
 * Supported doc-comment styles (recognised by leading marker):
 *   /\** ... *\/    — JSDoc, JavaDoc, C-style
 *   /// ...         — Rust, C# triple-slash
 *   //! ...         — Rust outer
 *   // ...          — line comments (consecutive)
 *   # ...           — Python, Ruby, shell (consecutive)
 *   """ ... """     — Python docstring (immediately INSIDE def/class — best effort)
 *
 * Identification heuristic: the entity is "found" in the file when the line
 * matches `(class|interface|struct|enum|type|def|function|const)\s+EntityName\b`
 * OR a known table/schema declaration like `pgTable('xs', ...)` / `Table.X`.
 *
 * Fail-open: any error returns null and the entry is left unchanged.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

const MAX_LEN = 200;
const MAX_SCAN_LINES = 10000; // skip very large files

/**
 * Extract a doc-comment description for `entityName` from `filePath`.
 * Returns a single-line string ≤ MAX_LEN, or null if not found.
 */
function extractDescription(filePath, entityName) {
  if (!filePath || !entityName) return null;
  let raw;
  try { raw = fs.readFileSync(filePath, 'utf8'); }
  catch (_) { return null; }
  const lines = raw.split('\n');
  if (lines.length > MAX_SCAN_LINES) return null;

  // Build a regex matching common entity-declaration patterns
  const escName = entityName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const declRe = new RegExp(
    '(?:^|[\\s\\W])(?:class|interface|struct|enum|type|def|function|fn|const|let|var|public\\s+(?:partial\\s+)?(?:class|interface)|export\\s+(?:default\\s+)?(?:class|interface|function|const|type|enum))\\s+' +
    escName + '\\b'
  );
  const tableRe = new RegExp(
    "(?:pgTable|sqliteTable|mysqlTable|Table|@Entity)\\s*\\(\\s*['\"]?" + escName + "['\"]?",
    'i'
  );
  const pluralLowerRe = new RegExp(
    "(?:pgTable|sqliteTable|mysqlTable|Table|@Entity)\\s*\\(\\s*['\"]?" +
    escName.toLowerCase() + 's?[\"\']',
    'i'
  );

  let declLine = -1;
  for (let i = 0; i < lines.length; i++) {
    if (declRe.test(lines[i]) || tableRe.test(lines[i]) || pluralLowerRe.test(lines[i])) {
      declLine = i;
      break;
    }
  }
  if (declLine === -1) return null;

  // Walk backward collecting the immediately-preceding doc-comment block.
  // Skip blank lines between block and decl (allow up to 1).
  let i = declLine - 1;
  while (i >= 0 && lines[i].trim() === '') i--;
  if (i < 0) return null;

  const collected = [];
  // /** ... */ block
  if (/\*\//.test(lines[i])) {
    let j = i;
    while (j >= 0) {
      collected.unshift(lines[j]);
      if (/\/\*\*?/.test(lines[j])) break;
      j--;
    }
    return cleanDocBlock(collected.join('\n'), 'jsdoc');
  }
  // /// or //! consecutive
  if (/^\s*\/\/[\/!]/.test(lines[i])) {
    let j = i;
    while (j >= 0 && /^\s*\/\/[\/!]/.test(lines[j])) {
      collected.unshift(lines[j]);
      j--;
    }
    return cleanDocBlock(collected.join('\n'), 'triple-slash');
  }
  // // consecutive line comments
  if (/^\s*\/\//.test(lines[i])) {
    let j = i;
    while (j >= 0 && /^\s*\/\//.test(lines[j]) && !/^\s*\/\/[\/!]/.test(lines[j])) {
      collected.unshift(lines[j]);
      j--;
    }
    return cleanDocBlock(collected.join('\n'), 'line');
  }
  // # consecutive comments (python/ruby/shell)
  if (/^\s*#/.test(lines[i]) && !/^\s*#!/.test(lines[i])) {
    let j = i;
    while (j >= 0 && /^\s*#/.test(lines[j]) && !/^\s*#!/.test(lines[j])) {
      collected.unshift(lines[j]);
      j--;
    }
    return cleanDocBlock(collected.join('\n'), 'hash');
  }
  // Python docstring inside class/def body — check next-line approach
  // (not common pattern for "above" scan; skip for now)

  return null;
}

/** Strip comment markers + collapse whitespace into single line, truncate. */
function cleanDocBlock(text, kind) {
  let s = text;
  if (kind === 'jsdoc') {
    s = s
      .replace(/\/\*\*?/g, '')
      .replace(/\*\//g, '')
      .replace(/^\s*\*\s?/gm, '')
      // Drop common JSDoc tags (@param @returns @example @since @version etc)
      .replace(/^\s*@\w+.*$/gm, '');
  } else if (kind === 'triple-slash') {
    s = s.replace(/^\s*\/\/[\/!]\s?/gm, '');
  } else if (kind === 'line') {
    s = s.replace(/^\s*\/\/\s?/gm, '');
  } else if (kind === 'hash') {
    s = s.replace(/^\s*#\s?/gm, '');
  }
  s = s.replace(/\s+/g, ' ').trim();
  if (!s) return null;
  if (s.length > MAX_LEN) s = s.slice(0, MAX_LEN - 1) + '…';
  return s;
}

/**
 * Walk `registry.e` and add `description` where extractable.
 * `registry.e[entityName].refs[0]` is treated as the canonical file.
 *
 * @param {object} registry
 * @param {string} projectRoot
 * @returns {{ enriched: number, scanned: number }}
 */
function enrichDescriptions(registry, projectRoot) {
  let enriched = 0;
  let scanned = 0;
  if (!registry || typeof registry.e !== 'object') return { enriched, scanned };

  for (const [name, entry] of Object.entries(registry.e)) {
    if (!entry || typeof entry !== 'object') continue;
    if (entry.description) continue; // already set (e.g., manually) — keep
    const refs = Array.isArray(entry.refs) ? entry.refs : [];
    if (refs.length === 0) continue;
    const refPath = typeof refs[0] === 'string'
      ? refs[0]
      : (refs[0] && typeof refs[0].path === 'string' ? refs[0].path : null);
    if (!refPath) continue;
    const absPath = path.isAbsolute(refPath) ? refPath : path.join(projectRoot, refPath);
    scanned++;
    const desc = extractDescription(absPath, name);
    if (desc) {
      entry.description = desc;
      enriched++;
    }
  }
  return { enriched, scanned };
}

module.exports = { enrichDescriptions, extractDescription };
