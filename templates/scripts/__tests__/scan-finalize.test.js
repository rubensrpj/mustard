'use strict';
// Tests for templates/scripts/scan/finalize.js.
// Verifies that each post-dispatch step runs, fails open, and reports its
// outcome in the JSON output.

const { test } = require('bun:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const TEMPLATES_DIR = path.resolve(__dirname, '..', '..');
const SCRIPTS_SRC = path.join(TEMPLATES_DIR, 'scripts');

function mkProject(opts = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-finalize-'));
  const claudeDir = path.join(root, '.claude');
  const scriptsDir = path.join(claudeDir, 'scripts');
  const scanDir = path.join(scriptsDir, 'scan');
  fs.mkdirSync(scanDir, { recursive: true });

  // Copy real finalize.js
  fs.copyFileSync(
    path.join(SCRIPTS_SRC, 'scan', 'finalize.js'),
    path.join(scanDir, 'finalize.js')
  );

  // Stub the four child scripts
  const stubExit = (code) => `#!/usr/bin/env node
process.stdout.write(${JSON.stringify(opts.stubStdout || '{}')} + '\\n');
process.exit(${code});
`;
  fs.writeFileSync(path.join(scriptsDir, 'sync-registry.js'), stubExit(opts.registryExit ?? 0), 'utf-8');
  fs.writeFileSync(path.join(scriptsDir, 'sync-detect.js'), stubExit(opts.detectExit ?? 0), 'utf-8');
  fs.writeFileSync(path.join(scriptsDir, 'skill-validate.js'), stubExit(opts.validateExit ?? 0), 'utf-8');

  // Security-scan stub: prints structured JSON
  const secStub = `#!/usr/bin/env node
process.stdout.write(${JSON.stringify(JSON.stringify(opts.securityOutput || { findings: [] }))} + '\\n');
process.exit(${opts.securityExit ?? 0});
`;
  fs.writeFileSync(path.join(scriptsDir, 'security-scan.js'), secStub, 'utf-8');

  return root;
}

function runFinalize(root, args = [], env = {}) {
  const script = path.join(root, '.claude', 'scripts', 'scan', 'finalize.js');
  return spawnSync(process.execPath, [script, ...args], {
    encoding: 'utf-8',
    cwd: root,
    env: { ...process.env, ...env },
  });
}

function parseStdout(res) {
  if (res.status !== 0) {
    throw new Error(`exit ${res.status}; stderr: ${res.stderr}`);
  }
  return JSON.parse(res.stdout);
}

// ---------------------------------------------------------------------------
// Happy path — all steps succeed
// ---------------------------------------------------------------------------

test('happy path: all four steps run and report ok', () => {
  const root = mkProject({});
  const out = parseStdout(runFinalize(root));

  assert.equal(out.steps.registry.ran, true);
  assert.equal(out.steps.registry.ok, true);
  assert.equal(out.steps.cache.ran, true);
  assert.equal(out.steps.cache.ok, true);
  assert.equal(out.steps.skills.ran, true);
  assert.equal(out.steps.skills.ok, true);
  assert.equal(out.steps.security.ran, true);
  assert.equal(out.steps.security.ok, true);
  assert.equal(out.errors.length, 0);
});

// ---------------------------------------------------------------------------
// Strict skill-validate failure
// ---------------------------------------------------------------------------

test('strict skill-validate failure surfaces in errors[]', () => {
  const root = mkProject({ validateExit: 1 });
  const out = parseStdout(runFinalize(root, [], { MUSTARD_SKILL_VALIDATE_MODE: 'strict' }));

  assert.equal(out.steps.skills.ok, false);
  assert.ok(out.errors.some(e => /skill-validate/.test(e)));
});

test('warn mode: skill-validate failure goes to warnings, not errors', () => {
  const root = mkProject({ validateExit: 1 });
  const out = parseStdout(runFinalize(root, [], { MUSTARD_SKILL_VALIDATE_MODE: 'warn' }));

  assert.equal(out.steps.skills.ok, true);
  assert.ok(out.warnings.some(w => /skill-validate/.test(w)));
  assert.equal(out.errors.length, 0);
});

test('off mode: skill-validate is skipped entirely', () => {
  const root = mkProject({ validateExit: 1 });
  const out = parseStdout(runFinalize(root, [], { MUSTARD_SKILL_VALIDATE_MODE: 'off' }));

  assert.equal(out.steps.skills.ran, false);
  assert.equal(out.steps.skills.ok, true);
});

// ---------------------------------------------------------------------------
// Security scan
// ---------------------------------------------------------------------------

test('security: findings counted and CRITICAL surfaces a warning', () => {
  const root = mkProject({
    securityExit: 1,
    securityOutput: { findings: [{ severity: 'CRITICAL', type: 'Secret' }, { severity: 'WARNING', type: 'Env' }] },
  });
  const out = parseStdout(runFinalize(root));

  assert.equal(out.steps.security.ok, true, 'exit 1 with findings is normal — still ok=true');
  assert.equal(out.steps.security.findings, 2);
  assert.ok(out.warnings.some(w => /CRITICAL/.test(w)));
});

test('--skip-security skips the security step', () => {
  const root = mkProject({});
  const out = parseStdout(runFinalize(root, ['--skip-security']));

  assert.equal(out.steps.security.ran, false);
});

// ---------------------------------------------------------------------------
// Fail-open: missing scripts
// ---------------------------------------------------------------------------

test('fail-open: missing sync-registry reports error but exits 0', () => {
  const root = mkProject({});
  fs.unlinkSync(path.join(root, '.claude', 'scripts', 'sync-registry.js'));

  const res = runFinalize(root);
  assert.equal(res.status, 0, 'fail-open');
  const out = JSON.parse(res.stdout);
  assert.ok(out.errors.some(e => /sync-registry/.test(e)));
});

// ---------------------------------------------------------------------------
// dispatchVerify — names of skills on disk (source of truth for skills_generated)
// ---------------------------------------------------------------------------

test('dispatchVerify: skills[] lists actual subdir names with SKILL.md (not just count)', () => {
  const root = mkProject({});

  // Stage a subproject with two real skills + one user-authored noise dir without SKILL.md.
  const sub = path.join(root, 'apps', 'mySub');
  const skillsDir = path.join(sub, '.claude', 'skills');
  fs.mkdirSync(path.join(skillsDir, 'auth-primitive-pattern'), { recursive: true });
  fs.mkdirSync(path.join(skillsDir, 'route-handler-pattern'), { recursive: true });
  fs.mkdirSync(path.join(skillsDir, 'no-skill-md-dir'), { recursive: true });
  fs.writeFileSync(path.join(skillsDir, 'auth-primitive-pattern', 'SKILL.md'), '---\nname: auth-primitive-pattern\n---\n');
  fs.writeFileSync(path.join(skillsDir, 'route-handler-pattern', 'SKILL.md'), '---\nname: route-handler-pattern\n---\n');

  // Write the dispatch-state file that finalize.js reads
  fs.writeFileSync(path.join(root, '.claude', '.scan-dispatch.json'), JSON.stringify({
    ts: new Date().toISOString(),
    dispatch: [{ name: 'mySub', path: 'apps/mySub', absSubprojectPath: sub.split(path.sep).join('/') }],
  }));

  const out = parseStdout(runFinalize(root));

  assert.equal(out.steps.dispatchVerify.ran, true);
  assert.equal(out.steps.dispatchVerify.ok, true);
  assert.equal(out.steps.dispatchVerify.subprojects.length, 1);

  const verdict = out.steps.dispatchVerify.subprojects[0];
  assert.equal(verdict.name, 'mySub');
  assert.equal(verdict.status, 'skills');
  assert.equal(verdict.skillsWritten, 2, 'count matches array length');
  assert.deepEqual(verdict.skills, ['auth-primitive-pattern', 'route-handler-pattern'],
    'skills[] holds the actual subdir names from disk, sorted');
});

test('dispatchVerify: empty skills/ surfaces status=empty with skills=[]', () => {
  const root = mkProject({});
  const sub = path.join(root, 'apps', 'emptySub');
  fs.mkdirSync(path.join(sub, '.claude', 'skills'), { recursive: true });

  fs.writeFileSync(path.join(root, '.claude', '.scan-dispatch.json'), JSON.stringify({
    ts: new Date().toISOString(),
    dispatch: [{ name: 'emptySub', path: 'apps/emptySub', absSubprojectPath: sub.split(path.sep).join('/') }],
  }));

  const out = parseStdout(runFinalize(root));
  const verdict = out.steps.dispatchVerify.subprojects[0];
  assert.equal(verdict.status, 'empty');
  assert.equal(verdict.skillsWritten, 0);
  assert.deepEqual(verdict.skills, []);
  assert.equal(out.steps.dispatchVerify.ok, false, 'empty dispatch violates HARD CONTRACT');
});

test('dispatchVerify: _no-patterns.md marker satisfies contract with skills=[]', () => {
  const root = mkProject({});
  const sub = path.join(root, 'apps', 'markerSub');
  const skillsDir = path.join(sub, '.claude', 'skills');
  fs.mkdirSync(skillsDir, { recursive: true });
  fs.writeFileSync(path.join(skillsDir, '_no-patterns.md'), '<!-- mustard:generated -->\n# No patterns\n');

  fs.writeFileSync(path.join(root, '.claude', '.scan-dispatch.json'), JSON.stringify({
    ts: new Date().toISOString(),
    dispatch: [{ name: 'markerSub', path: 'apps/markerSub', absSubprojectPath: sub.split(path.sep).join('/') }],
  }));

  const out = parseStdout(runFinalize(root));
  const verdict = out.steps.dispatchVerify.subprojects[0];
  assert.equal(verdict.status, 'no-patterns-marker');
  assert.equal(verdict.hasNoPatternsMarker, true);
  assert.deepEqual(verdict.skills, []);
  assert.equal(out.steps.dispatchVerify.ok, true, 'marker satisfies contract');
});
