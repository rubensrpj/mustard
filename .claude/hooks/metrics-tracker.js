#!/usr/bin/env node
'use strict';
/**
 * METRICS-TRACKER: PostToolUse hook that tracks pipeline metrics
 *
 * Increments counters in the active pipeline state file:
 * - apiCalls: total tool invocations
 * - toolBreakdown: { Bash: N, Write: N, Edit: N, Task: N }
 * - retries: incremented when tool_input contains retry/fix patterns
 * - gate_saves: spec edits made while phase=PLAN after first /approve
 * - wave_reentry: transitions from EXECUTE back to PLAN
 * - skillHits: per-agent { loaded: N, read: M } skill hit tracking
 *
 * @version 2.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

// ── Harness event bus (Wave 2 dual emission) ─────────────────────────────────
let harnessEmit = null;
let harnessGetSessionId = null;
let harnessGetWave = null;
try {
  const he = require('./_lib/harness-event.js');
  harnessEmit = he.emit;
  harnessGetSessionId = he.getCurrentSessionId;
  harnessGetWave = he.getCurrentWave;
} catch (_) {} // fail-open: harness optional

function emitEvent(eventName, payload, ctx) {
  try {
    if (harnessEmit) harnessEmit(eventName, payload, ctx);
  } catch (_) {} // fail-open
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('metrics-tracker')) { process.exit(0); }
    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();
    const toolName = data.tool_name || '';

    // Find active pipeline state
    const statesDir = path.join(cwd, '.claude', '.pipeline-states');
    if (!fs.existsSync(statesDir)) { process.exit(0); }

    const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json') && !f.endsWith('.metrics.json'));
    if (files.length === 0) { process.exit(0); }

    // Update the most recently modified pipeline state
    let newest = null;
    let newestMtime = 0;
    for (const f of files) {
      try {
        const fp = path.join(statesDir, f);
        const stat = fs.statSync(fp);
        if (stat.mtimeMs > newestMtime) {
          newestMtime = stat.mtimeMs;
          newest = fp;
        }
      } catch {}
    }

    if (!newest) { process.exit(0); }

    // Read pipeline-state.json to derive currentPhase, status, startedAt.
    let pipelineState = {};
    try {
      pipelineState = JSON.parse(fs.readFileSync(newest, 'utf8'));
    } catch {}

    const currentPhase = pipelineState.phaseName || pipelineState.phase || '';

    // Detect retry patterns (included as payload in tool.use event)
    const toolInput = data.tool_input || {};
    const content = JSON.stringify(toolInput).toLowerCase();
    const isRetry = /\b(retry|fix|error|failed|again)\b/.test(content);

    // ── Wave 4: emit tool.use heartbeat to harness log (no sidecar written) ─
    // All metrics are now derived from the log by buildPipelineState().
    // tool.use events carry enough signal: tool name, phase, retry flag,
    // spec (from pipelineState), so consumers can aggregate from the log.
    emitEvent('tool.use', {
      tool: toolName,
      phase: currentPhase || null,
      retry: isRetry || undefined,
      bytesIn: null,
      bytesOut: null,
    }, {
      cwd,
      sessionId: harnessGetSessionId ? harnessGetSessionId(data) : null,
      wave: harnessGetWave ? harnessGetWave(data) : 0,
      spec: pipelineState.spec || pipelineState.name || null,
      actor: { kind: 'hook', id: 'metrics-tracker' },
    });

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[metrics-tracker] Error: ${err.message}\n`);
    process.exit(0);
  }
});
