#!/usr/bin/env node
'use strict';
/**
 * BASH-NATIVE-REDIRECT: PreToolUse hook that intercepts Bash commands with
 * native tool equivalents and denies them, directing to Grep / Read / Glob.
 *
 * Intercepted commands: grep, rg, egrep, fgrep, cat, head, tail, less, more,
 *                       ls, find, tree, sed (read-only)
 *
 * NOT intercepted:
 *   - `rtk …`            — already token-optimised, pass through
 *   - Piped/chained      — |, &&, ||, ;, $(…), backtick, <<, >> (complex context)
 *   - sed -i             — write operation, not a Read substitute
 *   - wc, sort, uniq, awk — no direct native tool equivalent
 *   - Any command with > redirect — writing to file, not reading
 *
 * Environment:
 *   MUSTARD_BASH_REDIRECT_MODE — strict (default) | warn | off
 *
 * Fail-open: exits 0 on any error so Claude is never blocked by hook bugs.
 *
 * @version 1.0.0
 */

const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

/** @type {Record<string, { tool: string, tip: string }>} */
const REDIRECT_MAP = {
  grep:  { tool: 'Grep', tip: 'Grep(pattern, path, output_mode) — faster, no shell overhead' },
  rg:    { tool: 'Grep', tip: 'Grep tool is built on ripgrep — same power, structured output' },
  egrep: { tool: 'Grep', tip: 'Grep(pattern) supports full regex syntax' },
  fgrep: { tool: 'Grep', tip: 'Grep(pattern, -i) for case-insensitive search' },
  cat:   { tool: 'Read', tip: 'Read(file_path) — structured output with line numbers' },
  head:  { tool: 'Read', tip: 'Read(file_path, limit: N) — reads first N lines' },
  tail:  { tool: 'Read', tip: 'Read(file_path, offset: N) — reads from line N' },
  less:  { tool: 'Read', tip: 'Read(file_path, offset, limit) — paginated reading' },
  more:  { tool: 'Read', tip: 'Read(file_path) — full file reading' },
  ls:    { tool: 'Glob', tip: 'Glob(pattern) — e.g. "src/**/*.ts" for recursive listing' },
  find:  { tool: 'Glob', tip: 'Glob(pattern) — e.g. "**/*.cs" for pattern matching' },
  tree:  { tool: 'Glob', tip: 'Glob(pattern) — structured file listing by pattern' },
};

/**
 * Shell operators that indicate a complex/composed command that cannot be
 * safely intercepted without understanding the full pipeline context.
 */
const SHELL_OPERATOR_RE = /[|&;]|\$\(|`|<<|>>/;

/**
 * Output redirect (writing to file). Commands using > are not pure readers.
 * Note: >> is already covered by SHELL_OPERATOR_RE.
 */
const OUTPUT_REDIRECT_RE = /(?<![<>])>(?![>])/;

/**
 * Strip trailing stderr redirects that don't affect first-token detection.
 * e.g. "grep foo bar 2>/dev/null" → "grep foo bar"
 */
function stripStderrRedirects(cmd) {
  return cmd
    .replace(/\s+2>\/dev\/null\s*$/, '')
    .replace(/\s+2>&1\s*$/, '')
    .trim();
}

/**
 * Extract the effective first executable token from a command string.
 * Skips env-var prefix tokens (those containing '=').
 * Returns null if no token found.
 */
function firstToken(cmd) {
  const tokens = cmd.trim().split(/\s+/);
  for (const tok of tokens) {
    if (tok.includes('=')) continue; // env var assignment prefix, skip
    return tok;
  }
  return null;
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('bash-native-redirect')) { process.exit(0); }

    const mode = (process.env.MUSTARD_BASH_REDIRECT_MODE || 'strict').toLowerCase();
    if (mode === 'off') { process.exit(0); }

    const data = JSON.parse(input);
    let cmd = data.tool_input?.command || '';
    if (!cmd) { process.exit(0); }

    // Strip trailing stderr redirects before analysis
    cmd = stripStderrRedirects(cmd);

    // Pass through: output redirect (writing to file, not reading)
    if (OUTPUT_REDIRECT_RE.test(cmd)) { process.exit(0); }

    // Piped/chained commands: can't block safely, but warn if first segment
    // starts with a redirectable command (e.g. "grep foo | sort" → advise Grep)
    if (SHELL_OPERATOR_RE.test(cmd)) {
      const firstSegment = cmd.split(/\s*[|&;]\s*/)[0].trim();
      const segToken = firstToken(firstSegment);
      if (segToken && segToken !== 'rtk') {
        const segInfo = REDIRECT_MAP[segToken.toLowerCase()];
        if (segInfo) {
          emitMetric('bash-native-redirect', {
            tokensAffected: 0,
            tokensSaved: 0,
            note: 'piped-warn',
            extras: { from: segToken.toLowerCase(), to: segInfo.tool, command_head: cmd.slice(0, 80), category: 'redirection' },
          });
          console.log(JSON.stringify({
            hookSpecificOutput: {
              hookEventName: 'PreToolUse',
              permissionDecision: 'allow',
              additionalContext:
                `[Native Tool Redirect] The \`${segToken}\` part of this piped command could use the ${segInfo.tool} tool instead. ` +
                `${segInfo.tip}. Consider splitting the pipeline to use native tools where possible.`,
            },
          }));
        }
      }
      process.exit(0);
    }

    // Extract first executable token
    const token = firstToken(cmd);
    if (!token) { process.exit(0); }

    // Pass through: already RTK-wrapped
    if (token === 'rtk') { process.exit(0); }

    // Special case: sed — only deny read-only sed (no -i flag)
    if (token === 'sed') {
      // Allow sed -i (in-place write) through
      if (/\bsed\s+(-\w*i\w*|-i\b)/.test(cmd)) { process.exit(0); }
      // Read-only sed → redirect to Grep
      const redirectInfo = { tool: 'Grep', tip: 'Grep(pattern) — for pattern extraction without shell sed overhead' };
      emitAndDeny('sed', redirectInfo, cmd, mode);
      return;
    }

    const redirectInfo = REDIRECT_MAP[token.toLowerCase()];
    if (!redirectInfo) { process.exit(0); }

    emitAndDeny(token.toLowerCase(), redirectInfo, cmd, mode);
  } catch (err) {
    process.stderr.write(`[bash-native-redirect] Error: ${err.message}\n`);
    process.exit(0);
  }
});

/**
 * Emit the metric and either deny or warn depending on mode.
 */
function emitAndDeny(firstTok, redirectInfo, cmd, mode) {
  const { tool, tip } = redirectInfo;

  emitMetric('bash-native-redirect', {
    tokensAffected: 0,
    tokensSaved: 0,
    note: 'redirected',
    extras: {
      from: firstTok,
      to: tool,
      command_head: cmd.slice(0, 80),
      category: 'redirection',
    },
  });

  if (mode === 'warn') {
    console.log(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'allow',
        additionalContext: `[Native Tool Redirect] Consider using the ${tool} tool instead of \`${firstTok}\` in Bash. ${tip}`,
      },
    }));
    process.exit(0);
  }

  // Default: strict — deny
  console.log(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'deny',
      permissionDecisionReason: `[Native Tool Redirect] Use the ${tool} tool instead of \`${firstTok}\` in Bash. ${tip}`,
    },
  }));
  process.exit(0);
}
