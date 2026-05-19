#!/usr/bin/env bun
'use strict';
/**
 * PR-DETECT: PostToolUse(Bash) hook that emits DORA events when GitHub PRs
 * are opened or merged via `gh pr ...` commands.
 *
 * Events emitted (via harness-event.js):
 *   - pr.opened   — when `gh pr create ...` succeeds (exit 0)
 *   - pr.merged   — when `gh pr merge ...` succeeds (exit 0)
 *
 * Payload includes:
 *   { branch, spec, command }
 *
 * Branch comes from `git rev-parse --abbrev-ref HEAD`. Spec is inferred from
 * the most recently modified `.pipeline-states/*.json` file (best effort).
 *
 * Fail-open: exits 0 on any error.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const { shouldRun } = require('./_lib/hook-env.js');
const { emit } = require('./_lib/harness-event.js');

function detectBranch(cwd) {
  try {
    const out = execSync('git rev-parse --abbrev-ref HEAD', {
      cwd, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'], timeout: 2000,
    }).trim();
    return out || null;
  } catch (_) { return null; }
}

function detectMostRecentSpec(cwd) {
  try {
    const dir = path.join(cwd, '.claude', '.pipeline-states');
    if (!fs.existsSync(dir)) return null;
    const files = fs.readdirSync(dir).filter(f => f.endsWith('.json') && !f.endsWith('.metrics.json'));
    if (files.length === 0) return null;
    let best = null;
    let bestMtime = 0;
    for (const f of files) {
      try {
        const stat = fs.statSync(path.join(dir, f));
        if (stat.mtimeMs > bestMtime) { best = f; bestMtime = stat.mtimeMs; }
      } catch (_) { /* skip */ }
    }
    return best ? best.replace(/\.json$/, '') : null;
  } catch (_) { return null; }
}

function classify(command) {
  if (typeof command !== 'string') return null;
  // Conservative match: `gh pr create` / `gh pr merge` at start of token sequence.
  // Tolerate leading "rtk" wrapper.
  const cleaned = command.trim().replace(/^rtk\s+/i, '');
  if (/^gh\s+pr\s+create\b/i.test(cleaned)) return 'pr.opened';
  if (/^gh\s+pr\s+merge\b/i.test(cleaned)) return 'pr.merged';
  return null;
}

let stdinBuf = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', c => stdinBuf += c);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('pr-detect')) { process.exit(0); }

    const data = JSON.parse(stdinBuf || '{}');
    if ((data.tool_name || '') !== 'Bash') { process.exit(0); }

    const command = (data.tool_input && data.tool_input.command) || '';
    const event = classify(command);
    if (!event) { process.exit(0); }

    // Only emit on success — exit code is in tool_response.exit_code on most platforms,
    // but PostToolUse may not always populate it. Be permissive: emit if no explicit failure signal.
    const resp = data.tool_response;
    if (resp && typeof resp === 'object' && Number.isFinite(resp.exit_code) && resp.exit_code !== 0) {
      process.exit(0);
    }

    const cwd = data.cwd || process.cwd();
    const branch = detectBranch(cwd);
    const spec = detectMostRecentSpec(cwd);

    emit(event, {
      branch: branch || null,
      spec: spec || null,
      command: command.length > 200 ? command.slice(0, 200) + '...' : command,
    }, {
      cwd,
      sessionId: data.session_id || data.sessionId || null,
      actor: { kind: 'hook', id: 'pr-detect' },
    });

    process.exit(0);
  } catch (err) {
    process.stderr.write('[pr-detect] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
