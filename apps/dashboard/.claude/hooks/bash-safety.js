#!/usr/bin/env node
'use strict';
/**
 * SAFETY: PreToolUse guard for dangerous Bash commands
 *
 * Blocks: rm -rf, force push, reset --hard, chmod 777, mkfs, dd, format,
 *         git branch -D main/master, shutdown, reboot
 *
 * Belt-and-suspenders layer — deny rules in settings.json are primary.
 * Fail-open: exits 0 on any error.
 *
 * @version 1.0.0
 */

const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

const DANGEROUS = [
  { re: /\brm\s+(-\w*r\w*f|--no-preserve-root|-rf|-fr)\b/i, msg: 'Recursive force delete blocked', tag: 'rm-rf' },
  { re: /\bgit\s+push\s+(-\w*f\b|--force(?!-with-lease))\b/i, msg: 'Force push blocked (use --force-with-lease for safer overwrite)', tag: 'force-push' },
  { re: /\bgit\s+reset\s+--hard\b/i, msg: 'git reset --hard blocked', tag: 'reset-hard' },
  { re: /\bgit\s+clean\s+-f/i, msg: 'git clean -f blocked', tag: 'git-clean' },
  { re: /\bgit\s+checkout\s+--\s*\.\s*$/i, msg: 'git checkout -- . blocked', tag: 'checkout-dot' },
  { re: /\bgit\s+restore\s+\.\s*$/i, msg: 'git restore . blocked', tag: 'restore-dot' },
  { re: /\bgit\s+branch\s+-D\s+(main|master)\b/i, msg: 'Deleting main/master branch blocked', tag: 'branch-delete-main' },
  { re: /\bchmod\s+777\b/i, msg: 'chmod 777 blocked', tag: 'chmod-777' },
  { re: /\bmkfs\b/i, msg: 'mkfs blocked', tag: 'mkfs' },
  { re: /\bdd\s+if=/i, msg: 'dd if= blocked', tag: 'dd' },
  { re: /\bformat\s+[A-Z]:/i, msg: 'format drive blocked', tag: 'format-drive' },
  { re: /\bshutdown\b/i, msg: 'shutdown blocked', tag: 'shutdown' },
  { re: /\breboot\b/i, msg: 'reboot blocked', tag: 'reboot' },
];

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('bash-safety')) { process.exit(0); }
    const data = JSON.parse(input);
    if (data.tool_name !== 'Bash') {
      process.exit(0);
    }

    const cmd = data.tool_input?.command || '';

    for (const { re, msg, tag } of DANGEROUS) {
      if (re.test(cmd)) {
        emitMetric('bash-safety', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'blocked-' + tag,
          extras: { command_head: cmd.slice(0, 80), pattern: tag, category: 'prevention' },
        });
        console.log(JSON.stringify({
          hookSpecificOutput: {
            hookEventName: 'PreToolUse',
            permissionDecision: 'deny',
            permissionDecisionReason: `[bash-safety] ${msg}.\nCommand: ${cmd.substring(0, 120)}`
          }
        }));
        process.exit(0);
      }
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[bash-safety] Error: ${err.message}\n`);
    process.exit(0);
  }
});
