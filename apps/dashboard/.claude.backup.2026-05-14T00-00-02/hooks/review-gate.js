#!/usr/bin/env bun
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
 * Wave 9 — Strict mode: MUSTARD_COMMIT_GATE_MODE=warn|strict|off
 * Default: warn (retrocompat). In strict mode, blocks on:
 *   - Secrets detected (sensitive files staged)
 *   - Build broken (buildCommand from mustard.json fails)
 *
 * Fail-open: exits 0 on hook/env errors. Only real sensor failures block (strict).
 *
 * @version 2.0.0
 */

'use strict';

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const { shouldRun, isStrictMode } = require('./_lib/hook-env.js');
const { emit } = require('./_lib/harness-event.js');
let emitMetric = () => {};
try { emitMetric = require('./_lib/metrics-emit.js').emitMetric; } catch (_) {}

const BUILD_TIMEOUT_MS = 5 * 60 * 1000;

function getCommitGateMode() {
  return (process.env.MUSTARD_COMMIT_GATE_MODE || 'warn').toLowerCase();
}

/** Read buildCommand from mustard.json at cwd */
function readBuildCommand(cwd) {
  try {
    const p = path.join(cwd, 'mustard.json');
    if (!fs.existsSync(p)) return null;
    const cfg = JSON.parse(fs.readFileSync(p, 'utf8'));
    return cfg.buildCommand || null;
  } catch (_) {
    return null;
  }
}

/** Run build command. Returns { ok, envError, output } */
function runBuild(cmd, cwd) {
  try {
    execSync(cmd, {
      cwd,
      stdio: 'pipe',
      timeout: BUILD_TIMEOUT_MS,
      windowsHide: true,
    });
    return { ok: true, output: '', envError: false };
  } catch (err) {
    if (err.code === 'ENOENT' || (err.message && err.message.includes('ENOENT'))) {
      return { ok: false, output: err.message, envError: true };
    }
    if (err.signal === 'SIGTERM' || (err.killed && err.code == null)) {
      return { ok: false, output: `[timeout] ${cmd}`, envError: true };
    }
    const raw = [
      err.stdout ? err.stdout.toString() : '',
      err.stderr ? err.stderr.toString() : '',
      err.message || '',
    ].filter(Boolean).join('\n').trim();
    return { ok: false, output: raw, envError: false };
  }
}

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
  const commitGateMode = getCommitGateMode();

  // Mode: off — skip entirely
  if (commitGateMode === 'off') {
    process.exit(0);
  }

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
    // Strict-blocking findings: secrets or build broken
    const blockingFindings = [];

    // Check 1: Staged changes exist? Detect sensitive/generated files.
    let sensitiveFiles = [];
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
        sensitiveFiles = files.filter(f => {
          const normalized = f.replace(/\\/g, '/');
          return (
            /\.(env|pem|key|secret|p12|pfx|cer|crt)$/i.test(normalized) ||
            /credentials/i.test(normalized) ||
            /\.env\./i.test(normalized) ||
            /\/\.env$/i.test(normalized) ||
            /^\.env$/i.test(normalized)
          );
        });
        if (sensitiveFiles.length > 0) {
          const msg = `Sensitive files staged: ${sensitiveFiles.join(', ')}`;
          warnings.push(msg);
          blockingFindings.push({ type: 'secrets', msg });
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

    // Check 5: Build integrity (strict mode only — run buildCommand if available)
    let buildResult = null;
    if (commitGateMode === 'strict') {
      const buildCmd = readBuildCommand(cwd);
      if (buildCmd) {
        buildResult = runBuild(buildCmd, cwd);
        if (!buildResult.ok && !buildResult.envError) {
          const out = (buildResult.output || '').slice(0, 300) + (buildResult.output && buildResult.output.length > 300 ? '…' : '');
          const msg = `Build broken: ${out}`;
          warnings.push(msg);
          blockingFindings.push({ type: 'build', msg });
        } else if (!buildResult.ok && buildResult.envError) {
          process.stderr.write(`[review-gate] build env error (fail-open): ${buildResult.output}\n`);
        }
      }
    }

    // Check 6: Active pipeline advisory
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

    // Emit harness event
    try {
      emit('commit-gate.check', {
        mode: commitGateMode,
        warnings: warnings.length,
        blockingFindings: blockingFindings.map(f => f.type),
        hasSensitive: sensitiveFiles.length > 0,
        buildOk: buildResult ? buildResult.ok : null,
      }, { cwd, hookInput: data });
    } catch (_) {}

    // Strict mode: block on real sensor failures
    if (commitGateMode === 'strict' && blockingFindings.length > 0) {
      try {
        emitMetric('review-gate', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'blocked',
          extras: { findings: blockingFindings.map(f => f.type) },
          cwd,
        });
      } catch (_) {}
      const reason = `[Commit Gate] ${blockingFindings.map(f => f.msg).join(' | ')}`;
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason: reason,
        },
      }) + '\n');
      process.exit(0);
    }

    // Warn mode (or strict with no blocking): emit advisory if there are warnings
    if (warnings.length > 0) {
      try {
        emitMetric('review-gate', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'warned',
          extras: { warnings: warnings.length },
          cwd,
        });
      } catch (_) {}
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
