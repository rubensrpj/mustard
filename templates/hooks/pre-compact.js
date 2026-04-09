#!/usr/bin/env node
'use strict';
/**
 * PRE-COMPACT: Preserve context before conversation compaction
 *
 * Saves a snapshot of current state:
 * - Current git branch
 * - Uncommitted changes summary
 * - Active task summary (from stdin data)
 *
 * Returns additionalContext with a compact summary.
 * Saves state to .claude/.compact-state/ for debugging.
 *
 * @version 1.0.0
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('pre-compact')) { process.exit(0); }
    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();

    // ── Pipeline state validation ───────────────────────────────────────
    try {
      const statesDir = path.join(cwd, '.claude', '.pipeline-states');
      if (fs.existsSync(statesDir)) {
        const stateFiles = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
        const activeStates = stateFiles.reduce((acc, f) => {
          try {
            const parsed = JSON.parse(fs.readFileSync(path.join(statesDir, f), 'utf8'));
            if (parsed.status === 'active' || parsed.status === 'implementing') {
              acc.push({ file: f, state: parsed });
            }
          } catch (e) { /* skip unreadable */ }
          return acc;
        }, []);

        if (activeStates.length === 0) {
          // No active pipeline — skip compact (noop)
          process.exit(0);
        }

        if (activeStates.length >= 2) {
          process.stderr.write(`[pre-compact] WARNING: ${activeStates.length} active pipeline states found. Picking most recent.\n`);
          // Pick the one with most recent checkpoint or createdAt
          activeStates.sort((a, b) => {
            const tsA = new Date(a.state.checkpoint || a.state.createdAt || 0).getTime();
            const tsB = new Date(b.state.checkpoint || b.state.createdAt || 0).getTime();
            return tsB - tsA;
          });
        }
        // If exactly 1 or after sort: proceed normally (fall through)
      }
    } catch (e) { /* fail-open: validation error is non-fatal */ }

    const parts = [];

    // Git branch
    let branch = 'unknown';
    try {
      branch = execSync('git rev-parse --abbrev-ref HEAD', {
        cwd,
        encoding: 'utf8',
        stdio: ['pipe', 'pipe', 'pipe'],
      }).trim();
    } catch {}
    parts.push(`Branch: ${branch}`);

    // Uncommitted changes
    try {
      const status = execSync('git status --porcelain', {
        cwd,
        encoding: 'utf8',
        stdio: ['pipe', 'pipe', 'pipe'],
      }).trim();

      if (status) {
        const lines = status.split('\n');
        const staged = lines.filter(l => /^[MADRC]/.test(l)).length;
        const modified = lines.filter(l => /^.[MD]/.test(l)).length;
        const untracked = lines.filter(l => l.startsWith('??')).length;
        parts.push(`Changes: ${staged} staged, ${modified} modified, ${untracked} untracked`);

        // List changed files (max 20)
        const changedFiles = lines.slice(0, 20).map(l => l.substring(3)).join(', ');
        parts.push(`Files: ${changedFiles}`);
      } else {
        parts.push('Working tree: clean');
      }
    } catch {}

    // Recent commits (last 3)
    try {
      const log = execSync('git log --oneline -3', {
        cwd,
        encoding: 'utf8',
        stdio: ['pipe', 'pipe', 'pipe'],
      }).trim();
      if (log) {
        parts.push(`Recent commits:\n${log}`);
      }
    } catch {}

    // Active pipeline state
    try {
      const statesDir = path.join(cwd, '.claude', '.pipeline-states');
      if (fs.existsSync(statesDir)) {
        const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
        if (files.length > 0) {
          parts.push(`Active pipelines: ${files.map(f => f.replace('.json', '')).join(', ')}`);
        }
      }
    } catch {}

    // Persistent memory summary
    try {
      const memDir = path.join(cwd, '.claude', 'memory');
      const decCount = countEntries(path.join(memDir, 'decisions.json'));
      const lesCount = countEntries(path.join(memDir, 'lessons.json'));
      if (decCount > 0 || lesCount > 0) {
        parts.push(`Persistent memory: ${decCount} decisions, ${lesCount} lessons`);
      }
    } catch {}

    // Compact reason
    const reason = data.compact_reason || data.trigger || 'auto';
    parts.push(`Compact trigger: ${reason}`);

    const summary = parts.join('\n');

    // Save state snapshot for debugging
    const stateDir = path.join(cwd, '.claude', '.compact-state');
    try {
      if (!fs.existsSync(stateDir)) {
        fs.mkdirSync(stateDir, { recursive: true });
      }
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      fs.writeFileSync(
        path.join(stateDir, `${timestamp}.txt`),
        summary,
        'utf8'
      );
    } catch {}

    // Return additionalContext
    console.log(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreCompact',
        additionalContext: `[Pre-compact snapshot]\n${summary}`
      }
    }));

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[pre-compact] Error: ${err.message}\n`);
    process.exit(0);
  }
});

function countEntries(filePath) {
  try {
    const data = JSON.parse(fs.readFileSync(filePath, 'utf8'));
    return (data.entries || []).length;
  } catch { return 0; }
}
