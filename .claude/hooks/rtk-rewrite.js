#!/usr/bin/env node
'use strict';
/**
 * RTK REWRITE: PreToolUse hook that rewrites Bash commands through RTK
 *
 * Uses `rtk rewrite` (the official hook API) to get the optimized command.
 * Exit 0 + stdout = rewritten command; Exit 1 = no RTK equivalent.
 *
 * This approach:
 * - Eliminates the "No hook installed" warning (no `rtk <cmd>` prefix)
 * - Delegates command selection to RTK itself (no manual command set)
 * - Works cross-platform (Windows + Unix)
 *
 * RTK availability is cached in a temp file (60s TTL) to avoid spawning
 * which/where on every command invocation.
 *
 * Fail-open: exits 0 on any error so Claude is never blocked by this hook.
 *
 * @version 2.1.0
 */

const { execFileSync, execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

const CACHE_FILE = path.join(os.tmpdir(), 'rtk-available.json');
const CACHE_TTL_MS = 60_000;

/**
 * Returns true if `rtk` is available in PATH, using a cached result when
 * the cache is still within TTL.
 */
function isRtkAvailable() {
  try {
    if (fs.existsSync(CACHE_FILE)) {
      const raw = fs.readFileSync(CACHE_FILE, 'utf8');
      const cached = JSON.parse(raw);
      if (Date.now() - cached.ts < CACHE_TTL_MS) {
        return cached.available;
      }
    }
  } catch (_) {
    // Cache read failed — fall through to fresh check
  }

  let available = false;
  try {
    if (process.platform === 'win32') {
      execFileSync('where', ['rtk'], { stdio: 'ignore' });
    } else {
      execFileSync('which', ['rtk'], { stdio: 'ignore' });
    }
    available = true;
  } catch (_) {
    available = false;
  }

  try {
    fs.writeFileSync(CACHE_FILE, JSON.stringify({ available, ts: Date.now() }), 'utf8');
  } catch (_) {
    // Cache write failed — non-fatal
  }

  return available;
}

/**
 * Asks RTK to rewrite the command. Returns the rewritten command string,
 * or null if RTK has no optimized equivalent (exit code 1).
 */
function rtkRewrite(cmd) {
  try {
    // rtk rewrite expects the raw command as a single argv element.
    // Using execFileSync avoids shell re-parsing, which would strip quotes
    // and corrupt regex patterns containing brackets (e.g. grep '[x]').
    const result = execFileSync('rtk', ['rewrite', cmd], {
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'ignore'], // ignore stderr
      timeout: 3000,
    });
    const rewritten = result.trim();
    return rewritten || null;
  } catch (_) {
    // Exit 1 = no RTK equivalent, or timeout/error
    return null;
  }
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('rtk-rewrite')) { process.exit(0); }
    const data = JSON.parse(input);
    const cmd = data.tool_input?.command || '';

    // Already prefixed with rtk or RTK not available — pass through
    if (cmd.startsWith('rtk ') || !isRtkAvailable()) {
      process.exit(0);
    }

    // Ask RTK for the rewritten command
    const rewritten = rtkRewrite(cmd);
    if (!rewritten || rewritten === cmd) {
      // No optimization available or same command — pass through
      process.exit(0);
    }

    // Real tokens_saved numbers are imported from `rtk gain --format json` via
    // scripts/rtk-gain-import.js (event: rtk-gain). We only record the fact of
    // a rewrite here; no heuristics.
    emitMetric('rtk-rewrite', {
      tokensAffected: Math.round(cmd.length / 4),
      note: 'rewritten via rtk',
      extras: {
        command_head: cmd.slice(0, 60),
        rewritten_head: rewritten.slice(0, 60),
      },
    });

    console.log(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'allow',
        updatedInput: { command: `${rewritten} 2>/dev/null` }
      }
    }));
    process.exit(0);
  } catch (err) {
    process.stderr.write(`[rtk-rewrite] Error: ${err.message}\n`);
    process.exit(0);
  }
});
