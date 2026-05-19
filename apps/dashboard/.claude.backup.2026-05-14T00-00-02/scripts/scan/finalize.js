#!/usr/bin/env bun
'use strict';

/**
 * scan/finalize.js
 *
 * Post-dispatch finalization for /scan. Runs after all Task agents return.
 * Refreshes the entity registry, updates the detect cache, validates
 * generated skills, and runs the security scan — all in parallel (the 4
 * sub-scripts touch independent files and do not share mutable state).
 *
 * Contract:
 *   stdout: JSON { steps: { registry, cache, skills, security }, errors, warnings }
 *   exit:   always 0 (fail-open). Per-step errors are reported in the JSON.
 *
 * Usage:
 *   bun .claude/scripts/scan/finalize.js
 *   bun .claude/scripts/scan/finalize.js --skip-security
 */

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const CLAUDE_DIR = path.join(ROOT, '.claude');
const SCRIPTS_DIR = path.join(CLAUDE_DIR, 'scripts');
const SYNC_REGISTRY = path.join(SCRIPTS_DIR, 'sync-registry.js');
const SYNC_DETECT = path.join(SCRIPTS_DIR, 'sync-detect.js');
const SKILL_VALIDATE = path.join(SCRIPTS_DIR, 'skill-validate.js');
const SECURITY_SCAN = path.join(SCRIPTS_DIR, 'security-scan.js');
const DISPATCH_STATE = path.join(CLAUDE_DIR, '.scan-dispatch.json');

const argv = process.argv.slice(2);
const SKIP_SECURITY = argv.includes('--skip-security');

const result = {
  steps: {
    registry: { ran: false, ok: null, durationMs: null },
    cache: { ran: false, ok: null, durationMs: null },
    skills: { ran: false, ok: null, durationMs: null, mode: null },
    security: { ran: false, ok: null, durationMs: null, findings: 0 },
    dispatchVerify: { ran: false, ok: null, subprojects: [] },
  },
  errors: [],
  warnings: [],
};

function existsSafe(p) {
  try { return fs.existsSync(p); } catch { return false; }
}

function runScript(scriptPath, args, opts = {}) {
  if (!existsSafe(scriptPath)) {
    return Promise.resolve({
      ok: false,
      error: `script not found: ${path.relative(ROOT, scriptPath)}`,
      durationMs: 0,
      status: null,
      stdout: '',
      stderr: '',
    });
  }
  const start = Date.now();
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [scriptPath, ...args], {
      cwd: ROOT,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, ...(opts.env || {}) },
      windowsHide: true,
    });
    const out = [];
    const err = [];
    child.stdout.on('data', (b) => out.push(b));
    child.stderr.on('data', (b) => err.push(b));
    child.on('error', (e) => {
      resolve({
        ok: false,
        error: e.message,
        durationMs: Date.now() - start,
        status: null,
        stdout: Buffer.concat(out).toString('utf-8'),
        stderr: Buffer.concat(err).toString('utf-8'),
      });
    });
    child.on('close', (code) => {
      resolve({
        ok: code === 0,
        status: code,
        stdout: Buffer.concat(out).toString('utf-8'),
        stderr: Buffer.concat(err).toString('utf-8'),
        durationMs: Date.now() - start,
      });
    });
  });
}

// ---------------------------------------------------------------------------
// Step 4.7 — Refresh entity registry
// ---------------------------------------------------------------------------

async function refreshRegistry() {
  result.steps.registry.ran = true;
  const r = await runScript(SYNC_REGISTRY, ['--force']);
  result.steps.registry.ok = r.ok;
  result.steps.registry.durationMs = r.durationMs;
  if (!r.ok) {
    result.errors.push(`registry: ${r.error || `exit ${r.status}`}`);
    if (r.stderr) result.warnings.push(`registry stderr: ${r.stderr.slice(0, 500)}`);
  }
}

// ---------------------------------------------------------------------------
// Step 5 — Update detect cache (sync-detect with cache write)
// ---------------------------------------------------------------------------

async function updateCache() {
  result.steps.cache.ran = true;
  const r = await runScript(SYNC_DETECT, []);
  result.steps.cache.ok = r.ok;
  result.steps.cache.durationMs = r.durationMs;
  if (!r.ok) {
    result.errors.push(`cache: ${r.error || `exit ${r.status}`}`);
  }
}

// ---------------------------------------------------------------------------
// Step 6 — Validate skills (--factual)
// ---------------------------------------------------------------------------

async function validateSkills() {
  const mode = (process.env.MUSTARD_SKILL_VALIDATE_MODE || 'strict').toLowerCase();
  result.steps.skills.mode = mode;

  if (mode === 'off') {
    result.steps.skills.ran = false;
    result.steps.skills.ok = true;
    return;
  }

  result.steps.skills.ran = true;
  const r = await runScript(SKILL_VALIDATE, ['--factual']);
  result.steps.skills.durationMs = r.durationMs;

  if (mode === 'warn') {
    result.steps.skills.ok = true;
    if (!r.ok) {
      result.warnings.push(`skill-validate (warn mode): exit ${r.status}`);
      if (r.stdout) result.warnings.push(`skill-validate stdout: ${r.stdout.slice(0, 800)}`);
    }
  } else {
    result.steps.skills.ok = r.ok;
    if (!r.ok) {
      result.errors.push(`skill-validate (strict): exit ${r.status}`);
      if (r.stdout) result.errors.push(`skill-validate stdout: ${r.stdout.slice(0, 800)}`);
    }
  }
}

// ---------------------------------------------------------------------------
// Security scan
// ---------------------------------------------------------------------------

async function runSecurity() {
  if (SKIP_SECURITY) return;

  result.steps.security.ran = true;
  const r = await runScript(SECURITY_SCAN, [ROOT, '--json']);
  result.steps.security.durationMs = r.durationMs;

  // security-scan exit 0 = clean, exit 1 = findings present (still useful)
  if (r.status === 0 || r.status === 1) {
    result.steps.security.ok = true;
    try {
      const parsed = JSON.parse(r.stdout || '{}');
      const findings = (parsed.findings || []).length;
      result.steps.security.findings = findings;
      if (findings > 0) {
        const critical = (parsed.findings || []).filter(f => f.severity === 'CRITICAL').length;
        if (critical > 0) {
          result.warnings.push(`security: ${critical} CRITICAL finding(s) — review before commit`);
        }
      }
    } catch {
      result.warnings.push('security: could not parse JSON output');
    }
  } else {
    result.steps.security.ok = false;
    result.warnings.push(`security: unexpected exit ${r.status}`);
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Dispatch verification — checks each subproject the orchestrator dispatched
// produced either at least one SKILL.md or the explicit _no-patterns.md marker
// (per the HARD CONTRACT in agent-prompt.template.md). Surfaces a warning per
// subproject that returned empty so the user sees it without waiting for the
// LLM orchestrator to detect via shell.
// ---------------------------------------------------------------------------

function verifyDispatch() {
  let state;
  try {
    state = JSON.parse(fs.readFileSync(DISPATCH_STATE, 'utf-8'));
  } catch {
    return; // no dispatch state — incremental run with nothing dispatched
  }
  if (!state || !Array.isArray(state.dispatch) || state.dispatch.length === 0) return;

  result.steps.dispatchVerify.ran = true;
  let anyEmpty = false;

  for (const sub of state.dispatch) {
    const skillsDir = path.join(sub.absSubprojectPath, '.claude', 'skills');
    const verdict = { name: sub.name, skillsDir, status: 'unknown', skillsWritten: 0, hasNoPatternsMarker: false };

    try {
      if (!fs.existsSync(skillsDir)) {
        verdict.status = 'missing-dir';
      } else {
        const entries = fs.readdirSync(skillsDir, { withFileTypes: true });
        let skillCount = 0;
        let hasMarker = false;
        for (const e of entries) {
          if (e.isDirectory()) {
            if (fs.existsSync(path.join(skillsDir, e.name, 'SKILL.md'))) skillCount++;
          } else if (e.name === '_no-patterns.md') {
            hasMarker = true;
          }
        }
        verdict.skillsWritten = skillCount;
        verdict.hasNoPatternsMarker = hasMarker;
        if (skillCount > 0) verdict.status = 'skills';
        else if (hasMarker) verdict.status = 'no-patterns-marker';
        else verdict.status = 'empty';
      }
    } catch (e) {
      verdict.status = 'error';
      verdict.error = e.message;
    }

    if (verdict.status === 'empty' || verdict.status === 'missing-dir') {
      anyEmpty = true;
      result.warnings.push(
        `dispatchVerify: ${sub.name} returned empty skills/ — HARD CONTRACT violated ` +
        `(expected SKILL.md OR _no-patterns.md at ${skillsDir}). Re-dispatch needed.`
      );
    }
    result.steps.dispatchVerify.subprojects.push(verdict);
  }

  result.steps.dispatchVerify.ok = !anyEmpty;
}

async function main() {
  const start = Date.now();
  await Promise.all([
    refreshRegistry(),
    updateCache(),
    validateSkills(),
    runSecurity(),
  ]);
  // verifyDispatch is synchronous (only stat/readdir) and depends on no other
  // step — run it last so its warnings appear after registry/skill validation.
  verifyDispatch();
  result.totalDurationMs = Date.now() - start;
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

main().catch((e) => {
  result.errors.push('main: ' + (e && e.message ? e.message : String(e)));
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
  process.exit(0);
});
