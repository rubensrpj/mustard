#!/usr/bin/env bun
'use strict';
/**
 * Harness Wave 10 — Dev/QA Contract Tests
 *
 * Covers:
 * 1.  qa-run.js parses AC from spec markdown correctly
 * 2.  qa-run.js with AC pass → qa.result event overall=pass
 * 3.  qa-run.js with AC fail → overall=fail, criteria marks which failed
 * 4.  qa-run.js with no Acceptance Criteria section → skip with warning
 * 5.  qa-run.js with AC section but no parseable items → skip with warning
 * 6.  close-gate blocks CLOSE when no qa.result event exists (strict mode)
 * 7.  close-gate blocks CLOSE when qa.result overall=fail (strict mode)
 * 8.  close-gate allows CLOSE when qa.result overall=pass (strict mode)
 * 9.  MUSTARD_QA_GATE_MODE=warn + no QA → allow with stderr
 * 10. MUSTARD_QA_GATE_MODE=off → skip QA check entirely
 *
 * Run with: bun test templates/hooks/__tests__/harness-wave10.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const HOOKS_DIR = path.resolve(__dirname, '..');
const SCRIPTS_DIR = path.resolve(__dirname, '..', '..', 'scripts');
const CLOSE_GATE = path.join(HOOKS_DIR, 'close-gate.js');
const QA_RUN = path.join(SCRIPTS_DIR, 'qa-run.js');

const IS_WIN = process.platform === 'win32';
const EXIT_PASS = 'node -e "process.exit(0)"';
const EXIT_FAIL = 'node -e "process.exit(1)"';

// ── Helpers ───────────────────────────────────────────────────────────────────

function makeProjectDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-w10-'));
  fs.mkdirSync(path.join(dir, '.claude', '.harness', 'sessions'), { recursive: true });
  fs.mkdirSync(path.join(dir, '.claude', '.pipeline-states'), { recursive: true });
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

function writeMustardJson(projectDir, commands = {}) {
  const cfg = { git: { flow: {}, provider: 'github', submodules: false }, ...commands };
  fs.writeFileSync(path.join(projectDir, 'mustard.json'), JSON.stringify(cfg, null, 2), 'utf8');
}

function writeQAResultEvent(projectDir, specName, overall, criteria = []) {
  const eventsFile = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  const event = {
    v: 1,
    ts: new Date().toISOString(),
    sessionId: 's-test',
    wave: 0,
    actor: { kind: 'script', id: 'qa-run' },
    event: 'qa.result',
    payload: { spec: specName, overall, criteria },
  };
  fs.appendFileSync(eventsFile, JSON.stringify(event) + '\n', 'utf8');
}

function makePipelineStateInput(projectDir, specName, phase, extraFields = {}) {
  const content = JSON.stringify({ spec: specName, specName, phaseName: phase, phase: 99, ...extraFields });
  const filePath = path.join(projectDir, '.claude', '.pipeline-states', specName + '.json');
  return {
    tool: 'Write',
    tool_input: { file_path: filePath, content },
    cwd: projectDir,
  };
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

function runHook(hookPath, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const projectDir = opts.projectDir || os.tmpdir();
    const env = Object.assign({}, process.env);
    if (opts.env) Object.assign(env, opts.env);

    const child = spawn(process.execPath, [hookPath], {
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
      const response = parsed && parsed.hookSpecificOutput ? parsed.hookSpecificOutput : null;
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed, response });
    });
    child.stdin.write(JSON.stringify(inputObj));
    child.stdin.end();
  });
}

function readEvents(projectDir) {
  const f = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
  if (!fs.existsSync(f)) return [];
  return fs.readFileSync(f, 'utf8')
    .split('\n').filter(Boolean)
    .map(l => { try { return JSON.parse(l); } catch (_) { return null; } })
    .filter(Boolean);
}

// ── Sample spec with AC ───────────────────────────────────────────────────────

function makeSpecWithAC(passCmd, failCmd) {
  return `# Feature: test-feature
### Status: implementing | Phase: EXECUTE | Scope: light

## Summary
Test feature for Wave 10.

## Checklist
- [x] Implement feature

## Acceptance Criteria

Testable, binary (pass/fail) criteria.

- [ ] AC-1: Build succeeds — Command: \`${passCmd}\`
- [ ] AC-2: Linting passes — Command: \`${failCmd || passCmd}\`
`;
}

// PT-language spec: "## Tarefas" + "## Critérios de Aceitação" headings.
// AC item lines stay English per the spec-language hard rule.
function makeSpecWithACPt(passCmd) {
  return `# Feature: teste-recurso
### Status: implementing | Phase: EXECUTE | Scope: light
### Lang: pt

## Resumo
Recurso de teste para a Wave 10.

## Tarefas
- [x] Implementar recurso

## Critérios de Aceitação

Critérios testáveis e binários (pass/fail).

- [ ] AC-1: Build succeeds — Command: \`${passCmd}\`
- [ ] AC-2: Linting passes — Command: \`${passCmd}\`
`;
}

function makeSpecNoAC() {
  return `# Feature: no-ac
### Status: implementing | Phase: EXECUTE

## Summary
Feature without AC.

## Checklist
- [x] Implement
`;
}

function makeSpecACNoItems() {
  return `# Feature: ac-empty
## Acceptance Criteria

This section exists but has no parseable items.

Some unformatted text here.
`;
}

// ── Test 1: qa-run.js parses AC correctly ─────────────────────────────────────

describe('Wave 10 — qa-run: parses AC items from spec markdown', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('parses AC-1 and AC-2 with commands from spec', async () => {
    // We test the --json flag output which shows criteria
    writeSpec(tmp, 'parse-test', makeSpecWithAC(EXIT_PASS, EXIT_PASS));

    const result = await runScript(QA_RUN, ['--spec', 'parse-test', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    assert.equal(result.code, 0, `qa-run should exit 0 when all AC pass, stderr: ${result.stderr}`);
    assert.ok(result.parsed, `expected JSON output, stdout: ${result.stdout}`);
    assert.equal(result.parsed.payload.spec, 'parse-test');
    assert.ok(Array.isArray(result.parsed.payload.criteria), 'criteria must be array');
    assert.ok(result.parsed.payload.criteria.length >= 2,
      `expected ≥2 criteria, got: ${result.parsed.payload.criteria.length}`);
    const ids = result.parsed.payload.criteria.map(c => c.id);
    assert.ok(ids.includes('AC-1'), `AC-1 must be parsed, ids: ${ids}`);
    assert.ok(ids.includes('AC-2'), `AC-2 must be parsed, ids: ${ids}`);
  });
});

// ── Test 1b: qa-run.js parses PT "## Critérios de Aceitação" + "## Tarefas" ───

describe('Wave 10 — qa-run: parses AC from a PT-language spec', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('recognizes "## Critérios de Aceitação" and runs AC items', async () => {
    writeSpec(tmp, 'pt-ac-test', makeSpecWithACPt(EXIT_PASS));

    const result = await runScript(QA_RUN, ['--spec', 'pt-ac-test', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    assert.equal(result.code, 0, `qa-run should exit 0 when all AC pass, stderr: ${result.stderr}`);
    assert.ok(result.parsed, `expected JSON output, stdout: ${result.stdout}`);
    // A regression in PT heading recognition would surface as overall=skip
    // (no Acceptance Criteria section found).
    assert.notEqual(result.parsed.payload.overall, 'skip',
      'PT "## Critérios de Aceitação" must be recognized as the AC section');
    const ids = result.parsed.payload.criteria.map(c => c.id);
    assert.ok(ids.includes('AC-1') && ids.includes('AC-2'),
      `AC-1 and AC-2 must be parsed from PT spec, ids: ${ids}`);
  });
});

// ── Test 2: qa-run.js with all AC pass ───────────────────────────────────────

describe('Wave 10 — qa-run: all AC pass → qa.result event overall=pass', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('emits qa.result with overall=pass when all commands exit 0', async () => {
    writeSpec(tmp, 'pass-spec', makeSpecWithAC(EXIT_PASS, EXIT_PASS));

    const result = await runScript(QA_RUN, ['--spec', 'pass-spec', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    assert.equal(result.code, 0, `should exit 0 on pass, stderr: ${result.stderr}`);
    assert.ok(result.parsed, 'expected JSON output');
    assert.equal(result.parsed.payload.overall, 'pass',
      `expected overall=pass, got: ${result.parsed.payload.overall}`);

    // Verify harness event was emitted
    const events = readEvents(tmp);
    const qaEvent = events.find(e => e.event === 'qa.result');
    assert.ok(qaEvent, `expected qa.result event in harness log, events: ${JSON.stringify(events.map(e => e.event))}`);
    assert.equal(qaEvent.payload.overall, 'pass', `expected qa event overall=pass`);
  });

  it('writes sidecar .qa-reports/{spec}.json', async () => {
    writeSpec(tmp, 'sidecar-spec', makeSpecWithAC(EXIT_PASS, EXIT_PASS));

    await runScript(QA_RUN, ['--spec', 'sidecar-spec', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    const reportPath = path.join(tmp, '.claude', '.qa-reports', 'sidecar-spec.json');
    assert.ok(fs.existsSync(reportPath), `expected sidecar report at ${reportPath}`);
    const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'));
    assert.equal(report.overall, 'pass');
  });
});

// ── Test 3: qa-run.js with AC fail ───────────────────────────────────────────

describe('Wave 10 — qa-run: AC fail → overall=fail, criteria marks which failed', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('returns overall=fail and marks the failing criterion', async () => {
    // AC-1 passes, AC-2 fails
    writeSpec(tmp, 'fail-spec', makeSpecWithAC(EXIT_PASS, EXIT_FAIL));

    const result = await runScript(QA_RUN, ['--spec', 'fail-spec', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    // CLI exits 1 on fail
    assert.equal(result.code, 1, `should exit 1 when AC fails, code: ${result.code}`);
    assert.ok(result.parsed, `expected JSON output, stdout: ${result.stdout}`);
    assert.equal(result.parsed.payload.overall, 'fail',
      `expected overall=fail, got: ${result.parsed.payload.overall}`);

    const criteria = result.parsed.payload.criteria;
    assert.ok(Array.isArray(criteria), 'criteria must be array');

    const ac1 = criteria.find(c => c.id === 'AC-1');
    const ac2 = criteria.find(c => c.id === 'AC-2');
    assert.ok(ac1, 'AC-1 must be present');
    assert.ok(ac2, 'AC-2 must be present');
    assert.equal(ac1.status, 'pass', `AC-1 should pass, got: ${ac1.status}`);
    assert.equal(ac2.status, 'fail', `AC-2 should fail, got: ${ac2.status}`);
  });

  it('harness event also reflects overall=fail', async () => {
    writeSpec(tmp, 'fail-event-spec', makeSpecWithAC(EXIT_PASS, EXIT_FAIL));

    await runScript(QA_RUN, ['--spec', 'fail-event-spec', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    const events = readEvents(tmp);
    const qaEvent = events.find(e => e.event === 'qa.result');
    assert.ok(qaEvent, 'expected qa.result event');
    assert.equal(qaEvent.payload.overall, 'fail', `expected fail, got: ${qaEvent.payload.overall}`);
  });
});

// ── Test 4: qa-run.js no AC section → skip ───────────────────────────────────

describe('Wave 10 — qa-run: no Acceptance Criteria section → skip with warning', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('returns overall=skip and prints warning to stderr', async () => {
    writeSpec(tmp, 'no-ac-spec', makeSpecNoAC());

    const result = await runScript(QA_RUN, ['--spec', 'no-ac-spec', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    // Skip exits 0 (not a blocker)
    assert.equal(result.code, 0, `should exit 0 for skip, stderr: ${result.stderr}`);
    assert.ok(result.stderr.includes('[qa-run]'),
      `expected [qa-run] warning in stderr, got: ${result.stderr}`);

    // Output may be JSON or plain text — check for skip
    if (result.parsed) {
      assert.equal(result.parsed.payload.overall, 'skip',
        `expected overall=skip, got: ${result.parsed.payload.overall}`);
    } else {
      // Non-JSON output (text mode) — check for SKIP keyword
      assert.ok(result.stdout.includes('SKIP') || result.stderr.includes('SKIP') ||
        result.stderr.includes('No') || result.stderr.includes('no'),
        `expected skip indicator, stdout: ${result.stdout}, stderr: ${result.stderr}`);
    }
  });
});

// ── Test 5: qa-run.js AC section but no parseable items → skip ───────────────

describe('Wave 10 — qa-run: AC section exists but no parseable items → skip', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('returns overall=skip when AC section has no valid format items', async () => {
    writeSpec(tmp, 'ac-empty-spec', makeSpecACNoItems());

    const result = await runScript(QA_RUN, ['--spec', 'ac-empty-spec', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    assert.equal(result.code, 0, `should exit 0 for skip, stderr: ${result.stderr}`);
    assert.ok(
      result.stderr.includes('[qa-run]') || result.stdout.includes('skip') || result.stdout.includes('SKIP'),
      `expected skip behavior, stdout: ${result.stdout}, stderr: ${result.stderr}`
    );
  });
});

// Tests 6-10 (the close-gate QA gate) were ported to the Rust `close_gate`
// module (b3 Wave 4); their parity tests now live in
// `packages/rt/src/hooks/close_gate.rs`.

// ── Regression: qa-run recognizes the Portuguese AC heading ──────────────────

describe('Wave 10 — qa-run regression: PT Acceptance-Criteria heading', () => {
  let tmp;
  beforeEach(() => { tmp = makeProjectDir(); });
  afterEach(() => { cleanDir(tmp); });

  it('Portuguese heading "## Critérios de Aceitação" is recognized → overall=pass', async () => {
    // Regression: pt-language specs use "Critérios de Aceitação" per spec-language HARD RULE.
    // extractACSection must match both the English and Portuguese canonical headings.
    const ptSpec = `# Feature: pt-spec
### Status: implementing | Phase: EXECUTE | Scope: light

## Resumo
Spec em português para testar heading pt.

## Critérios de Aceitação

- [ ] AC-1: Build OK — Command: \`${EXIT_PASS}\`
`;
    writeSpec(tmp, 'pt-ac-spec', ptSpec);

    const result = await runScript(QA_RUN, ['--spec', 'pt-ac-spec', '--json'], {
      projectDir: tmp,
      env: { MUSTARD_DISABLED_HOOKS: 'all' },
    });

    assert.equal(result.code, 0, `qa-run should exit 0 (pass), stderr: ${result.stderr}`);
    assert.ok(result.parsed, `expected JSON output, stdout: ${result.stdout}`);
    assert.equal(result.parsed.payload.overall, 'pass',
      `pt heading must be recognized — expected pass, got: ${result.parsed.payload.overall}`);
    assert.ok(Array.isArray(result.parsed.payload.criteria) && result.parsed.payload.criteria.length >= 1,
      `expected ≥1 criterion, got: ${JSON.stringify(result.parsed.payload.criteria)}`);
    assert.equal(result.parsed.payload.criteria[0].id, 'AC-1');
    assert.equal(result.parsed.payload.criteria[0].status, 'pass');
  });
});
