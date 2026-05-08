'use strict';
// Tests for templates/scripts/scan/orchestrate.js.
// Uses node:test + node:assert; spawns the orchestrator against a temp ROOT
// containing a minimal fake project layout.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const TEMPLATES_DIR = path.resolve(__dirname, '..', '..');
const SCRIPTS_SRC = path.join(TEMPLATES_DIR, 'scripts');

/**
 * Build a fake project ROOT with .claude/scripts/ scaffolded as siblings of
 * the orchestrator script. We physically copy the scripts so the resolved
 * ROOT (path.resolve(__dirname, '..', '..', '..')) points at our temp dir.
 *
 * Layout produced:
 *   <root>/CLAUDE.md          (optional)
 *   <root>/.claude/CLAUDE.md  (optional)
 *   <root>/.claude/entity-registry.json (optional)
 *   <root>/.claude/scripts/sync-detect.js   (stub that prints fixture JSON)
 *   <root>/.claude/scripts/scan/orchestrate.js (real script copied)
 *   <root>/.claude/scripts/scan/agent-prompt.template.md (real template copied)
 *   <root>/<subproject>/CLAUDE.md (optional)
 */
function mkProject(opts = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-scan-'));
  const claudeDir = path.join(root, '.claude');
  const scriptsDir = path.join(claudeDir, 'scripts');
  const scanDir = path.join(scriptsDir, 'scan');
  fs.mkdirSync(scanDir, { recursive: true });

  // Copy the real orchestrator + template
  fs.copyFileSync(
    path.join(SCRIPTS_SRC, 'scan', 'orchestrate.js'),
    path.join(scanDir, 'orchestrate.js')
  );
  fs.copyFileSync(
    path.join(SCRIPTS_SRC, 'scan', 'agent-prompt.template.md'),
    path.join(scanDir, 'agent-prompt.template.md')
  );

  // Stub sync-detect.js — writes a JSON fixture to stdout
  const detectFixture = opts.detectFixture || {
    subprojects: [
      { name: 'api', path: 'api', role: 'api', agent: 'backend', commands: [], stackSummary: '.NET 9' },
    ],
    sourceHashes: { api: 'hash-v1' },
    moduleHashes: {},
    agents: ['orchestrator'],
    detectedAgents: ['backend'],
  };
  const detectStub = `#!/usr/bin/env node
process.stdout.write(${JSON.stringify(JSON.stringify(detectFixture, null, 2))} + '\\n');
`;
  fs.writeFileSync(path.join(scriptsDir, 'sync-detect.js'), detectStub, 'utf-8');

  // Optional foundation files
  if (opts.rootClaudeMd) fs.writeFileSync(path.join(root, 'CLAUDE.md'), opts.rootClaudeMd, 'utf-8');
  if (opts.orchClaudeMd) fs.writeFileSync(path.join(claudeDir, 'CLAUDE.md'), opts.orchClaudeMd, 'utf-8');
  if (opts.registry) fs.writeFileSync(path.join(claudeDir, 'entity-registry.json'), JSON.stringify(opts.registry), 'utf-8');
  if (opts.detectCache) fs.writeFileSync(path.join(claudeDir, '.detect-cache.json'), JSON.stringify(opts.detectCache, null, 2), 'utf-8');

  // Optional subproject CLAUDE.md files
  if (opts.subprojectClaudeMd) {
    for (const [name, content] of Object.entries(opts.subprojectClaudeMd)) {
      const dir = path.join(root, name);
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, 'CLAUDE.md'), content, 'utf-8');
    }
  }

  return root;
}

function runOrchestrate(root, args = []) {
  const script = path.join(root, '.claude', 'scripts', 'scan', 'orchestrate.js');
  return spawnSync(process.execPath, [script, ...args], {
    encoding: 'utf-8',
    cwd: root,
  });
}

function parseStdout(res) {
  if (res.status !== 0) {
    throw new Error(`exit ${res.status}; stderr: ${res.stderr}`);
  }
  return JSON.parse(res.stdout);
}

// ---------------------------------------------------------------------------
// Bootstrap fresh — first run, nothing exists
// ---------------------------------------------------------------------------

test('bootstrap fresh: writes orchestrator CLAUDE.md, root CLAUDE.md, registry, subproject CLAUDE.md', () => {
  const root = mkProject({});
  const res = runOrchestrate(root);
  const out = parseStdout(res);

  assert.equal(out.fastPath, false, 'fastPath should be false on first run');
  assert.ok(out.generated.includes('.claude/CLAUDE.md'), 'orchestrator CLAUDE.md generated');
  assert.ok(out.generated.includes('CLAUDE.md'), 'root CLAUDE.md generated');
  assert.ok(out.generated.includes('.claude/entity-registry.json'), 'registry generated');
  assert.ok(out.generated.includes('api/CLAUDE.md'), 'subproject CLAUDE.md generated');

  // Verify files exist on disk
  assert.ok(fs.existsSync(path.join(root, '.claude', 'CLAUDE.md')));
  assert.ok(fs.existsSync(path.join(root, 'CLAUDE.md')));
  assert.ok(fs.existsSync(path.join(root, '.claude', 'entity-registry.json')));
  assert.ok(fs.existsSync(path.join(root, 'api', 'CLAUDE.md')));

  // Generated CLAUDE.md must carry the marker
  const orch = fs.readFileSync(path.join(root, '.claude', 'CLAUDE.md'), 'utf-8');
  assert.match(orch, /<!-- mustard:generated -->/);
});

// ---------------------------------------------------------------------------
// Fast path — foundational files exist + no --force
// ---------------------------------------------------------------------------

test('fast path: skips bootstrap when root CLAUDE.md and registry exist', () => {
  const root = mkProject({
    rootClaudeMd: '# Root\n',
    registry: { _meta: { version: '4.0' } },
  });
  const res = runOrchestrate(root);
  const out = parseStdout(res);

  assert.equal(out.fastPath, true);
  // Bootstrap should not have re-generated foundational files
  assert.ok(!out.generated.includes('.claude/CLAUDE.md'));
  assert.ok(!out.generated.includes('CLAUDE.md'));
  assert.ok(!out.generated.includes('.claude/entity-registry.json'));
});

// ---------------------------------------------------------------------------
// Force flag bypasses fast path
// ---------------------------------------------------------------------------

test('--force overrides fast path and regenerates orchestrator CLAUDE.md', () => {
  const root = mkProject({
    rootClaudeMd: '# Root\n',
    registry: { _meta: { version: '4.0' } },
  });
  const res = runOrchestrate(root, ['--force']);
  const out = parseStdout(res);

  assert.equal(out.force, true);
  assert.equal(out.fastPath, false);
  assert.ok(out.generated.includes('.claude/CLAUDE.md'), 'orch CLAUDE.md regenerated');
});

// ---------------------------------------------------------------------------
// Dispatch list — incremental skip
// ---------------------------------------------------------------------------

test('incremental: subproject with unchanged hash is skipped, not dispatched', () => {
  const root = mkProject({
    rootClaudeMd: '# Root\n',
    registry: { _meta: { version: '4.0' } },
    detectCache: {
      subprojects: [{ name: 'api', path: 'api', role: 'api', agent: 'backend' }],
      sourceHashes: { api: 'hash-v1' }, // matches detect fixture
    },
  });
  const res = runOrchestrate(root);
  const out = parseStdout(res);

  assert.equal(out.dispatch.length, 0, 'no dispatch — hash matched');
  assert.equal(out.skipped.length, 1);
  assert.equal(out.skipped[0].name, 'api');
});

test('incremental: subproject with changed hash is dispatched', () => {
  const root = mkProject({
    rootClaudeMd: '# Root\n',
    registry: { _meta: { version: '4.0' } },
    detectCache: {
      subprojects: [{ name: 'api', path: 'api', role: 'api', agent: 'backend' }],
      sourceHashes: { api: 'old-hash' }, // mismatch with detect fixture (hash-v1)
    },
  });
  const res = runOrchestrate(root);
  const out = parseStdout(res);

  assert.equal(out.dispatch.length, 1);
  assert.equal(out.dispatch[0].name, 'api');
  assert.ok(out.dispatch[0].agentPrompt, 'agentPrompt rendered');
  assert.match(out.dispatch[0].agentPrompt, /scanning subproject `api`/);
  assert.match(out.dispatch[0].agentPrompt, /\.NET 9/, 'stack interpolated');
});

test('--force dispatches even when hash matches', () => {
  const root = mkProject({
    rootClaudeMd: '# Root\n',
    registry: { _meta: { version: '4.0' } },
    detectCache: {
      subprojects: [{ name: 'api', path: 'api', role: 'api', agent: 'backend' }],
      sourceHashes: { api: 'hash-v1' },
    },
  });
  const res = runOrchestrate(root, ['--force']);
  const out = parseStdout(res);

  assert.equal(out.dispatch.length, 1);
  assert.match(out.dispatch[0].agentPrompt, /FORCE MODE ACTIVE/);
});

// ---------------------------------------------------------------------------
// Target single subproject
// ---------------------------------------------------------------------------

test('single subproject filter: --force <name> dispatches only that one', () => {
  const root = mkProject({
    detectFixture: {
      subprojects: [
        { name: 'api', path: 'api', role: 'api', agent: 'backend', stackSummary: '.NET 9' },
        { name: 'ui', path: 'ui', role: 'ui', agent: 'frontend', stackSummary: 'React 19' },
      ],
      sourceHashes: { api: 'h1', ui: 'h2' },
      moduleHashes: {},
    },
  });
  const res = runOrchestrate(root, ['ui', '--force']);
  const out = parseStdout(res);

  assert.equal(out.dispatch.length, 1);
  assert.equal(out.dispatch[0].name, 'ui');
});

// ---------------------------------------------------------------------------
// Cleanup stale subprojects
// ---------------------------------------------------------------------------

test('cleanup: removes generated agent .md for subproject no longer detected', () => {
  const root = mkProject({
    rootClaudeMd: '# Root\n',
    registry: { _meta: { version: '4.0' } },
    detectCache: {
      subprojects: [
        { name: 'api', path: 'api', role: 'api', agent: 'backend' },
        { name: 'old', path: 'old', role: 'api', agent: 'backend' },
      ],
      sourceHashes: { api: 'hash-v1', old: 'old-hash' },
    },
  });

  // Pre-create generated agent files for 'old'
  const agentsDir = path.join(root, '.claude', 'agents');
  fs.mkdirSync(agentsDir, { recursive: true });
  fs.writeFileSync(
    path.join(agentsDir, 'old-impl.md'),
    '<!-- mustard:generated -->\n# old impl\n',
    'utf-8'
  );
  fs.writeFileSync(
    path.join(agentsDir, 'user-authored.md'),
    '# manual file (no marker)\n',
    'utf-8'
  );

  const res = runOrchestrate(root);
  const out = parseStdout(res);

  assert.ok(out.cleanup.includes('.claude/agents/old-impl.md'), 'generated stale file removed');
  assert.ok(!fs.existsSync(path.join(agentsDir, 'old-impl.md')));
  assert.ok(fs.existsSync(path.join(agentsDir, 'user-authored.md')), 'user file preserved');
});

// ---------------------------------------------------------------------------
// Agent file generation
// ---------------------------------------------------------------------------

test('generates impl + explorer agent .md when missing', () => {
  const root = mkProject({});
  const res = runOrchestrate(root, ['--force']);
  const out = parseStdout(res);

  assert.ok(out.generated.includes('.claude/agents/api-impl.md'));
  assert.ok(out.generated.includes('.claude/agents/api-explorer.md'));

  const impl = fs.readFileSync(path.join(root, '.claude', 'agents', 'api-impl.md'), 'utf-8');
  assert.match(impl, /name: api-impl/);
  assert.match(impl, /model: sonnet/);
  assert.match(impl, /<!-- mustard:generated -->/);
  assert.match(impl, /# Api Implementation Agent/, 'name capitalized');

  const exp = fs.readFileSync(path.join(root, '.claude', 'agents', 'api-explorer.md'), 'utf-8');
  assert.match(exp, /name: api-explorer/);
  assert.match(exp, /model: haiku/);
});

test('preserves existing agent .md when present and --force is not set', () => {
  const root = mkProject({});
  // Pre-create a customized agent file
  const agentsDir = path.join(root, '.claude', 'agents');
  fs.mkdirSync(agentsDir, { recursive: true });
  const customContent = `---
name: api-impl
description: Custom description
---
<!-- mustard:generated -->
# Custom title

## Boundary
Custom boundary text refined by a previous scan.
`;
  fs.writeFileSync(path.join(agentsDir, 'api-impl.md'), customContent, 'utf-8');

  // Run without --force
  const res = runOrchestrate(root, []);
  parseStdout(res);

  // File should still contain the custom content
  const after = fs.readFileSync(path.join(agentsDir, 'api-impl.md'), 'utf-8');
  assert.match(after, /Custom boundary text refined by a previous scan/);
});

test('--force overwrites existing agent .md', () => {
  const root = mkProject({});
  const agentsDir = path.join(root, '.claude', 'agents');
  fs.mkdirSync(agentsDir, { recursive: true });
  fs.writeFileSync(path.join(agentsDir, 'api-impl.md'), '# old\n', 'utf-8');

  const res = runOrchestrate(root, ['--force']);
  parseStdout(res);

  const after = fs.readFileSync(path.join(agentsDir, 'api-impl.md'), 'utf-8');
  assert.match(after, /name: api-impl/, 'overwritten by template');
});

// ---------------------------------------------------------------------------
// Fail-open: missing prompt template is reported but does not crash
// ---------------------------------------------------------------------------

test('fail-open: missing agent-prompt.template.md reports error but exits 0', () => {
  const root = mkProject({
    detectCache: {
      subprojects: [{ name: 'api', path: 'api', role: 'api' }],
      sourceHashes: { api: 'old' }, // forces dispatch
    },
  });
  // Delete the template
  fs.unlinkSync(path.join(root, '.claude', 'scripts', 'scan', 'agent-prompt.template.md'));

  const res = runOrchestrate(root);
  assert.equal(res.status, 0, 'fail-open');
  const out = JSON.parse(res.stdout);
  assert.ok(
    out.errors.some(e => /prompt template/i.test(e)),
    `expected template error, got: ${JSON.stringify(out.errors)}`
  );
});

// ---------------------------------------------------------------------------
// Fail-open: broken sync-detect reports error
// ---------------------------------------------------------------------------

test('fail-open: broken sync-detect reports error in JSON, exits 0', () => {
  const root = mkProject({});
  // Replace stub with one that crashes
  fs.writeFileSync(
    path.join(root, '.claude', 'scripts', 'sync-detect.js'),
    'process.exit(1);\n',
    'utf-8'
  );

  const res = runOrchestrate(root);
  assert.equal(res.status, 0);
  const out = JSON.parse(res.stdout);
  assert.ok(out.errors.some(e => /detect/.test(e)), `expected detect error: ${JSON.stringify(out.errors)}`);
});

// ---------------------------------------------------------------------------
// Root CLAUDE.md update — Project Structure table refresh
// ---------------------------------------------------------------------------

test('preserves CRLF line endings of root CLAUDE.md when updating', () => {
  const initialRoot = '# Project\r\n\r\n## Project Structure\r\n\r\n| Subproject | Technology | Port | CLAUDE.md |\r\n|------------|------------|------|-----------|\r\n| api | old stack | - | [api](./api/CLAUDE.md) |\r\n';
  const root = mkProject({
    rootClaudeMd: initialRoot,
    registry: { _meta: { version: '4.0' } },
  });

  parseStdout(runOrchestrate(root));
  const updated = fs.readFileSync(path.join(root, 'CLAUDE.md'), 'utf-8');
  // Must keep CRLF — never silently rewrite line endings
  assert.ok(updated.includes('\r\n'), 'CRLF preserved');
  // And must NOT contain LF-only newlines (would be a regression on Windows)
  assert.ok(!/(?<!\r)\n/.test(updated), 'no bare LF introduced');
});

test('preserves existing Technology cell when stackSummary is empty', () => {
  const initialRoot = `# Project

## Project Structure

| Subproject | Technology | Port | CLAUDE.md |
|------------|------------|------|-----------|
| api | Hand-curated stack notes | - | [api](./api/CLAUDE.md) |
`;
  const root = mkProject({
    rootClaudeMd: initialRoot,
    registry: { _meta: { version: '4.0' } },
    detectFixture: {
      subprojects: [{ name: 'api', path: 'api', role: 'api', agent: 'backend', stackSummary: '' }],
      sourceHashes: { api: 'h1' },
      moduleHashes: {},
    },
  });

  parseStdout(runOrchestrate(root));
  const updated = fs.readFileSync(path.join(root, 'CLAUDE.md'), 'utf-8');
  assert.match(updated, /Hand-curated stack notes/, 'previous tech preserved when new is empty');
});

test('updates Project Structure table in existing root CLAUDE.md', () => {
  const initialRoot = `# Project

## Project Structure

| Subproject | Technology | Port | CLAUDE.md |
|------------|------------|------|-----------|
| oldname | old stack | - | [oldname](./oldname/CLAUDE.md) |

## Other Section

stuff here
`;
  const root = mkProject({
    rootClaudeMd: initialRoot,
    registry: { _meta: { version: '4.0' } },
  });

  const res = runOrchestrate(root);
  parseStdout(res);

  const updated = fs.readFileSync(path.join(root, 'CLAUDE.md'), 'utf-8');
  assert.match(updated, /\| api \| \.NET 9 \| - \| \[api\]\(\.\/api\/CLAUDE\.md\) \|/);
  assert.doesNotMatch(updated, /oldname/, 'old row removed');
  assert.match(updated, /## Other Section/, 'other sections preserved');
});
