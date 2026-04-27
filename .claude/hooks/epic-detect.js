#!/usr/bin/env node
'use strict';
/**
 * EPIC-DETECT: PostToolUse hook — detect when an epic is ready to fold.
 *
 * Matcher: Write|Edit on .pipeline-states/*.json
 *
 * After each write to a pipeline-states file, runs detectCompletedEpics.
 * If any epics are ready, emits `epic.ready` event with the list.
 * Does NOT fold automatically — the agent (via /complete or manual) does that.
 *
 * Fail-open: any error → exit 0, never blocks.
 *
 * @version 1.0.0
 */

const path = require('path');
const fs = require('fs');

let shouldRunFn = null;
try {
  const hookEnv = require('./_lib/hook-env.js');
  shouldRunFn = hookEnv.shouldRun.bind(hookEnv);
} catch (_) {}

let harnessEvent = null;
try {
  harnessEvent = require('./_lib/harness-event.js');
} catch (_) {}

let epicFold = null;
try {
  epicFold = require('./../scripts/epic-fold.js');
} catch (_) {}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (shouldRunFn && !shouldRunFn('epic-detect')) {
      process.stdout.write(JSON.stringify({ decision: 'continue' }) + '\n');
      process.exit(0);
    }

    let data = {};
    try { data = input ? JSON.parse(input) : {}; } catch (_) {}

    // Determine the file path that was written
    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || toolInput.path || '';

    // Only react to writes on .pipeline-states/*.json files
    const isPipelineState = filePath &&
      filePath.replace(/\\/g, '/').includes('.pipeline-states/') &&
      filePath.endsWith('.json');

    if (!isPipelineState) {
      process.stdout.write(JSON.stringify({ decision: 'continue' }) + '\n');
      process.exit(0);
    }

    const cwd = data.cwd || process.env.CLAUDE_PROJECT_DIR || process.cwd();

    // Run detectCompletedEpics
    if (!epicFold || typeof epicFold.detectCompletedEpics !== 'function') {
      process.stdout.write(JSON.stringify({ decision: 'continue' }) + '\n');
      process.exit(0);
    }

    const ready = epicFold.detectCompletedEpics({ cwd });

    if (ready && ready.length > 0) {
      // Emit epic.ready event (advisory — no fold)
      if (harnessEvent && typeof harnessEvent.emit === 'function') {
        try {
          harnessEvent.emit('epic.ready', {
            epics: ready,
            detected_after_write: filePath,
          }, {
            cwd,
            actor: { kind: 'hook', id: 'epic-detect' },
          });
        } catch (_) {}
      }

      // Advisory message to agent (non-blocking)
      const hint = ready.length === 1
        ? `[epic-detect] Epic "${ready[0]}" is ready to fold. Run: node .claude/scripts/epic-fold.js --epic ${ready[0]}`
        : `[epic-detect] ${ready.length} epics ready to fold: ${ready.join(', ')}. Run: node .claude/scripts/epic-fold.js --detect`;

      process.stderr.write(hint + '\n');
    }

    process.stdout.write(JSON.stringify({ decision: 'continue' }) + '\n');
    process.exit(0);
  } catch (err) {
    try { process.stderr.write('[epic-detect] warn: ' + err.message + '\n'); } catch (_) {}
    process.stdout.write(JSON.stringify({ decision: 'continue' }) + '\n');
    process.exit(0);
  }
});
