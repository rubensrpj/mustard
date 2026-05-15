#!/usr/bin/env bun
'use strict';
/**
 * SESSION-CLEANUP: Clean stale state files on session end
 *
 * Cleans:
 * - .claude/.pipeline-states/ terminal states (completed, cancelled, etc.)
 * - .claude/.compact-state/ files older than 24h
 * - .claude/.pipeline-state.json legacy single file (terminal states only)
 *
 * Wave 4: .agent-state/ and .agent-memory/ are no longer written by any hook,
 * so cleanup of those directories is removed. The harness log rotation
 * (.harness/sessions/*.jsonl > 30d) is handled by harness-init.js on SessionStart.
 *
 * @version 2.0.0
 */

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const { shouldRun } = require('./_lib/hook-env.js');

const ONE_DAY_MS = 24 * 60 * 60 * 1000;

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('session-cleanup')) { process.exit(0); }
    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();
    const claudeDir = path.join(cwd, '.claude');

    // Archive stale closed-followup specs (Onda 2.3) — best-effort, fail-open
    try {
      const completeScript = path.join(claudeDir, 'scripts', 'complete-spec.js');
      if (fs.existsSync(completeScript)) {
        spawnSync(process.execPath, [completeScript, '--archive-stale'], {
          cwd,
          timeout: 5000,
          stdio: ['ignore', 'ignore', 'ignore'],
          windowsHide: true,
        });
      }
    } catch (_) {}

    // Clean pipeline states (directory-based + legacy single file)
    cleanPipelineStates(claudeDir);

    // Clean statusline git cache in temp dir
    const tmpDir = require('os').tmpdir();
    cleanFile(path.join(tmpDir, 'claude-statusline-git.json'));

    // Clean compact-state (only files older than 24h)
    cleanDirectory(path.join(claudeDir, '.compact-state'), { maxAgeMs: ONE_DAY_MS });

    // NOTE: .claude/memory/ is PERSISTENT — do NOT clean.

    // Stop OTEL collector spawned by harness-init.js (best-effort)
    stopOtelCollector(claudeDir);

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[session-cleanup] Error: ${err.message}\n`);
    process.exit(0);
  }
});

/**
 * Stop the OTEL collector started by harness-init.js. Graceful SIGTERM with a
 * 2s wait, then SIGKILL as escalation. Always removes the PID file on exit.
 * Fail-open: any I/O or signal error is swallowed.
 */
function stopOtelCollector(claudeDir) {
  const pidFile = path.join(claudeDir, '.harness', '.otel-collector.pid');
  try {
    if (!fs.existsSync(pidFile)) return;
    let pid;
    try {
      pid = parseInt(fs.readFileSync(pidFile, 'utf8').trim(), 10);
    } catch (_) { pid = NaN; }

    if (!Number.isFinite(pid) || pid <= 0) {
      try { fs.unlinkSync(pidFile); } catch (_) {}
      return;
    }

    try { process.kill(pid, 'SIGTERM'); } catch (_) { /* already dead */ }

    // Poll up to 2s for the process to exit.
    const deadline = Date.now() + 2000;
    let alive = true;
    while (Date.now() < deadline) {
      try { process.kill(pid, 0); } catch (_) { alive = false; break; }
      // Busy-wait with a small spawnSync sleep — Bun/Node lack sync sleep.
      try { spawnSync(process.execPath, ['-e', 'setTimeout(()=>{},200)'], { timeout: 300 }); } catch (_) { break; }
    }

    if (alive) {
      try { process.kill(pid, 'SIGKILL'); } catch (_) {}
    }

    try { fs.unlinkSync(pidFile); } catch (_) {}
  } catch (_) { /* fail-open */ }
}

function cleanFile(filePath) {
  try {
    if (fs.existsSync(filePath)) fs.unlinkSync(filePath);
  } catch {}
}

function cleanPipelineStates(claudeDir) {
  // closed-followup is NOT terminal — it must survive cleanup so metrics-tracker
  // can still link follow-up edits. Archival happens via complete-spec.js --archive-stale
  // (invoked above) once the 24h TTL elapses.
  const terminal = new Set(['implemented', 'completed', 'validated', 'cancelled']);

  // Directory-based states
  const statesDir = path.join(claudeDir, '.pipeline-states');
  try {
    if (fs.existsSync(statesDir)) {
      const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
      for (const f of files) {
        try {
          const filePath = path.join(statesDir, f);
          const raw = JSON.parse(fs.readFileSync(filePath, 'utf8'));
          // Remove terminal states
          if (terminal.has(raw.status)) { fs.unlinkSync(filePath); continue; }
          // Remove orphaned states: spec is completed but pipeline state is still active
          if (raw.specName && isSpecDone(claudeDir, raw.specName)) { fs.unlinkSync(filePath); continue; }
        } catch {}
      }
      // Remove directory if empty
      try {
        if (fs.readdirSync(statesDir).length === 0) fs.rmdirSync(statesDir);
      } catch {}
    }
  } catch {}

  // Backward compat: legacy single file
  const legacyFile = path.join(claudeDir, '.pipeline-state.json');
  try {
    if (fs.existsSync(legacyFile)) {
      const raw = JSON.parse(fs.readFileSync(legacyFile, 'utf8'));
      if (terminal.has(raw.status)) fs.unlinkSync(legacyFile);
    }
  } catch {}
}

function isSpecDone(claudeDir, specName) {
  // Check completed/ directory
  if (fs.existsSync(path.join(claudeDir, 'spec', 'completed', specName))) return true;
  // Check active spec status header
  const specFile = path.join(claudeDir, 'spec', 'active', specName, 'spec.md');
  try {
    if (!fs.existsSync(specFile)) return true; // spec deleted = done
    const head = fs.readFileSync(specFile, 'utf8').slice(0, 500);
    return /Status:\s*(completed|done)\b/i.test(head);
  } catch { return false; }
}

function cleanDirectory(dirPath, opts = {}) {
  try {
    if (!fs.existsSync(dirPath)) return;

    const files = fs.readdirSync(dirPath);
    const now = Date.now();
    let remaining = 0;

    for (const file of files) {
      const filePath = path.join(dirPath, file);
      try {
        if (opts.removeAll) {
          fs.unlinkSync(filePath);
        } else if (opts.maxAgeMs) {
          const stat = fs.statSync(filePath);
          if (now - stat.mtimeMs > opts.maxAgeMs) {
            fs.unlinkSync(filePath);
          } else {
            remaining++;
          }
        }
      } catch {}
    }

    // Remove empty directory
    if (remaining === 0) {
      try {
        const leftover = fs.readdirSync(dirPath);
        if (leftover.length === 0) {
          fs.rmdirSync(dirPath);
        }
      } catch {}
    }
  } catch {}
}
