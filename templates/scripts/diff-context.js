#!/usr/bin/env bun
/**
 * DIFF-CONTEXT: Generate compact git diff summary for agent context
 *
 * Outputs a formatted diff summary that can be injected into agent prompts.
 * Includes: staged changes, unstaged changes, and commits since branch divergence.
 *
 * Usage: bun .claude/scripts/diff-context.js [--parent branch-name] [--subproject path] [--phase analyze|plan|execute]
 * Output: Formatted markdown summary to stdout (max 3000 chars)
 *
 * Flags:
 *   --parent     branch to compare against (auto-detects main/master)
 *   --subproject scope diff to a path
 *   --phase      pipeline phase. When `analyze`, exits silently (no-op) — diff
 *                is always empty before work starts; ANALYZE callers emit the
 *                `analyze-diff-skip` metric instead. Other values behave normally.
 *
 * @version 1.2.0
 */

const { execSync } = require('child_process');

const MAX_CHARS = 3000;

function run(cmd, cwd) {
  try {
    return execSync(cmd, {
      cwd,
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
      timeout: 10000,
      windowsHide: true,
    }).trim();
  } catch { return ''; }
}

function main() {
  try {
    const cwd = process.cwd();
    const args = process.argv.slice(2);
    const parentIdx = args.indexOf('--parent');
    let parentBranch = parentIdx >= 0 && args[parentIdx + 1] ? args[parentIdx + 1] : null;
    const subIdx = args.indexOf('--subproject');
    const subPath = subIdx >= 0 && args[subIdx + 1] ? args[subIdx + 1] : null;
    const phaseIdx = args.indexOf('--phase');
    const phase = phaseIdx >= 0 && args[phaseIdx + 1] ? String(args[phaseIdx + 1]).toLowerCase() : null;

    // Silent no-op on ANALYZE: diff is always empty pre-work, so we skip the
    // git invocations entirely. Callers emit the `analyze-diff-skip` metric.
    if (phase === 'analyze') {
      console.log('');
      process.exit(0);
    }

    function scopeCmd(cmd) {
      return subPath ? `${cmd} -- ${subPath}` : cmd;
    }

    // Auto-detect parent branch if not specified
    if (!parentBranch) {
      const branch = run('git rev-parse --abbrev-ref HEAD', cwd);
      if (branch && branch !== 'main' && branch !== 'master') {
        if (run('git rev-parse --verify main', cwd)) {
          parentBranch = 'main';
        } else if (run('git rev-parse --verify master', cwd)) {
          parentBranch = 'master';
        }
      }
    }

    const parts = [];

    // Current branch
    const currentBranch = run('git rev-parse --abbrev-ref HEAD', cwd);
    if (currentBranch) {
      parts.push(`## Branch: ${currentBranch}`);
    }

    // Staged changes
    const stagedStat = run(scopeCmd('git diff --cached --stat'), cwd);
    const stagedFiles = run(scopeCmd('git diff --cached --name-only'), cwd);
    if (stagedFiles) {
      parts.push('## Staged Changes');
      parts.push('```');
      parts.push(stagedStat || stagedFiles);
      parts.push('```');
    }

    // Unstaged changes
    const unstagedStat = run(scopeCmd('git diff --stat'), cwd);
    const unstagedFiles = run(scopeCmd('git diff --name-only'), cwd);
    if (unstagedFiles) {
      parts.push('## Unstaged Changes');
      parts.push('```');
      parts.push(unstagedStat || unstagedFiles);
      parts.push('```');
    }

    // Untracked files
    const untracked = run(scopeCmd('git ls-files --others --exclude-standard'), cwd);
    if (untracked) {
      const files = untracked.split('\n').filter(Boolean);
      if (files.length > 0 && files.length <= 20) {
        parts.push('## Untracked Files');
        files.forEach(f => parts.push(`- ${f}`));
      } else if (files.length > 20) {
        parts.push(`## Untracked Files (${files.length} total)`);
        files.slice(0, 10).forEach(f => parts.push(`- ${f}`));
        parts.push(`- ...and ${files.length - 10} more`);
      }
    }

    // Commits since divergence from parent
    if (parentBranch) {
      const mergeBase = run(`git merge-base ${parentBranch} HEAD`, cwd);
      if (mergeBase) {
        const log = run(scopeCmd(`git log --oneline ${mergeBase}..HEAD`), cwd);
        if (log) {
          parts.push(`## Commits since ${parentBranch}`);
          const commits = log.split('\n').filter(Boolean);
          if (commits.length <= 20) {
            commits.forEach(c => parts.push(`- ${c}`));
          } else {
            commits.slice(0, 15).forEach(c => parts.push(`- ${c}`));
            parts.push(`- ...and ${commits.length - 15} more commits`);
          }
        }

        // Diff stat since divergence
        const diffStat = run(scopeCmd(`git diff --stat ${mergeBase}..HEAD`), cwd);
        if (diffStat) {
          parts.push('### Changed files since divergence');
          parts.push('```');
          parts.push(diffStat);
          parts.push('```');
        }
      }
    }

    if (parts.length === 0) {
      parts.push('No changes detected.');
    }

    // Build output with cap
    let output = parts.join('\n');
    if (output.length > MAX_CHARS) {
      output = output.slice(0, MAX_CHARS - 20) + '\n...truncated';
    }

    console.log(output);
  } catch (err) {
    process.stderr.write(`[diff-context] Error: ${err.message}\n`);
    console.log('Unable to generate diff context.');
  }

  process.exit(0);
}

main();
