#!/usr/bin/env bun
'use strict';
/**
 * Tests for templates/scripts/pipeline-summary.js
 * Run with: bun test templates/hooks/__tests__/pipeline-summary.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const SCRIPT = path.resolve(__dirname, '..', '..', 'scripts', 'pipeline-summary.js');

let tmpRoot;
let specDir;

beforeEach(() => {
  tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-summary-'));
  fs.mkdirSync(path.join(tmpRoot, '.claude', '.pipeline-states'), { recursive: true });
});

afterEach(() => {
  try { fs.rmSync(tmpRoot, { recursive: true, force: true }); } catch (_) {}
});

function writeSpec(name, contents, stateJson) {
  specDir = path.join(tmpRoot, '.claude', 'spec', 'active', name);
  fs.mkdirSync(specDir, { recursive: true });
  fs.writeFileSync(path.join(specDir, 'spec.md'), contents, 'utf8');
  if (stateJson) {
    fs.writeFileSync(
      path.join(tmpRoot, '.claude', '.pipeline-states', `${name}.json`),
      JSON.stringify(stateJson, null, 2),
      'utf8'
    );
  }
}

function runScript(extraArgs = []) {
  const args = [SCRIPT, '--spec-dir', specDir, ...extraArgs];
  const r = spawnSync('bun', args, { encoding: 'utf8', cwd: tmpRoot });
  return { code: r.status, stdout: r.stdout || '', stderr: r.stderr || '' };
}

describe('pipeline-summary script', () => {
  it('all-pass: clean output with git commit suggestions', () => {
    writeSpec('all-pass', [
      '# Feature: all-pass',
      '### Status: completed | Phase: CLOSE',
      '### Lang: en',
      '',
      '## Checklist',
      '',
      '- [x] Step one',
      '- [x] Step two',
      '',
      '## Acceptance Criteria',
      '',
      '- [x] AC-1: works — Command: `echo ok`',
      '- [x] AC-2: also works',
      '',
      '## Files',
      '',
      '- `src/foo.ts`',
      '',
    ].join('\n'));

    const r = runScript();
    assert.equal(r.code, 0, `expected exit 0, got ${r.code}; stderr=${r.stderr}`);
    assert.ok(r.stdout.includes("## What's Done"), 'has Done section');
    assert.ok(r.stdout.includes("## What's Left"), 'has Left section');
    assert.ok(r.stdout.includes('Nothing pending.'), 'Left is empty -> nothing pending');
    assert.ok(r.stdout.includes('## Next Steps'), 'has Next Steps');
    assert.ok(/git commit/i.test(r.stdout), 'suggests git commit on happy path');
    assert.ok(/AC passed: 2\/2/.test(r.stdout), 'AC counter correct');
  });

  it('AC-failed: failing AC and command appear in Left', () => {
    writeSpec('ac-failed', [
      '# Feature: ac-failed',
      '### Status: implementing | Phase: EXECUTE',
      '### Lang: en',
      '',
      '## Acceptance Criteria',
      '',
      '- [x] AC-1: ok',
      '- [ ] AC-2: broken — Command: `bun test broken.test.js`',
      '- [x] AC-3: fine',
      '',
    ].join('\n'));

    const r = runScript();
    assert.equal(r.code, 0, `stderr=${r.stderr}`);
    assert.ok(r.stdout.includes("## What's Left"));
    assert.ok(r.stdout.includes('AC-2: broken'), 'failing AC listed');
    assert.ok(r.stdout.includes('`bun test broken.test.js`'), 'command preserved');
    assert.ok(/Rerun failing AC/i.test(r.stdout), 'top next step is rerun');
  });

  it('concerns: bullets surface under Left', () => {
    writeSpec('concerns', [
      '# Bugfix: concerns',
      '### Status: implementing | Phase: EXECUTE',
      '### Lang: en',
      '',
      '## Acceptance Criteria',
      '',
      '- [x] AC-1: ok',
      '',
      '## Concerns',
      '',
      '- Logging library mismatch between subprojects',
      '- Snapshot test brittle on Windows',
      '',
    ].join('\n'));

    const r = runScript();
    assert.equal(r.code, 0);
    assert.ok(r.stdout.includes('Logging library mismatch'));
    assert.ok(r.stdout.includes('Snapshot test brittle'));
    assert.ok(r.stdout.includes('Concern:'));
  });

  it('lang pt vs en: labels differ', () => {
    const body = [
      '# Feature: i18n',
      '### Status: completed | Phase: CLOSE',
      '### Lang: %LANG%',
      '',
      '## Acceptance Criteria',
      '',
      '- [x] AC-1: ok',
      '',
    ];

    writeSpec('lang-pt', body.join('\n').replace('%LANG%', 'pt'));
    const pt = runScript();
    assert.equal(pt.code, 0);
    assert.ok(pt.stdout.includes('## Feito'), 'pt: Feito');
    assert.ok(pt.stdout.includes('## Falta'), 'pt: Falta');
    assert.ok(pt.stdout.includes('## Próximos Passos'), 'pt: Próximos');
    assert.ok(!pt.stdout.includes("What's Done"), 'pt should not have en labels');

    writeSpec('lang-en', body.join('\n').replace('%LANG%', 'en'));
    const en = runScript();
    assert.equal(en.code, 0);
    assert.ok(en.stdout.includes("## What's Done"), 'en: Done');
    assert.ok(en.stdout.includes("## What's Left"), 'en: Left');
    assert.ok(!en.stdout.includes('## Feito'), 'en should not have pt labels');
  });

  it('file heuristics: migration + env produce follow-ups', () => {
    writeSpec('files-heur', [
      '# Feature: heur',
      '### Status: completed | Phase: CLOSE',
      '### Lang: en',
      '',
      '## Acceptance Criteria',
      '',
      '- [x] AC-1: ok',
      '',
      '## Files',
      '',
      '- `db/migration_001.sql`',
      '- `apps/web/.env.example`',
      '',
    ].join('\n'));

    const r = runScript();
    assert.equal(r.code, 0, `stderr=${r.stderr}`);
    assert.ok(r.stdout.includes('## Manual Follow-ups'), 'has follow-ups header');
    assert.ok(/migration/i.test(r.stdout), 'mentions migration follow-up');
    assert.ok(/env/i.test(r.stdout), 'mentions env follow-up');
  });

  it('exits non-zero without --spec-dir', () => {
    const r = spawnSync('bun', [SCRIPT], { encoding: 'utf8', cwd: tmpRoot });
    assert.notEqual(r.status, 0, 'expected non-zero exit without --spec-dir');
  });

  it('--format json emits structured object', () => {
    writeSpec('json-out', [
      '# Feature: json',
      '### Status: completed | Phase: CLOSE',
      '### Lang: en',
      '',
      '## Acceptance Criteria',
      '',
      '- [x] AC-1: ok',
      '',
    ].join('\n'));
    const r = runScript(['--format', 'json']);
    assert.equal(r.code, 0);
    const obj = JSON.parse(r.stdout);
    assert.ok(Array.isArray(obj.done));
    assert.ok(Array.isArray(obj.left));
    assert.ok(Array.isArray(obj.nextSteps));
    assert.ok(Array.isArray(obj.followUps));
  });
});
