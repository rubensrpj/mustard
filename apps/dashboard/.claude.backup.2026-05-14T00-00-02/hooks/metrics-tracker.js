#!/usr/bin/env bun
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

    // ── Scope-aware attribution (Onda 2.2) ───────────────────────────────────
    // - Active specs:        use the most recently modified one as before.
    // - closed-followup specs: only attribute when the tool_input.file_path is
    //                         in spec.affectedFiles. This lets post-feature
    //                         fixes (no command) still link metrics to the
    //                         spec that owns those files.
    const toolInput = data.tool_input || {};
    const touchedPath = String(toolInput.file_path || toolInput.path || toolInput.notebook_path || '');
    const normalizedTouched = touchedPath ? touchedPath.replace(/\\/g, '/') : '';

    let activeNewest = null, activeMtime = 0;
    let followupNewest = null, followupMtime = 0;

    for (const f of files) {
      try {
        const fp = path.join(statesDir, f);
        const stat = fs.statSync(fp);
        const parsed = JSON.parse(fs.readFileSync(fp, 'utf8'));
        const status = String(parsed.status || '').toLowerCase();
        if (status === 'closed-followup') {
          const affected = Array.isArray(parsed.affectedFiles) ? parsed.affectedFiles : [];
          if (!normalizedTouched || affected.length === 0) continue;
          const hit = affected.some(rel => {
            if (!rel) return false;
            const normRel = String(rel).replace(/\\/g, '/');
            return normalizedTouched === normRel || normalizedTouched.endsWith('/' + normRel);
          });
          if (!hit) continue;
          if (stat.mtimeMs > followupMtime) { followupMtime = stat.mtimeMs; followupNewest = { fp, parsed }; }
        } else {
          if (stat.mtimeMs > activeMtime) { activeMtime = stat.mtimeMs; activeNewest = { fp, parsed }; }
        }
      } catch {}
    }

    const chosen = activeNewest || followupNewest;
    if (!chosen) { process.exit(0); }

    const pipelineState = chosen.parsed || {};

    const currentPhase = pipelineState.phaseName || pipelineState.phase || '';

    // Retry detection moved to subagent-tracker → emits `dispatch.failure` based
    // on `tool_response.is_error === true`. Keyword scans on tool_input were
    // 80% false positives (any "fix typo" Edit counted as a retry).

    // ── Wave 4: emit tool.use heartbeat to harness log (no sidecar written) ─
    // All metrics are now derived from the log by buildPipelineState().
    // tool.use events carry enough signal: tool name, phase, retry flag,
    // spec (from pipelineState), so consumers can aggregate from the log.
    const wave = typeof pipelineState.currentWave === 'number'
      ? pipelineState.currentWave
      : (harnessGetWave ? harnessGetWave(data) : 0);

    // Capture salient fields from tool_input so the Mustard Dashboard (standalone) can show *what*
    // is being done, not just *which tool*. Sizes capped to keep events lean.
    const target = {};
    if (toolInput.file_path) target.file = String(toolInput.file_path).slice(-80);
    if (toolInput.command) target.command = String(toolInput.command).slice(0, 120);
    if (toolInput.pattern) target.pattern = String(toolInput.pattern).slice(0, 80);
    if (toolInput.description) target.description = String(toolInput.description).slice(0, 100);
    if (toolInput.subagent_type) target.subagent = String(toolInput.subagent_type);
    if (toolInput.notebook_path) target.file = String(toolInput.notebook_path).slice(-80);
    if (toolInput.url) target.url = String(toolInput.url).slice(0, 120);

    emitEvent('tool.use', {
      tool: toolName,
      phase: currentPhase || null,
      target: Object.keys(target).length ? target : undefined,
    }, {
      cwd,
      sessionId: harnessGetSessionId ? harnessGetSessionId(data) : null,
      wave,
      spec: pipelineState.specName || pipelineState.spec || pipelineState.name || null,
      actor: { kind: 'hook', id: 'metrics-tracker' },
    });

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[metrics-tracker] Error: ${err.message}\n`);
    process.exit(0);
  }
});
