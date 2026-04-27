#!/usr/bin/env node
/**
 * REGRESSION-GUARD: PostToolUse hook that re-runs the test for a file when it
 * is part of a "shared" path (edited in >= 2 CLOSED specs).
 *
 * Matcher: PostToolUse Write|Edit
 *
 * Heuristic:
 *   1. Check if the edited file has appeared in events from >=2 CLOSED pipeline states.
 *   2. Try to map file → test file by basename (e.g. foo.ts → foo.test.ts).
 *   3. Run the test. On fail: emit regression.warn (warn mode) or block (strict).
 *   4. If test file cannot be inferred → skip silently (no warn).
 *
 * Env:
 *   MUSTARD_REGRESSION_MODE=warn|strict|off  (default: off — too costly for default-on)
 *
 * @version 1.0.0
 */

'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

let emit;
try { emit = require('./_lib/harness-event.js').emit; } catch (_) { emit = () => false; }

let shouldRun;
try { shouldRun = require('./_lib/hook-env.js').shouldRun; } catch (_) { shouldRun = () => true; }

const HOOK_NAME = 'regression-guard';
const TEST_TIMEOUT_MS = 60 * 1000; // 1 min per test run
const LOOKBACK_DAYS = 30;

// ── Shared-file detection ─────────────────────────────────────────────────────

/**
 * Read pipeline-states/*.json and harness events to find files edited in >= 2 closed specs.
 * Returns Set<string> of "shared" normalized file paths.
 * Expensive but only called when mode != off.
 */
function findSharedFiles(cwd) {
  const shared = new Set();
  try {
    const statesDir = path.join(cwd, '.claude', '.pipeline-states');
    if (!fs.existsSync(statesDir)) return shared;

    // Find closed specs
    const stateFiles = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
    const cutoff = Date.now() - LOOKBACK_DAYS * 24 * 60 * 60 * 1000;

    const closedSpecs = new Set();
    for (const sf of stateFiles) {
      try {
        const fullPath = path.join(statesDir, sf);
        const mtime = fs.statSync(fullPath).mtimeMs;
        if (mtime < cutoff) continue;

        const state = JSON.parse(fs.readFileSync(fullPath, 'utf8'));
        const phase = (state.phase || state.phaseName || '').toUpperCase();
        if (phase === 'CLOSE' || phase === 'CLOSED') {
          closedSpecs.add(sf.replace(/\.json$/, ''));
        }
      } catch (_) {}
    }

    // For each closed spec, read events.jsonl to see what files were touched
    const eventsFile = path.join(cwd, '.claude', '.harness', 'events.jsonl');
    if (!fs.existsSync(eventsFile)) return shared;

    const raw = fs.readFileSync(eventsFile, 'utf8');
    const lines = raw.split(/\r?\n/).filter(Boolean);

    // Count per file how many distinct closed specs touched it
    const fileSpecCount = {}; // file -> Set<spec>

    for (const line of lines) {
      try {
        const ev = JSON.parse(line);
        if (!ev || !ev.spec) continue;
        if (!closedSpecs.has(ev.spec)) continue;

        // Look for tool.use events with file paths (write/edit operations)
        if (ev.event !== 'tool.use') continue;
        const p = ev.payload || {};
        const tool = (p.tool || '').toLowerCase();
        if (tool !== 'write' && tool !== 'edit') continue;

        const fp = p.file_path || p.path || '';
        if (!fp) continue;

        const norm = fp.replace(/\\/g, '/');
        if (!fileSpecCount[norm]) fileSpecCount[norm] = new Set();
        fileSpecCount[norm].add(ev.spec);
      } catch (_) {}
    }

    for (const [file, specs] of Object.entries(fileSpecCount)) {
      if (specs.size >= 2) shared.add(file);
    }
  } catch (_) {}

  return shared;
}

// ── Test file inference ───────────────────────────────────────────────────────

/**
 * Try to find a test file for the given source file.
 * Checks common patterns by replacing/adding .test / .spec suffix.
 * Returns the test file path string if found on disk, or null.
 */
function inferTestFile(filePath, cwd) {
  const dir = path.dirname(filePath);
  const ext = path.extname(filePath);
  const base = path.basename(filePath, ext);

  const candidates = [
    path.join(dir, `${base}.test${ext}`),
    path.join(dir, `${base}.spec${ext}`),
    path.join(dir, '__tests__', `${base}.test${ext}`),
    path.join(dir, '__tests__', `${base}.spec${ext}`),
    // One level up
    path.join(dir, '..', '__tests__', `${base}.test${ext}`),
  ];

  for (const c of candidates) {
    const abs = path.isAbsolute(c) ? c : path.join(cwd, c);
    if (fs.existsSync(abs)) return abs;
  }
  return null;
}

// ── Test runner ───────────────────────────────────────────────────────────────

/**
 * Run a test file via Node.js built-in test runner or mustard.json testCommand.
 * Returns { ok, output, envError }.
 */
function runTest(testFile, cwd) {
  if (!testFile) return { ok: false, output: 'no test file', envError: true };

  const IS_WIN = process.platform === 'win32';
  const shellCmd = IS_WIN ? 'cmd' : 'sh';

  // Try node --test first (for .test.js files)
  if (testFile.endsWith('.js') || testFile.endsWith('.ts')) {
    // Read mustard.json for testCommand; if present, prefer it
    let testCmd = null;
    try {
      const cfg = JSON.parse(fs.readFileSync(path.join(cwd, 'mustard.json'), 'utf8'));
      testCmd = cfg.testCommand || null;
    } catch (_) {}

    const cmd = testCmd ? `${testCmd} "${testFile}"` : `node --test "${testFile}"`;

    try {
      const result = spawnSync(IS_WIN ? shellCmd : shellCmd, [IS_WIN ? '/c' : '-c', cmd], {
        cwd,
        stdio: 'pipe',
        timeout: TEST_TIMEOUT_MS,
        encoding: 'utf8',
        windowsHide: true,
      });

      if (result.error) return { ok: false, output: result.error.message, envError: true };
      if (result.status === null) return { ok: false, output: `[timeout] ${cmd}`, envError: true };

      const ok = result.status === 0;
      const output = [result.stdout || '', result.stderr || ''].join('\n').trim().slice(0, 500);
      return { ok, output, envError: false };
    } catch (e) {
      return { ok: false, output: e.message, envError: true };
    }
  }

  return { ok: false, output: 'unsupported file type for regression test', envError: true };
}

// ── Main logic ────────────────────────────────────────────────────────────────

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun(HOOK_NAME)) process.exit(0);
  } catch (_) {}

  const mode = (process.env.MUSTARD_REGRESSION_MODE || 'off').toLowerCase();

  // Default off — too expensive to run for every write
  if (mode === 'off') process.exit(0);

  let data;
  try {
    data = JSON.parse(input);
  } catch (_) {
    process.exit(0); // fail-open
  }

  try {
    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || toolInput.path || '';
    if (!filePath) process.exit(0);

    const cwd = data.cwd || process.cwd();

    // Check if file is "shared" (touched in >= 2 closed specs)
    const sharedFiles = findSharedFiles(cwd);
    const normalizedFilePath = filePath.replace(/\\/g, '/');

    const isShared = sharedFiles.has(normalizedFilePath) ||
      // Also check by relative comparison
      Array.from(sharedFiles).some(sf => normalizedFilePath.endsWith(sf) || sf.endsWith(normalizedFilePath));

    if (!isShared) {
      // Not a shared file → skip silently
      process.exit(0);
    }

    // Try to infer test file
    const absFilePath = path.isAbsolute(filePath) ? filePath : path.join(cwd, filePath);
    const testFile = inferTestFile(absFilePath, cwd);

    if (!testFile) {
      // Cannot map to test file → skip silently (heuristic gap, expected)
      process.exit(0);
    }

    // Run the test
    const result = runTest(testFile, cwd);

    if (result.envError) {
      // Environment issue, not a real test failure → fail-open
      process.stderr.write(`[regression-guard] Env error running test (fail-open): ${result.output}\n`);
      process.exit(0);
    }

    if (result.ok) {
      // Test passes → no warn
      process.exit(0);
    }

    // Test failed — real signal
    const reason = `[regression-guard] Test regression detected for shared file "${path.basename(filePath)}".\nTest: ${testFile}\nOutput: ${result.output}`;

    try {
      emit('regression.warn', {
        file: filePath,
        testFile,
        output: result.output,
      }, { cwd, hookInput: data });
    } catch (_) {}

    if (mode === 'strict') {
      process.stdout.write(JSON.stringify({
        decision: 'block',
        reason,
      }) + '\n');
      process.exit(0);
    }

    // warn
    process.stderr.write(reason + '\n');
    process.exit(0);

  } catch (err) {
    process.stderr.write(`[regression-guard] Hook error (fail-open): ${err.message}\n`);
    process.exit(0);
  }
});
