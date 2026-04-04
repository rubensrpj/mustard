#!/usr/bin/env node
/**
 * METRICS-TRACKER: PostToolUse hook that tracks pipeline metrics
 *
 * Increments counters in the active pipeline state file:
 * - apiCalls: total tool invocations
 * - toolBreakdown: { Bash: N, Write: N, Edit: N, Task: N }
 * - retries: incremented when tool_input contains retry/fix patterns
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

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

    const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
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

    const state = JSON.parse(fs.readFileSync(newest, 'utf8'));

    // Initialize metrics if not present
    if (!state.metrics) {
      state.metrics = {
        apiCalls: 0,
        toolBreakdown: {},
        retries: 0,
        startedAt: state.startedAt || new Date().toISOString(),
      };
    }

    // Increment counters
    state.metrics.apiCalls++;
    state.metrics.toolBreakdown[toolName] = (state.metrics.toolBreakdown[toolName] || 0) + 1;

    // Detect retry patterns
    const toolInput = data.tool_input || {};
    const content = JSON.stringify(toolInput).toLowerCase();
    if (/\b(retry|fix|error|failed|again)\b/.test(content)) {
      state.metrics.retries++;
      // Per-phase attempt tracking
      if (!state.metrics.agentAttempts) {
        state.metrics.agentAttempts = {};
      }
      var phase = state.phaseName || state.phase || 'unknown';
      state.metrics.agentAttempts[phase] = (state.metrics.agentAttempts[phase] || 0) + 1;
    }

    state.metrics.updatedAt = new Date().toISOString();

    fs.writeFileSync(newest, JSON.stringify(state, null, 2), 'utf8');

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[metrics-tracker] Error: ${err.message}\n`);
    process.exit(0);
  }
});
