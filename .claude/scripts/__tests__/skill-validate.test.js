'use strict';
// Tests for .claude/scripts/skill-validate.js.
//
// Strategy:
// - Build a tiny fake project tree in a temp dir with ROOT skills + subproject
//   skills + a .detect-cache.json.
// - Point the validator at that root via a wrapper invocation (cwd + modifying
//   ROOT is not supported; we copy the script into the temp root to exercise
//   its own ROOT resolution).
// - Assert exit code + stdout summary.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const SCRIPT_SRC = path.resolve(__dirname, '..', 'skill-validate.js');

function mkTmpProject() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-skills-'));
  // Mimic the {ROOT}/.claude/scripts/skill-validate.js layout the script
  // expects (it resolves ROOT as `__dirname/../..`).
  fs.mkdirSync(path.join(root, '.claude', 'scripts'), { recursive: true });
  fs.copyFileSync(SCRIPT_SRC, path.join(root, '.claude', 'scripts', 'skill-validate.js'));
  return root;
}

function writeSkill(dir, name, opts = {}) {
  const folder = path.join(dir, name);
  fs.mkdirSync(folder, { recursive: true });
  const desc = opts.description ?? 'Pattern for use when creating entities or adding new routes across the project codebase.';
  const source = opts.source ?? 'scan';
  const body = `---
name: ${name}
description: "${desc}"
source: ${source}
---
<!-- mustard:generated -->

# ${name}
`;
  fs.writeFileSync(path.join(folder, 'SKILL.md'), body);
}

function run(root, args = []) {
  return spawnSync(process.execPath, [path.join(root, '.claude', 'scripts', 'skill-validate.js'), ...args], {
    encoding: 'utf-8',
  });
}

test('validates ROOT skills and reports ok for valid ones', () => {
  const root = mkTmpProject();
  const skillsRoot = path.join(root, '.claude', 'skills');
  fs.mkdirSync(skillsRoot, { recursive: true });
  writeSkill(skillsRoot, 'frontend-dto-conventions');
  writeSkill(skillsRoot, 'backend-entity-creation');

  const res = run(root);
  assert.equal(res.status, 0, `stderr: ${res.stderr}\nstdout: ${res.stdout}`);
  assert.match(res.stdout, /2\/2 ok/);
});

test('detects missing frontmatter and exits with 2', () => {
  const root = mkTmpProject();
  const skillsRoot = path.join(root, '.claude', 'skills');
  fs.mkdirSync(path.join(skillsRoot, 'broken'), { recursive: true });
  fs.writeFileSync(path.join(skillsRoot, 'broken', 'SKILL.md'), '# no frontmatter\n');

  const res = run(root);
  assert.equal(res.status, 2);
  assert.match(res.stdout, /\[fail\]/);
  assert.match(res.stdout, /missing YAML frontmatter/);
});

test('walks subprojects from detect cache', () => {
  const root = mkTmpProject();
  // subproject at apps/ui
  const subSkills = path.join(root, 'apps', 'ui', '.claude', 'skills');
  fs.mkdirSync(subSkills, { recursive: true });
  writeSkill(subSkills, 'ui-navigation');

  fs.writeFileSync(
    path.join(root, '.claude', '.detect-cache.json'),
    JSON.stringify({ subprojects: [{ name: 'ui', path: 'apps/ui', role: 'ui', agent: 'frontend' }] })
  );

  const res = run(root);
  assert.equal(res.status, 0, `stdout: ${res.stdout}`);
  assert.match(res.stdout, /1\/1 ok/);
});

test('--json emits machine-readable summary', () => {
  const root = mkTmpProject();
  const skillsRoot = path.join(root, '.claude', 'skills');
  fs.mkdirSync(skillsRoot, { recursive: true });
  writeSkill(skillsRoot, 'frontend-ok');
  writeSkill(skillsRoot, 'too-short', { description: 'tiny' });

  const res = run(root, ['--json']);
  const parsed = JSON.parse(res.stdout);
  assert.equal(parsed.summary.total, 2);
  assert.equal(parsed.summary.ok, 1);
  assert.equal(parsed.summary.failed, 1);
  assert.equal(res.status, 2);
});
