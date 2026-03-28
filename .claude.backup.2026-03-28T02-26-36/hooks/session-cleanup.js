#!/usr/bin/env node
/**
 * SESSION-CLEANUP: Clean stale state files on session end
 *
 * Cleans:
 * - .claude/.agent-state/ (stale subagent tracking)
 * - .claude/.compact-state/ files older than 24h
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

const ONE_DAY_MS = 24 * 60 * 60 * 1000;

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();
    const claudeDir = path.join(cwd, '.claude');

    // Clean agent-state (only files belonging to THIS session)
    cleanAgentState(path.join(claudeDir, '.agent-state'), data.session_id);

    // Clean pipeline states (directory-based + legacy single file)
    cleanPipelineStates(claudeDir);

    // Clean statusline git cache in temp dir
    const tmpDir = require('os').tmpdir();
    cleanFile(path.join(tmpDir, 'claude-statusline-git.json'));

    // Clean compact-state (only files older than 24h)
    cleanDirectory(path.join(claudeDir, '.compact-state'), { maxAgeMs: ONE_DAY_MS });

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[session-cleanup] Error: ${err.message}\n`);
    process.exit(0);
  }
});

function cleanAgentState(dirPath, sessionId) {
  try {
    if (!fs.existsSync(dirPath)) return;
    const files = fs.readdirSync(dirPath).filter(f => f.endsWith('.json') && f !== '_queue.json');

    for (const file of files) {
      const filePath = path.join(dirPath, file);
      try {
        const content = JSON.parse(fs.readFileSync(filePath, 'utf8'));
        // Remove if: matches this session OR has no session_id (legacy)
        if (!content.session_id || content.session_id === sessionId) {
          fs.unlinkSync(filePath);
        }
      } catch {
        // Corrupt file — remove it
        try { fs.unlinkSync(filePath); } catch {}
      }
    }

    // Remove directory if empty
    try {
      if (fs.readdirSync(dirPath).length === 0) fs.rmdirSync(dirPath);
    } catch {}
  } catch {}
}

function cleanFile(filePath) {
  try {
    if (fs.existsSync(filePath)) fs.unlinkSync(filePath);
  } catch {}
}

function cleanPipelineStates(claudeDir) {
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
