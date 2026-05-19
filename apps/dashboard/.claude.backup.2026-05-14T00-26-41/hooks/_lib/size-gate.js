'use strict';
/**
 * size-gate — shared helper for size-based PreToolUse gates (specs, skills, etc.).
 *
 * Encapsulates the pattern shared by `spec-size-gate.js` and `skill-size-gate.js`:
 *   - Mode resolution from env var (off | warn | strict).
 *   - Path predicate to filter which files this gate inspects.
 *   - Simulate Edit to compute post-edit content.
 *   - Three-tier line-count check (warn / strict-warn / block).
 *   - Strict mode → deny via `permissionDecision: 'deny'`.
 *   - Warn/below-threshold → allow (or exit 0 if no decision needed).
 *
 * Usage (in a hook):
 *
 *   require('./_lib/size-gate.js').run({
 *     name: 'spec-size-gate',
 *     envVar: 'MUSTARD_SPEC_SIZE_MODE',
 *     defaultMode: 'warn',
 *     isTargetPath: (filePath) => /\/spec\/.+\.md$/.test(filePath.replace(/\\/g, '/')),
 *     thresholds: { warn: 200, strictWarn: 400, block: 500 },
 *     blockReason: (lines) => `[spec-size-gate] spec exceeds 500 lines (${lines}) — split into references/`,
 *     // optional: skip-generated handling
 *     skipWhen: (content, mode) => mode === 'warn' && content.startsWith('<!-- mustard:generated -->'),
 *   });
 */

const fs = require('fs');

function resolveMode(envVar, defaultMode) {
  const raw = (process.env[envVar] || defaultMode || 'warn').toLowerCase();
  if (raw === 'off' || raw === 'warn' || raw === 'strict') return raw;
  return defaultMode || 'warn';
}

function simulateEdit(toolInput) {
  const filePath = toolInput.file_path;
  if (!filePath) return null;
  let current;
  try { current = fs.readFileSync(filePath, 'utf8'); }
  catch (_) { current = ''; }
  const oldStr = toolInput.old_string || '';
  const newStr = toolInput.new_string || '';
  if (toolInput.replace_all) {
    return current.split(oldStr).join(newStr);
  }
  const idx = current.indexOf(oldStr);
  if (idx === -1) return current;
  return current.slice(0, idx) + newStr + current.slice(idx + oldStr.length);
}

function countLines(content) {
  if (!content) return 0;
  return content.split('\n').length;
}

function deny(reason) {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'deny',
      permissionDecisionReason: reason,
    },
  }) + '\n');
}

function allow() {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'allow',
    },
  }) + '\n');
}

/**
 * Drive a size gate. Reads stdin once, decides, writes stdout (when needed),
 * and exits with code 0 (fail-open). Accepts:
 *
 * @param {Object} opts
 * @param {string} opts.name             logging prefix, e.g. 'spec-size-gate'
 * @param {string} opts.envVar           env var controlling mode
 * @param {'off'|'warn'|'strict'} [opts.defaultMode='warn']
 * @param {(p: string) => boolean} opts.isTargetPath
 * @param {{warn: number, strictWarn: number, block: number}} opts.thresholds
 * @param {(lines: number) => string} opts.blockReason
 * @param {(content: string, mode: string) => boolean} [opts.skipWhen]  optional pre-check
 */
function run(opts) {
  const {
    name,
    envVar,
    defaultMode = 'warn',
    isTargetPath,
    thresholds,
    blockReason,
    skipWhen,
    onDecision,
  } = opts || {};

  let input = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', c => input += c);
  process.stdin.on('end', () => {
    try {
      const mode = resolveMode(envVar, defaultMode);
      if (mode === 'off') { process.exit(0); }

      let data;
      try { data = JSON.parse(input); }
      catch (_) { process.exit(0); }

      const toolInput = (data && data.tool_input) || {};
      const filePath = toolInput.file_path || '';
      if (!isTargetPath(filePath)) { process.exit(0); }

      const toolName = (data && data.tool_name) || '';
      let content;
      if (toolName === 'Write') {
        content = toolInput.content || '';
      } else if (toolName === 'Edit') {
        content = simulateEdit(toolInput);
        if (content === null) { process.exit(0); }
      } else {
        process.exit(0);
      }

      if (typeof skipWhen === 'function' && skipWhen(content, mode)) {
        process.exit(0);
      }

      const lines = countLines(content);

      if (lines >= thresholds.block) {
        const msg = blockReason(lines);
        process.stderr.write(msg + '\n');
        if (typeof onDecision === 'function') {
          try { onDecision({ lines, mode, decision: mode === 'strict' ? 'blocked' : 'over-size', thresholds, filePath }); } catch (_) {}
        }
        if (mode === 'strict') { deny(msg); process.exit(0); }
        allow();
        process.exit(0);
      }
      if (lines >= thresholds.strictWarn) {
        process.stderr.write(`[${name}] WARN: ${lines} lines (strict-warn threshold ${thresholds.strictWarn})\n`);
      } else if (lines >= thresholds.warn) {
        process.stderr.write(`[${name}] ADVISORY: ${lines} lines (warn threshold ${thresholds.warn})\n`);
      }

      process.exit(0);
    } catch (err) {
      process.stderr.write(`[${name}] Error (fail-open): ${err.message}\n`);
      process.exit(0);
    }
  });
}

module.exports = { run, resolveMode, simulateEdit, countLines, deny, allow };
