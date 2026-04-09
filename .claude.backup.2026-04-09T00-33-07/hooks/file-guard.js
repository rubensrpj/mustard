#!/usr/bin/env node
'use strict';
/**
 * SAFETY: PreToolUse guard for sensitive file access
 *
 * Blocks Read/Write/Edit on: credentials*, *.pem, *.key, .git/config
 * Does NOT block .env (user decision).
 *
 * Belt-and-suspenders layer alongside permissions.deny.
 * Fail-open: exits 0 on any error.
 *
 * @version 1.0.0
 */

const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

const BLOCKED_PATTERNS = [
  /credentials/i,
  /\.pem$/i,
  /\.key$/i,
  /\.git[/\\]config$/i,
  /id_rsa/i,
  /id_ed25519/i,
  /\.pfx$/i,
  /\.p12$/i,
];

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('file-guard')) { process.exit(0); }
    const data = JSON.parse(input);
    const tool = data.tool_name || '';

    if (!['Read', 'Write', 'Edit'].includes(tool)) {
      process.exit(0);
    }

    const filePath = data.tool_input?.file_path || data.tool_input?.path || '';
    const normalized = filePath.replace(/\\/g, '/');
    const basename = path.basename(normalized);

    for (const pattern of BLOCKED_PATTERNS) {
      if (pattern.test(normalized) || pattern.test(basename)) {
        console.log(JSON.stringify({
          hookSpecificOutput: {
            hookEventName: 'PreToolUse',
            permissionDecision: 'deny',
            permissionDecisionReason: `[file-guard] Access to sensitive file blocked: ${basename}\nMatched pattern: ${pattern.source}`
          }
        }));
        process.exit(0);
      }
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[file-guard] Error: ${err.message}\n`);
    process.exit(0);
  }
});
