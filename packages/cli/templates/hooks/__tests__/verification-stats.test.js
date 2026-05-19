#!/usr/bin/env bun
'use strict';
/**
 * verification-stats — Wave 2 contract tests
 *
 * Covers:
 * 1. qa-run.js emits a `qa` hook metric with note=overall
 * 2. review-result.js emits a `review` hook metric for approved and rejected
 * 3. metrics.js collect renders the "Verification (QA + Review)" panel
 *    when verification events exist
 *
 * Run with: bun test templates/hooks/__tests__/verification-stats.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const QA_RUN = path.join(SCRIPTS_DIR, 'qa-run.js');
const REVIEW_RESULT = path.join(SCRIPTS_DIR, 'review-result.js');
const METRICS = path.join(SCRIPTS_DIR, 'metrics.js');

const EXIT_PASS = 'node -e "process.exit(0)"';

// ── Helpers ───────────────────────────────────────────────────────────────────

function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-vstats-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', 'specs'), { recursive: true });
  return dir;
}

function cleanDir(dir) {
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch (_) {}
}

function writeSpec(projectDir, specName, content) {
  const specFile = path.join(projectDir, '.claude', 'specs', specName + '.md');
  fs.writeFileSync(specFile, content, 'utf8');
  return specFile;
}

function specWithAC(passCmd) {
  return `# Feature: vstats-test
### Status: implementing | Phase: EXECUTE | Scope: light

## Summary
Verification stats test.

## Acceptance Criteria

Testable, binary (pass/fail) criteria.

- [ ] AC-1: Build succeeds — Command: \`${passCmd}\`
`;
}

function runScript(scriptPath, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const env = Object.assign({}, process.env, { MUSTARD_DISABLED_HOOKS: 'all' });
    if (opts.env) Object.assign(env, opts.env);

    const child = spawn(process.execPath, [scriptPath, ...args], {
      cwd: projectDir,
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', d => (stdout += d));
    child.stderr.on('data', d => (stderr += d));
    child.on('error', reject);
    child.on('close', code => {
      let parsed = null;
      try { parsed = JSON.parse(stdout.trim()); } catch (_) {}
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });
    child.stdin.end();
  });
}

function readMetricLines(projectDir, event) {
  const f = path.join(projectDir, '.claude', '.metrics', event + '.jsonl');
  if (!fs.existsSync(f)) return [];
  return fs.readFileSync(f, 'utf8')
    .split('\n').filter(Boolean)
    .map(l => { try { return JSON.parse(l); } catch (_) { return null; } })
    .filter(Boolean);
}

// ── Test 1: qa-run emits a `qa` metric with note=overall ──────────────────────

describe('verification-stats — qa-run emits a `qa` hook metric', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('appends a qa metric line with note equal to overall', async () => {
    writeSpec(tmp, 'qa-metric-spec', specWithAC(EXIT_PASS));

    const result = await runScript(QA_RUN, ['--spec', 'qa-metric-spec', '--json'], {
      projectDir: tmp,
    });

    assert.equal(result.code, 0, `qa-run should exit 0 on pass, stderr: ${result.stderr}`);

    const metrics = readMetricLines(tmp, 'qa');
    assert.ok(metrics.length >= 1, `expected a qa metric line, got: ${metrics.length}`);
    const m = metrics[metrics.length - 1];
    assert.equal(m.event, 'qa', 'metric event must be "qa"');
    assert.equal(m.note, 'pass', `note must equal overall ("pass"), got: ${m.note}`);
    assert.equal(m.note, m.overall, 'note must equal the overall field');
    assert.equal(m.category, 'verification', 'metric category must be "verification"');
  });
});

// ── Test 2: review-result emits a `review` metric for both verdicts ───────────

describe('verification-stats — review-result emits a `review` hook metric', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('emits a review metric with note=approved', async () => {
    const result = await runScript(REVIEW_RESULT, ['--spec', 'rev-spec', '--verdict', 'approved'], {
      projectDir: tmp,
    });
    assert.equal(result.code, 0, `review-result should exit 0, stderr: ${result.stderr}`);

    const metrics = readMetricLines(tmp, 'review');
    assert.ok(metrics.length >= 1, 'expected a review metric line');
    const m = metrics[metrics.length - 1];
    assert.equal(m.event, 'review', 'metric event must be "review"');
    assert.equal(m.note, 'approved', `note must be "approved", got: ${m.note}`);
    assert.equal(m.category, 'verification', 'category must be "verification"');
  });

  it('emits a review metric with note=rejected and criticalCount', async () => {
    const result = await runScript(
      REVIEW_RESULT,
      ['--spec', 'rev-spec', '--verdict', 'rejected', '--critical', '3'],
      { projectDir: tmp }
    );
    assert.equal(result.code, 0, `review-result should exit 0, stderr: ${result.stderr}`);

    const metrics = readMetricLines(tmp, 'review');
    assert.ok(metrics.length >= 1, 'expected a review metric line');
    const m = metrics[metrics.length - 1];
    assert.equal(m.note, 'rejected', `note must be "rejected", got: ${m.note}`);
    assert.equal(m.criticalCount, 3, `criticalCount must be 3, got: ${m.criticalCount}`);
  });
});

// ── Test 3: metrics.js renders the Verification panel ─────────────────────────

describe('verification-stats — metrics collect renders the Verification panel', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('shows "Verification (QA + Review)" with QA and Review lines when events exist', async () => {
    // Seed qa + review metrics by running the real producers.
    writeSpec(tmp, 'panel-spec', specWithAC(EXIT_PASS));
    await runScript(QA_RUN, ['--spec', 'panel-spec', '--json'], { projectDir: tmp });
    await runScript(REVIEW_RESULT, ['--spec', 'panel-spec', '--verdict', 'approved'], { projectDir: tmp });
    await runScript(REVIEW_RESULT, ['--spec', 'panel-spec', '--verdict', 'rejected'], { projectDir: tmp });

    const result = await runScript(METRICS, ['collect', '--hooks-only'], { projectDir: tmp });
    assert.equal(result.code, 0, `metrics collect should exit 0, stderr: ${result.stderr}`);

    assert.ok(
      result.stdout.includes('## Verification (QA + Review)'),
      `expected Verification panel heading, stdout:\n${result.stdout}`
    );
    assert.ok(result.stdout.includes('**QA**'), 'expected QA line in panel');
    assert.ok(result.stdout.includes('**Review**'), 'expected Review line in panel');
    assert.ok(result.stdout.includes('approval rate'), 'expected approval rate when verdicts exist');

    // Panel must sit before the raw hook-events table.
    const panelIdx = result.stdout.indexOf('## Verification (QA + Review)');
    const rawIdx = result.stdout.indexOf('## All Hook Events');
    assert.ok(panelIdx >= 0 && rawIdx >= 0 && panelIdx < rawIdx,
      'Verification panel must render before the raw hook-events table');
  });
});
