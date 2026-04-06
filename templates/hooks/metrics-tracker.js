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

    // Read pipeline-state.json READ-ONLY (to derive currentPhase, status, startedAt).
    // Never write to it — metrics live in a sidecar to avoid "file modified since
    // read" races with Edit/Write on the pipeline-state file.
    let pipelineState = {};
    try {
      pipelineState = JSON.parse(fs.readFileSync(newest, 'utf8'));
    } catch {}

    const sidecarPath = newest.replace(/\.json$/, '.metrics.json');
    let sidecar;
    if (fs.existsSync(sidecarPath)) {
      try {
        sidecar = JSON.parse(fs.readFileSync(sidecarPath, 'utf8'));
      } catch {
        sidecar = null;
      }
    }
    if (!sidecar || typeof sidecar !== 'object') {
      sidecar = {
        v: 1,
        metrics: {
          apiCalls: 0,
          toolBreakdown: {},
          retries: 0,
          startedAt: pipelineState.startedAt || new Date().toISOString(),
        },
        previousPhase: '',
      };
    }
    if (!sidecar.metrics) {
      sidecar.metrics = {
        apiCalls: 0,
        toolBreakdown: {},
        retries: 0,
        startedAt: pipelineState.startedAt || new Date().toISOString(),
      };
    }
    if (!sidecar.metrics.toolBreakdown) sidecar.metrics.toolBreakdown = {};

    // Alias for minimal churn below — all mutations go to the sidecar.
    const state = sidecar;

    // ── wave_reentry: track EXECUTE → PLAN transitions ──────────────────────
    // previousPhase is updated on every write so we can detect phase changes.
    const currentPhase = pipelineState.phaseName || pipelineState.phase || '';
    const previousPhase = sidecar.previousPhase || '';
    if (currentPhase === 'PLAN' && previousPhase === 'EXECUTE') {
      state.metrics.wave_reentry = (state.metrics.wave_reentry || 0) + 1;
    }
    // Always update previousPhase to the current phase so the NEXT write can
    // detect a transition.
    sidecar.previousPhase = currentPhase;

    // ── gate_saves: spec edits in PLAN phase after first /approve ────────────
    // Proxy for "first approve recorded": pipelineState.status === 'approved'
    // (set by /approve command).  A spec file is any .md in .claude/spec/ or
    // matching *spec*.md anywhere in the pipeline-states dir.
    if ((toolName === 'Edit' || toolName === 'Write') && currentPhase === 'PLAN' && pipelineState.status === 'approved') {
      const toolFilePath = (data.tool_input || {}).file_path || (data.tool_input || {}).path || '';
      const isSpecFile =
        /[/\\]\.claude[/\\]spec[/\\]/.test(toolFilePath) ||
        /spec.*\.md$/i.test(toolFilePath) ||
        (/\.pipeline-states[/\\]/.test(toolFilePath) && toolFilePath.endsWith('.md'));
      if (isSpecFile) {
        state.metrics.gate_saves = (state.metrics.gate_saves || 0) + 1;
      }
    }

    // ── skill_hit_rate: Read on a skill file → attribute to active subagent ──
    // This is heuristic: we look up the most recent subagent entry in the
    // registry that has no endedAt (i.e. currently active).  We cannot
    // perfectly attribute reads to a specific subagent context when multiple
    // agents run in parallel — we accept this imprecision.
    if (toolName === 'Read') {
      const readPath = (data.tool_input || {}).file_path || (data.tool_input || {}).path || '';
      const isSkillFile =
        /[/\\]skills[/\\][^/\\]+[/\\]SKILL\.md$/i.test(readPath) ||
        /[/\\]\.claude[/\\]skills[/\\][^/\\]+\.md$/i.test(readPath);
      if (isSkillFile) {
        const registryPath = path.join(cwd, '.claude', '.subagent-registry.json');
        try {
          if (fs.existsSync(registryPath)) {
            const registry = JSON.parse(fs.readFileSync(registryPath, 'utf8'));
            // Find the most recently started entry without an endedAt
            let activeEntry = null;
            let latestStart = 0;
            for (const [, entry] of Object.entries(registry)) {
              if (entry.endedAt) continue;
              const t = new Date(entry.startedAt || 0).getTime();
              if (t > latestStart) {
                latestStart = t;
                activeEntry = entry;
              }
            }
            if (activeEntry && activeEntry.agentType && Array.isArray(activeEntry.recommendedSkills)) {
              // Extract skill name from the file path (last directory component before SKILL.md)
              const skillName = path.basename(path.dirname(readPath));
              if (activeEntry.recommendedSkills.includes(skillName)) {
                if (!state.metrics.skillHits) state.metrics.skillHits = {};
                if (!state.metrics.skillHits[activeEntry.agentType]) {
                  state.metrics.skillHits[activeEntry.agentType] = { loaded: 0, read: 0 };
                }
                state.metrics.skillHits[activeEntry.agentType].read++;
              }
            }
          }
        } catch {} // fail-open: skill attribution is advisory
      }
    }

    // Increment counters (skip Read — it's too noisy for general tracking)
    if (toolName !== 'Read') {
      state.metrics.apiCalls++;
      state.metrics.toolBreakdown[toolName] = (state.metrics.toolBreakdown[toolName] || 0) + 1;
    }

    // Detect retry patterns
    const toolInput = data.tool_input || {};
    const content = JSON.stringify(toolInput).toLowerCase();
    if (/\b(retry|fix|error|failed|again)\b/.test(content)) {
      state.metrics.retries++;
      // Per-phase attempt tracking
      if (!state.metrics.agentAttempts) {
        state.metrics.agentAttempts = {};
      }
      var phase = currentPhase || 'unknown';
      state.metrics.agentAttempts[phase] = (state.metrics.agentAttempts[phase] || 0) + 1;
    }

    state.metrics.updatedAt = new Date().toISOString();

    // Write ONLY the sidecar — never touch pipeline-state.json from this hook.
    fs.writeFileSync(sidecarPath, JSON.stringify(sidecar, null, 2), 'utf8');

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[metrics-tracker] Error: ${err.message}\n`);
    process.exit(0);
  }
});
