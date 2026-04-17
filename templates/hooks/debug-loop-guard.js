#!/usr/bin/env node
'use strict';
/**
 * DEBUG-LOOP-GUARD: PostToolUse hook that detects iterative debugging anti-patterns.
 *
 * Signals tracked:
 *   - ≥5 consecutive Edit/Write to the same file_path (reset on file change or >60s gap)
 *   - ≥3 consecutive Bash failures (exit_code != 0) on test/build commands
 *
 * Action: advisory warning to stderr + additionalContext nudging agent to Task(Plan).
 * Never blocks — exits 0 always (fail-open).
 *
 * State file: .claude/.agent-state/debug-loop-state.json
 * Format: { editStreak: { file, count, lastTs }, bashFailStreak: { count, lastTs } }
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

const EDIT_WARN_THRESHOLD = 5;
const BASH_FAIL_WARN_THRESHOLD = 3;
const STREAK_RESET_MS = 60 * 1000; // 60s gap resets streak

const BUILD_TEST_PATTERN = /\b(test|build|dotnet|npm run|tsc)\b/i;

// Pipeline files where many consecutive edits are EXPECTED (line-by-line checkbox
// updates during EXECUTE/CLOSE are the prescribed protocol, not a debug loop).
const PIPELINE_EXEMPT_RE = /[\\/]\.claude[\\/]spec[\\/](active|completed)[\\/][^\\/]+[\\/]spec\.md$/;

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('debug-loop-guard')) { process.exit(0); }

    const data = JSON.parse(input);
    const toolName = data.tool_name || '';
    const projectDir = data.cwd || process.cwd();
    const stateDir = path.join(projectDir, '.claude', '.agent-state');
    const stateFile = path.join(stateDir, 'debug-loop-state.json');

    const state = loadState(stateFile);
    const now = Date.now();
    let warning = null;

    if (toolName === 'Edit' || toolName === 'Write') {
      const filePath = (data.tool_input && data.tool_input.file_path) || '';
      warning = handleEditWrite(state, filePath, now);
    } else if (toolName === 'Bash') {
      const command = (data.tool_input && data.tool_input.command) || '';
      const exitCode = (data.tool_response && data.tool_response.exit_code) != null
        ? data.tool_response.exit_code
        : null;
      warning = handleBash(state, command, exitCode, now);
    }

    saveState(stateFile, state);

    if (warning) {
      process.stderr.write(`[Debug Loop Guard] ${warning}\n`);
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PostToolUse',
          additionalContext:
            `[Debug Loop Guard] ADVISORY (non-blocking) — possible anti-pattern: ${warning}\n` +
            `This is a heuristic. If the edits are intentional (e.g. line-by-line checkbox updates, batch refactor), ignore and continue. ` +
            `If you are actually debugging the same error repeatedly, consider delegating root-cause analysis via Task(Plan) before more edits.`,
        },
      }) + '\n');
    }

    process.exit(0);
  } catch (_err) {
    // Fail-open: any error → exit 0 silently
    process.exit(0);
  }
});

/**
 * Handle Edit/Write tool: track consecutive edits to the same file.
 * @returns {string|null} warning message or null
 */
function handleEditWrite(state, filePath, now) {
  // Pipeline spec files: expected to receive many line-by-line edits during
  // EXECUTE (checkbox updates) and CLOSE. Skip tracking entirely.
  if (filePath && PIPELINE_EXEMPT_RE.test(filePath)) {
    return null;
  }

  const streak = state.editStreak || { file: '', count: 0, lastTs: 0 };
  const elapsed = now - (streak.lastTs || 0);

  if (filePath && streak.file === filePath && elapsed < STREAK_RESET_MS) {
    streak.count += 1;
  } else {
    // Different file or gap too large → reset
    streak.file = filePath;
    streak.count = 1;
  }
  streak.lastTs = now;
  state.editStreak = streak;

  if (streak.count >= EDIT_WARN_THRESHOLD) {
    return `${streak.count} consecutive edits to \`${path.basename(filePath)}\` without verification.`;
  }
  return null;
}

/**
 * Handle Bash tool: track consecutive failures on test/build commands.
 * @returns {string|null} warning message or null
 */
function handleBash(state, command, exitCode, now) {
  const streak = state.bashFailStreak || { count: 0, lastTs: 0 };
  const elapsed = now - (streak.lastTs || 0);
  const isBuildTest = BUILD_TEST_PATTERN.test(command);
  const isFailed = exitCode != null && exitCode !== 0;

  if (isBuildTest && isFailed) {
    if (elapsed < STREAK_RESET_MS) {
      streak.count += 1;
    } else {
      streak.count = 1;
    }
    streak.lastTs = now;
    state.bashFailStreak = streak;

    if (streak.count >= BASH_FAIL_WARN_THRESHOLD) {
      return `${streak.count} consecutive failing test/build commands — re-examine root cause.`;
    }
  } else {
    // Success or non-build command resets the failure streak
    if (!isBuildTest || !isFailed) {
      streak.count = 0;
      streak.lastTs = now;
      state.bashFailStreak = streak;
    }
  }
  return null;
}

function loadState(stateFile) {
  try {
    return JSON.parse(fs.readFileSync(stateFile, 'utf8'));
  } catch {
    return {};
  }
}

function saveState(stateFile, state) {
  try {
    fs.mkdirSync(path.dirname(stateFile), { recursive: true });
    fs.writeFileSync(stateFile, JSON.stringify(state, null, 2), 'utf8');
  } catch {
    // Fail silently
  }
}
