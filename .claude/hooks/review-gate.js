#!/usr/bin/env node
/**
 * REVIEW-GATE: PreToolUse hook that validates before git commit
 *
 * Detects `git commit` in Bash commands and checks:
 * 1. Are there staged changes? (git diff --cached --name-only)
 * 2. Are sensitive files staged? (.env, .pem, .key, credentials, etc.)
 * 3. Are generated/build files staged? (dist/, node_modules/, obj/, bin/)
 * 4. Is the commit suspiciously large? (>30 files)
 * 5. Are there active pipelines? (advisory reminder to match spec)
 *
 * Fail-open: exits 0 on any error. Advisory warnings only — never blocks.
 *
 * @version 1.0.0
 */

'use strict';

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const { shouldRun, isStrictMode } = require('./_lib/hook-env.js');

/**
 * Detect git commit commands (not git add, push, etc.)
 * Handles: `git commit`, `rtk git commit`, quoted variants.
 */
function isGitCommit(cmd) {
  return /\bgit\s+commit\b/i.test(cmd);
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('review-gate')) { process.exit(0); }
    const data = JSON.parse(input);
    const cmd = data.tool_input?.command || '';

    // Only trigger on git commit commands
    if (!isGitCommit(cmd)) {
      process.exit(0);
    }

    const cwd = data.cwd || process.cwd();
    const warnings = [];

    // Check 1: Staged changes exist? Detect sensitive/generated files.
    try {
      const staged = execSync('git diff --cached --name-only', {
        cwd,
        encoding: 'utf8',
        stdio: ['pipe', 'pipe', 'pipe'],
        timeout: 5000,
        windowsHide: true,
      }).trim();

      if (!staged) {
        warnings.push('No staged changes detected');
      } else {
        const files = staged.split('\n').filter(Boolean);

        // Check 2: Sensitive file patterns
        const sensitive = files.filter(f => {
          const normalized = f.replace(/\\/g, '/');
          return (
            /\.(env|pem|key|secret|p12|pfx|cer|crt)$/i.test(normalized) ||
            /credentials/i.test(normalized) ||
            /\.env\./i.test(normalized) ||
            /\/\.env$/i.test(normalized) ||
            /^\.env$/i.test(normalized)
          );
        });
        if (sensitive.length > 0) {
          warnings.push(`Sensitive files staged: ${sensitive.join(', ')}`);
        }

        // Check 3: Generated/build output files
        const generated = files.filter(f => {
          const normalized = f.replace(/\\/g, '/');
          return (
            /^dist\//i.test(normalized) ||
            /^node_modules\//i.test(normalized) ||
            /^obj\//i.test(normalized) ||
            /^bin\//i.test(normalized)
          );
        });
        if (generated.length > 0) {
          warnings.push(`Generated/build files staged: ${generated.join(', ')}`);
        }

        // Check 4: Large commit warning
        if (files.length > 30) {
          warnings.push(`Large commit: ${files.length} files staged. Consider splitting.`);
        }
      }
    } catch (_) {
      // fail-open — git may not be available in cwd
    }

    // Check 5: Active pipeline advisory
    try {
      const statesDir = path.join(cwd, '.claude', '.pipeline-states');
      if (fs.existsSync(statesDir)) {
        const pipelineFiles = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
        if (pipelineFiles.length > 0) {
          const names = pipelineFiles.map(f => f.replace('.json', '')).join(', ');
          warnings.push(`Active pipeline(s): ${names}. Ensure changes match spec.`);
        }
      }
    } catch (_) {
      // fail-open
    }

    if (warnings.length > 0) {
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: isStrictMode() ? 'deny' : 'allow',
          permissionDecisionReason: `[Review Gate] ${warnings.join(' | ')}`,
        },
      }) + '\n');
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[review-gate] Error: ${err.message}\n`);
    process.exit(0);
  }
});
