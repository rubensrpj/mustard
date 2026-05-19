#!/usr/bin/env bun
'use strict';
/**
 * PIPELINE-PHASE: PostToolUse hook that emits pipeline.phase events to the
 * harness event bus whenever a pipeline-state file is written/edited.
 *
 * Strategy (Wave 2):
 *   - Registers on PostToolUse matcher: Write|Edit
 *   - When the written file is .claude/.pipeline-states/{spec}.json,
 *     reads the new phase from the file.
 *   - Compares against a phase cache in .claude/.harness/.phase-cache.json
 *     to determine the "from" phase.
 *   - If phase changed (or no prior record), emits pipeline.phase { from, to }.
 *   - Updates the cache with the new phase.
 *
 * Guards:
 *   - Fail-open: any error silently exits 0 — never blocks the tool call.
 *   - Node built-ins only.
 *   - Respects MUSTARD_DISABLED_HOOKS=pipeline-phase.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const { emit, getCurrentSessionId, getCurrentWave } = require('./_lib/harness-event.js');

const PHASE_CACHE_FILE = '.phase-cache.json';

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('pipeline-phase')) { process.exit(0); }

    const data = JSON.parse(input);
    const toolName = data.tool_name || '';

    // Only interested in Write and Edit
    if (toolName !== 'Write' && toolName !== 'Edit') { process.exit(0); }

    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || toolInput.path || '';

    // Only interested in .pipeline-states/{spec}.json files (exclude .metrics.json)
    if (!isPipelineStateFile(filePath)) { process.exit(0); }

    const cwd = data.cwd || process.cwd();

    // Read the updated pipeline state file
    let pipelineState = null;
    try {
      if (fs.existsSync(filePath)) {
        pipelineState = JSON.parse(fs.readFileSync(filePath, 'utf8'));
      }
    } catch (_) { process.exit(0); }

    if (!pipelineState) { process.exit(0); }

    const currentPhase = pipelineState.phaseName || pipelineState.phase || null;
    if (!currentPhase) { process.exit(0); }

    // Derive spec name from filename (e.g. "add-login.json" → "add-login")
    const spec = path.basename(filePath).replace(/\.json$/, '');

    // Read phase cache
    const harnessDir = path.join(cwd, '.claude', '.harness');
    const cacheFile = path.join(harnessDir, PHASE_CACHE_FILE);
    let cache = {};
    try {
      if (fs.existsSync(cacheFile)) {
        cache = JSON.parse(fs.readFileSync(cacheFile, 'utf8'));
      }
    } catch (_) {}

    const previousPhase = cache[spec] || null;

    // Only emit when phase actually changes (or first time we see this spec)
    if (previousPhase === currentPhase) { process.exit(0); }

    // Update cache
    try {
      cache[spec] = currentPhase;
      // Ensure harnessDir exists (harness-init may not have run in tests)
      if (!fs.existsSync(harnessDir)) {
        fs.mkdirSync(harnessDir, { recursive: true });
      }
      fs.writeFileSync(cacheFile, JSON.stringify(cache, null, 2), 'utf8');
    } catch (_) {} // fail-open: cache update is advisory

    // Emit pipeline.phase event
    try {
      const sessionId = getCurrentSessionId(data);
      const wave = getCurrentWave(data);
      emit('pipeline.phase', { from: previousPhase, to: currentPhase }, {
        cwd,
        sessionId,
        wave,
        spec,
        actor: { kind: 'hook', id: 'pipeline-phase' },
      });
    } catch (_) {} // fail-open

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[pipeline-phase] Error: ${err.message}\n`);
    process.exit(0);
  }
});

/**
 * Returns true if the given file path is a pipeline-state file.
 * Matches .claude/.pipeline-states/{anything}.json but NOT .metrics.json.
 */
function isPipelineStateFile(filePath) {
  if (!filePath) return false;
  // Normalize separators for cross-platform matching
  const normalized = filePath.replace(/\\/g, '/');
  return (
    /\/.claude\/\.pipeline-states\/[^/]+\.json$/.test(normalized) &&
    !normalized.endsWith('.metrics.json')
  );
}

/**
 * Valid pipeline phases including Wave 7 COORDINATE (used by specs with children).
 * Kept here as documentation — the hook does not validate phases, only records them.
 * Valid values: ANALYZE, PLAN, EXECUTE, CLOSE, COORDINATE
 */
// const VALID_PHASES = ['ANALYZE', 'PLAN', 'EXECUTE', 'CLOSE', 'COORDINATE'];
