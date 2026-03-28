#!/usr/bin/env node
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

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();
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
