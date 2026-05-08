#!/usr/bin/env node
'use strict';

/**
 * scan/finalize.js
 *
 * Post-dispatch finalization for /scan. Runs after all Task agents return.
 * Refreshes the entity registry, updates the detect cache, validates
 * generated skills, and runs the security scan.
 *
 * Contract:
 *   stdout: JSON { steps: { registry, cache, skills, security }, errors, warnings }
 *   exit:   always 0 (fail-open). Per-step errors are reported in the JSON.
 *
 * Usage:
 *   node .claude/scripts/scan/finalize.js
 *   node .claude/scripts/scan/finalize.js --skip-security
 */

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const SCRIPTS_DIR = path.join(ROOT, '.claude', 'scripts');
const SYNC_REGISTRY = path.join(SCRIPTS_DIR, 'sync-registry.js');
const SYNC_DETECT = path.join(SCRIPTS_DIR, 'sync-detect.js');
const SKILL_VALIDATE = path.join(SCRIPTS_DIR, 'skill-validate.js');
const SECURITY_SCAN = path.join(SCRIPTS_DIR, 'security-scan.js');

const argv = process.argv.slice(2);
const SKIP_SECURITY = argv.includes('--skip-security');

const result = {
  steps: {
    registry: { ran: false, ok: null, durationMs: null },
    cache: { ran: false, ok: null, durationMs: null },
    skills: { ran: false, ok: null, durationMs: null, mode: null },
    security: { ran: false, ok: null, durationMs: null, findings: 0 },
  },
  errors: [],
  warnings: [],
};

function existsSafe(p) {
  try { return fs.existsSync(p); } catch { return false; }
}

function runScript(scriptPath, args, opts = {}) {
  if (!existsSafe(scriptPath)) {
    return { ok: false, error: `script not found: ${path.relative(ROOT, scriptPath)}`, durationMs: 0 };
  }
  const start = Date.now();
  const res = spawnSync(process.execPath, [scriptPath, ...args], {
    encoding: 'utf-8',
    cwd: ROOT,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, ...(opts.env || {}) },
  });
  const durationMs = Date.now() - start;
  return {
    ok: res.status === 0,
    status: res.status,
    stdout: res.stdout || '',
    stderr: res.stderr || '',
    durationMs,
  };
}

// ---------------------------------------------------------------------------
// Step 4.7 — Refresh entity registry
// ---------------------------------------------------------------------------

function refreshRegistry() {
  result.steps.registry.ran = true;
  const r = runScript(SYNC_REGISTRY, ['--force']);
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

function updateCache() {
  result.steps.cache.ran = true;
  const r = runScript(SYNC_DETECT, []);
  result.steps.cache.ok = r.ok;
  result.steps.cache.durationMs = r.durationMs;
  if (!r.ok) {
    result.errors.push(`cache: ${r.error || `exit ${r.status}`}`);
  }
}

// ---------------------------------------------------------------------------
// Step 6 — Validate skills (--factual)
// ---------------------------------------------------------------------------

function validateSkills() {
  const mode = (process.env.MUSTARD_SKILL_VALIDATE_MODE || 'strict').toLowerCase();
  result.steps.skills.mode = mode;

  if (mode === 'off') {
    result.steps.skills.ran = false;
    result.steps.skills.ok = true;
    return;
  }

  result.steps.skills.ran = true;
  const r = runScript(SKILL_VALIDATE, ['--factual']);
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

function runSecurity() {
  if (SKIP_SECURITY) return;

  result.steps.security.ran = true;
  const r = runScript(SECURITY_SCAN, [ROOT, '--json']);
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

function main() {
  refreshRegistry();
  updateCache();
  validateSkills();
  runSecurity();

  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

main();
