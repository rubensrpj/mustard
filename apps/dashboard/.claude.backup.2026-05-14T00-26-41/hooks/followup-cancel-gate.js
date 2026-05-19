#!/usr/bin/env bun
'use strict';
/**
 * FOLLOWUP-CANCEL-GATE: UserPromptSubmit hook that archives any pending
 * `closed-followup` pipeline-state when the user starts a new pipeline.
 *
 * Triggers archival when the prompt invokes `/mustard:feature`, `/mustard:bugfix`,
 * or `/mustard:task` — signalling that the previous followup window is over and
 * subsequent edits belong to a new context, not to the recently closed spec.
 *
 * Wires `complete-spec.js --archive-followups` (no TTL) so any closed-followup
 * state is moved to `spec/completed/` and its metrics archive is written.
 *
 * Fail-open: exits 0 on any error.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const { shouldRun } = require('./_lib/hook-env.js');
let emitMetric = () => {};
try { emitMetric = require('./_lib/metrics-emit.js').emitMetric; } catch (_) {}

const TRIGGER_RE = /^\s*\/mustard:(feature|bugfix|task)\b/i;

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('followup-cancel-gate')) { process.exit(0); }

    let data = {};
    try { data = JSON.parse(input || '{}'); } catch (_) { process.exit(0); }

    const prompt = String(data.prompt || '');
    if (!TRIGGER_RE.test(prompt)) { process.exit(0); }

    const cwd = data.cwd || process.cwd();
    const script = path.join(cwd, '.claude', 'scripts', 'complete-spec.js');
    if (!fs.existsSync(script)) { process.exit(0); }

    const r = spawnSync(process.execPath, [script, '--archive-followups'], {
      cwd,
      timeout: 5000,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
      windowsHide: true,
    });

    let archived = 0;
    try {
      const parsed = JSON.parse(r.stdout || '{}');
      archived = Number(parsed.archived || 0);
    } catch (_) {}

    if (archived > 0) {
      try {
        emitMetric('followup-cancel-gate', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'archived-' + archived,
          extras: { archived },
          cwd,
        });
      } catch (_) {}
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[followup-cancel-gate] ' + err.message + '\n');
    process.exit(0);
  }
});
