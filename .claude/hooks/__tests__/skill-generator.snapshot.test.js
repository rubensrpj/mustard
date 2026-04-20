'use strict';
const test = require('node:test');
const assert = require('node:assert');
const { execSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const SCRIPT = path.join(ROOT, 'templates', 'scripts', 'skill-generator.js');

test('skill-generator: --dry-run output is stable across runs', () => {
  const opts = { encoding: 'utf-8', cwd: ROOT };
  const out1 = execSync(`node "${SCRIPT}" --dry-run`, opts);
  const out2 = execSync(`node "${SCRIPT}" --dry-run`, opts);
  assert.strictEqual(out1, out2, 'dry-run output should be deterministic');
});

test('skill-generator: --check flag passes (JS syntax valid)', () => {
  const out = execSync(`node --check "${SCRIPT}"`, { encoding: 'utf-8', cwd: ROOT });
  // node --check exits 0 on success, no assertion needed beyond no-throw
  assert.ok(true, 'syntax check passed');
});

test('skill-generator: _skill-meta.json is valid JSON with required keys', () => {
  const metaPath = path.join(ROOT, 'templates', 'scripts', '_skill-meta.json');
  const meta = JSON.parse(require('fs').readFileSync(metaPath, 'utf-8'));
  assert.ok(meta.stacks && typeof meta.stacks === 'object', 'must have stacks');
  assert.ok(meta.roles && typeof meta.roles === 'object', 'must have roles');
  assert.ok(meta.stacks.dotnet, 'stacks.dotnet must exist');
  assert.ok(meta.stacks.typescript, 'stacks.typescript must exist');
  assert.strictEqual(meta.stacks.dotnet.lang, 'csharp');
});

test('skill-generator: validateSkill catches missing description', () => {
  // skill-generator exports validateSkill via module.exports when required (not run as main)
  const { validateSkill } = require('../../scripts/skill-generator.js');
  assert.ok(typeof validateSkill === 'function', 'validateSkill must be exported');

  // Test: missing frontmatter
  let r = validateSkill('no frontmatter here');
  assert.strictEqual(r.ok, false, 'should fail without frontmatter');
  assert.ok(r.errors.some(e => e.includes('frontmatter')), 'error should mention frontmatter');

  // Test: missing description
  r = validateSkill('---\nname: foo-bar\nsource: scan\n---\n<!-- mustard:generated -->\nhi');
  assert.strictEqual(r.ok, false, 'should fail without description');
  assert.ok(r.errors.some(e => e.includes('description')), 'error should mention description');

  // Test: missing source
  r = validateSkill('---\nname: foo-bar\ndescription: "Use when creating a new entity, add model, create table, even if the user says new thing. This is long enough."\n---\n<!-- mustard:generated -->\nhi');
  assert.strictEqual(r.ok, false, 'should fail without source');
  assert.ok(r.errors.some(e => e.includes('source')), 'error should mention source');

  // Test: valid
  r = validateSkill('---\nname: foo-bar\ndescription: "Use when creating a new entity, add model, create table, even if the user says new thing. This is long enough to pass."\nsource: scan\n---\n<!-- mustard:generated -->\nhi');
  assert.strictEqual(r.ok, true, 'valid skill should pass');
});

test('skill-generator: all skill .tmpl files have source: scan in frontmatter', () => {
  const fs = require('fs');
  const tplDir = path.join(ROOT, 'templates', 'skill-templates');
  const files = fs.readdirSync(tplDir).filter(f => f.endsWith('.skill.md.tmpl'));
  assert.ok(files.length > 0, 'should have at least one skill template');
  for (const file of files) {
    const content = fs.readFileSync(path.join(tplDir, file), 'utf-8');
    assert.ok(content.includes('source: scan'), `${file} must contain "source: scan" in frontmatter`);
  }
});

// ---------------------------------------------------------------------------
// Cluster discovery tests
// ---------------------------------------------------------------------------

test('cluster-discovery: discovers suffix-cluster from synthetic temp dir', () => {
  const fs = require('fs');
  const os = require('os');
  const { discoverClusters } = require('../../scripts/registry/cluster-discovery.js');

  // Create a temporary directory structure with 5+ files sharing suffix "Handler"
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-test-'));
  const subDir = path.join(tmpDir, 'Commands');
  fs.mkdirSync(subDir, { recursive: true });

  const files = [
    'CreateUserHandler.cs',
    'UpdateUserHandler.cs',
    'DeleteUserHandler.cs',
    'CreateContractHandler.cs',
    'UpdateContractHandler.cs',
    'CreateInvoiceHandler.cs',
  ];
  for (const f of files) {
    fs.writeFileSync(path.join(subDir, f), `public class ${f.replace('.cs', '')} { }`);
  }

  try {
    const clusters = discoverClusters(tmpDir, 'dotnet');
    assert.ok(Array.isArray(clusters), 'should return array');
    assert.ok(clusters.length >= 1, 'should find at least one cluster');

    const handlerCluster = clusters.find(c => c.suffix === 'Handler');
    assert.ok(handlerCluster, 'should detect "Handler" suffix cluster');
    assert.ok(handlerCluster.fileCount >= 6, `expected fileCount >= 6, got ${handlerCluster.fileCount}`);
    assert.ok(
      handlerCluster.kind === 'folder-cluster' || handlerCluster.kind === 'suffix-cluster',
      `expected folder-cluster or suffix-cluster, got ${handlerCluster.kind}`
    );
  } finally {
    // Cleanup
    try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
  }
});

test('cluster-discovery: no hardcoded tech names in cluster-discovery.js source', () => {
  const fs = require('fs');
  const src = fs.readFileSync(
    path.join(ROOT, 'templates', 'scripts', 'registry', 'cluster-discovery.js'),
    'utf-8'
  );
  // These technology names must NOT appear in non-comment source lines of the discovery code.
  // We strip comment lines before checking so JSDoc examples don't trigger false positives.
  const forbidden = ['graphql', 'GraphQL', 'cqrs', 'CQRS', 'mediator', 'Mediator'];
  const codeLines = src.split('\n').filter(line => {
    const trimmed = line.trim();
    return !trimmed.startsWith('//') && !trimmed.startsWith('*') && trimmed !== '';
  });
  const codeOnly = codeLines.join('\n');
  for (const word of forbidden) {
    assert.ok(
      !codeOnly.includes(word),
      `cluster-discovery.js must not contain hardcoded tech term "${word}" in non-comment code`
    );
  }
});

test('genClusterSkill: produces valid SKILL.md for Handler cluster', () => {
  const { genClusterSkill, validateSkill } = require('../../scripts/skill-generator.js');

  const cluster = {
    kind: 'suffix-cluster',
    suffix: 'Handler',
    ext: '.cs',
    fileCount: 7,
    folders: ['Commands/Create', 'Commands/Update', 'Commands/Delete'],
    folderPattern: '**/Commands/',
    samples: ['CreateUserHandler.cs', 'UpdateContractHandler.cs', 'DeleteInvoiceHandler.cs'],
    label: 'Handler',
  };

  const result = genClusterSkill('backend', 'dotnet', cluster, 'api');
  assert.ok(result !== null, 'genClusterSkill should return a result');
  assert.ok(result.slug === 'handler', `slug should be "handler", got "${result.slug}"`);

  const validation = validateSkill(result.skillMd);
  assert.ok(validation.ok, `generated skill should be valid. Errors: ${validation.errors.join(', ')}`);

  // Frontmatter name must start with skill prefix
  assert.ok(result.skillMd.includes('backend-handler-pattern'), 'name should be backend-handler-pattern');

  // Must NOT contain forbidden tech names as string literals in the output
  const forbidden = ['graphql', 'GraphQL', 'cqrs', 'mediator'];
  for (const word of forbidden) {
    assert.ok(!result.skillMd.includes(word), `output must not contain "${word}"`);
  }
});

test('cleanupOrphanSkills: removes source:scan folders not in expected set', () => {
  const { cleanupOrphanSkills } = require('../../scripts/skill-generator.js');

  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-cleanup-'));
  const skillsDir = path.join(tmpRoot, '.claude', 'skills');

  const mkSkill = (folder, frontmatter) => {
    const dir = path.join(skillsDir, folder);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'SKILL.md'), `---\n${frontmatter}\n---\n# skill`);
  };

  // Expected skills (current run will write these)
  mkSkill('backend-entity-creation', 'name: backend-entity-creation\ndescription: x\nsource: scan');
  mkSkill('backend-service-pattern', 'name: backend-service-pattern\ndescription: x\nsource: scan');

  // Orphan: was generated previously but pattern no longer exists
  mkSkill('backend-queryresolver-pattern', 'name: backend-queryresolver-pattern\ndescription: x\nsource: scan');

  // Manual skill — MUST NOT be touched (source: manual)
  mkSkill('backend-custom-helper', 'name: backend-custom-helper\ndescription: x\nsource: manual');

  // Unrelated sub — MUST NOT be touched (not in processed subs)
  mkSkill('frontend-entity-creation', 'name: frontend-entity-creation\ndescription: x\nsource: scan');

  const expected = new Set(['backend-entity-creation', 'backend-service-pattern']);
  const log = [];
  const removed = cleanupOrphanSkills(skillsDir, expected, ['backend'], log);

  try {
    assert.strictEqual(removed, 1, 'should remove exactly 1 orphan');
    assert.ok(!fs.existsSync(path.join(skillsDir, 'backend-queryresolver-pattern')), 'orphan removed');
    assert.ok(fs.existsSync(path.join(skillsDir, 'backend-entity-creation')), 'expected preserved');
    assert.ok(fs.existsSync(path.join(skillsDir, 'backend-custom-helper')), 'source:manual preserved');
    assert.ok(fs.existsSync(path.join(skillsDir, 'frontend-entity-creation')), 'other sub preserved');
  } finally {
    try { fs.rmSync(tmpRoot, { recursive: true, force: true }); } catch { /* ignore */ }
  }
});

test('cluster-discovery: min suffix length filters short suffixes', () => {
  const fs = require('fs');
  const os = require('os');
  const { discoverClusters } = require('../../scripts/registry/cluster-discovery.js');

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-test-short-'));
  const subDir = path.join(tmpDir, 'Models');
  fs.mkdirSync(subDir, { recursive: true });

  // Files ending in short suffix "es" should NOT trigger a cluster
  const files = ['Bankes.cs', 'Foxes.cs', 'Boxes.cs', 'Taxes.cs', 'Mixes.cs', 'Fixes.cs'];
  for (const f of files) {
    fs.writeFileSync(path.join(subDir, f), `public class ${f.replace('.cs', '')} { }`);
  }

  try {
    const clusters = discoverClusters(tmpDir, 'dotnet');
    const shortSuffix = clusters.find(c => c.suffix === 'es' || c.suffix.length < 6);
    assert.ok(!shortSuffix, 'should NOT detect short suffix clusters (< 6 chars)');
  } finally {
    try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
  }
});
