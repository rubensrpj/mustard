#!/usr/bin/env bun
/**
 * CONVENTION-CHECK: PostToolUse hook that warns when a written file violates
 * naming/path conventions recorded in knowledge.json.
 *
 * Matcher: PostToolUse Write|Edit (all files)
 *
 * Heuristic: reads knowledge.json entries with confidence >= 0.8 and
 * type==="convention"|"pattern". Derives simple path rules from phrases like:
 *   "Repository always in /Repositories"  →  name contains "Repository" → path must contain /Repositories/
 *   "Services in /Services"               →  similar
 *
 * If no rule can be derived from an entry, it is silently ignored.
 *
 * Env:
 *   MUSTARD_CONVENTION_MODE=warn|strict|off  (default: warn)
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

const HOOK_NAME = 'convention-check';
const CONFIDENCE_THRESHOLD = 0.8;

// ── Knowledge loading ─────────────────────────────────────────────────────────

function loadKnowledge(cwd) {
  try {
    const kPath = path.join(cwd, '.claude', 'knowledge.json');
    if (!fs.existsSync(kPath)) return null;
    const raw = fs.readFileSync(kPath, 'utf8');
    return JSON.parse(raw);
  } catch (_) {
    return null; // fail-open
  }
}

// ── Rule derivation ───────────────────────────────────────────────────────────

/**
 * Attempt to derive a {keyword, requiredPathSegment} rule from a knowledge entry content string.
 * Patterns tried (case-insensitive):
 *   "{X} always in /{Y}"
 *   "{X} always in {Y}"
 *   "{X} em /{Y}"
 *   "{X} em {Y}"
 *   "{X} sempre em /{Y}"
 *   "{X} in /{Y}"
 *   "{X} in {Y}"
 *
 * Returns { keyword: string, dir: string } or null if no pattern matches.
 */
function deriveRule(content) {
  if (!content || typeof content !== 'string') return null;

  // Patterns: keyword (always|sempre|em) in? /{dir} or {dir}
  // Group 1 = keyword, Group 2 = dir segment
  const patterns = [
    /([A-Za-z_][A-Za-z0-9_]*)\s+(?:always\s+in|sempre\s+em|in|em)\s+\/([A-Za-z_][A-Za-z0-9_/]*)/i,
    /([A-Za-z_][A-Za-z0-9_]*)\s+(?:always\s+in|sempre\s+em|in|em)\s+([A-Za-z_][A-Za-z0-9_/]*)/i,
  ];

  for (const re of patterns) {
    const m = content.match(re);
    if (m) {
      const keyword = m[1].trim();
      const dir = m[2].trim().replace(/\//g, path.sep);
      if (keyword.length >= 2 && dir.length >= 2) {
        return { keyword, dir };
      }
    }
  }

  return null;
}

/**
 * Extract conventions from knowledge.json array or object.
 * Returns Array<{ keyword, dir, content }>.
 */
function extractConventions(knowledge) {
  const rules = [];
  if (!knowledge) return rules;

  const entries = Array.isArray(knowledge) ? knowledge
    : typeof knowledge === 'object' ? Object.values(knowledge)
    : [];

  for (const entry of entries) {
    if (!entry || typeof entry !== 'object') continue;

    // Filter by confidence and type
    const confidence = typeof entry.confidence === 'number' ? entry.confidence : 0;
    if (confidence < CONFIDENCE_THRESHOLD) continue;

    const type = (entry.type || '').toLowerCase();
    if (type !== 'convention' && type !== 'pattern' && type !== 'structure') continue;

    const content = entry.content || entry.description || entry.pattern || '';
    const rule = deriveRule(content);
    if (rule) {
      rules.push({ ...rule, content });
    }
  }

  return rules;
}

// ── Violation check ───────────────────────────────────────────────────────────

/**
 * Check if filePath violates any of the derived conventions.
 * Returns Array<{ rule, filePath }> of violations.
 */
function findViolations(filePath, rules) {
  const violations = [];
  const normalizedFilePath = filePath.replace(/\\/g, '/');
  const basename = path.basename(filePath);

  for (const rule of rules) {
    // Does the basename contain the keyword?
    if (basename.toLowerCase().includes(rule.keyword.toLowerCase())) {
      // Does the path contain the required directory segment?
      const normalizedDir = rule.dir.replace(/\\/g, '/');
      if (!normalizedFilePath.includes('/' + normalizedDir + '/') &&
          !normalizedFilePath.endsWith('/' + normalizedDir)) {
        violations.push({ rule, filePath: normalizedFilePath });
      }
    }
  }

  return violations;
}

// ── Main logic ────────────────────────────────────────────────────────────────

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun(HOOK_NAME)) process.exit(0);
  } catch (_) {}

  const mode = (process.env.MUSTARD_CONVENTION_MODE || 'warn').toLowerCase();
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
    if (!filePath) process.exit(0);

    const cwd = data.cwd || process.cwd();
    const knowledge = loadKnowledge(cwd);

    if (!knowledge) {
      // knowledge.json missing → fail-open
      process.exit(0);
    }

    const rules = extractConventions(knowledge);

    // Log active rule count to stderr (diagnostic, quiet)
    if (rules.length > 0) {
      process.stderr.write(`[convention-check] ${rules.length} active rule(s) derived from knowledge.json\n`);
    } else {
      // No extractable rules → skip silently
      process.exit(0);
    }

    const violations = findViolations(filePath, rules);
    if (violations.length === 0) process.exit(0);

    const lines = violations.map(v =>
      `  "${path.basename(v.filePath)}" should be in /${v.rule.dir}/ (convention: "${v.rule.content}")`
    );

    const reason = `[convention-check] Convention violation(s) detected:\n${lines.join('\n')}\nMove the file to the correct directory or update the convention.`;

    // Emit harness event
    try {
      emit('convention.warn', {
        file: filePath,
        violations: violations.map(v => ({ keyword: v.rule.keyword, expectedDir: v.rule.dir })),
      }, { cwd, hookInput: data });
    } catch (_) {}

    try {
      emitMetric('convention-check', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: mode === 'strict' ? 'blocked' : 'warned',
        extras: { violations: violations.length, file: path.basename(filePath) },
        cwd,
      });
    } catch (_) {}

    if (mode === 'strict') {
      process.stdout.write(JSON.stringify({
        decision: 'block',
        reason,
      }) + '\n');
      process.exit(0);
    }

    // warn (default)
    process.stderr.write(reason + '\n');
    process.exit(0);

  } catch (err) {
    process.stderr.write(`[convention-check] Hook error (fail-open): ${err.message}\n`);
    process.exit(0);
  }
});
