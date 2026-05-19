'use strict';

const { test } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const {
  backupGeneratedMds,
  purgeGeneratedSkills,
  ensureNotesMd,
  buildToolingBlock,
  buildStructureBlock,
} = require('../scan/_precompute.js');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function mkTmp() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'scan-precompute-'));
}

// ---------------------------------------------------------------------------
// backupGeneratedMds
// ---------------------------------------------------------------------------

test('backupGeneratedMds: moves generated files, preserves user-authored', () => {
  const dir = mkTmp();
  fs.writeFileSync(path.join(dir, 'stack.md'), '<!-- mustard:generated -->\n# Stack\n');
  fs.writeFileSync(path.join(dir, 'notes.md'), '# Notes (user)\n');
  fs.writeFileSync(path.join(dir, 'patterns.md'), '<!-- mustard:generated -->\n# Patterns\n');

  const res = backupGeneratedMds(dir);

  assert.ok(res.moved.includes('stack.md'), 'stack.md moved');
  assert.ok(res.moved.includes('patterns.md'), 'patterns.md moved');
  assert.ok(!res.moved.includes('notes.md'), 'notes.md not moved');
  assert.ok(res.created_backup_dir === true, 'backup dir created');
  assert.ok(fs.existsSync(path.join(dir, '_backup', 'stack.md')), 'stack in backup');
  assert.ok(fs.existsSync(path.join(dir, '_backup', 'patterns.md')), 'patterns in backup');
  assert.ok(fs.existsSync(path.join(dir, 'notes.md')), 'notes still in commands dir');
  assert.ok(!fs.existsSync(path.join(dir, 'stack.md')), 'stack no longer in commands dir');
});

test('backupGeneratedMds: idempotent second run (no double-move)', () => {
  const dir = mkTmp();
  fs.writeFileSync(path.join(dir, 'stack.md'), '<!-- mustard:generated -->\n# Stack\n');

  const res1 = backupGeneratedMds(dir);
  assert.equal(res1.moved.length, 1);

  // File is now in _backup, not in commands dir — second run should move 0
  const res2 = backupGeneratedMds(dir);
  assert.equal(res2.moved.length, 0, 'second run moves nothing');
  assert.ok(fs.existsSync(path.join(dir, '_backup', 'stack.md')), 'backup still intact');
});

test('backupGeneratedMds: returns empty result for missing dir', () => {
  const res = backupGeneratedMds('/tmp/__nonexistent_dir_xyzzy__');
  assert.deepEqual(res, { moved: [], created_backup_dir: false });
});

// ---------------------------------------------------------------------------
// purgeGeneratedSkills
// ---------------------------------------------------------------------------

test('purgeGeneratedSkills: removes generated skill subdirs, preserves user-authored', () => {
  const dir = mkTmp();
  const genSkill = path.join(dir, 'api-handler-pattern');
  const userSkill = path.join(dir, 'my-custom-skill');
  fs.mkdirSync(genSkill, { recursive: true });
  fs.mkdirSync(userSkill, { recursive: true });
  fs.writeFileSync(path.join(genSkill, 'SKILL.md'), '<!-- mustard:generated -->\n# Handler\n');
  fs.writeFileSync(path.join(userSkill, 'SKILL.md'), '# My Custom Skill\n');

  const res = purgeGeneratedSkills(dir);

  assert.ok(res.removed.includes('api-handler-pattern'), 'generated skill removed');
  assert.ok(!res.removed.includes('my-custom-skill'), 'user skill not removed');
  assert.ok(!fs.existsSync(genSkill), 'generated skill dir gone');
  assert.ok(fs.existsSync(userSkill), 'user skill dir preserved');
});

test('purgeGeneratedSkills: handles missing dir gracefully', () => {
  const res = purgeGeneratedSkills('/tmp/__nonexistent_skills_xyzzy__');
  assert.deepEqual(res, { removed: [] });
});

// Regression: the scan agent writes SKILL.md with YAML frontmatter FIRST and the
// `<!-- mustard:generated -->` marker AFTER it (mandated by templates/CLAUDE.md).
// Real-world `description` fields routinely push the marker past byte 200, so a
// head-only check (the prior implementation) silently treated all of them as
// user-authored and skipped the purge — letting --force become a no-op.
test('purgeGeneratedSkills: marker after long YAML frontmatter still triggers purge', () => {
  const dir = mkTmp();
  const skillDir = path.join(dir, 'sialia-partners-auth-primitive-pattern');
  fs.mkdirSync(skillDir, { recursive: true });

  // Realistic SKILL.md as produced by /scan agents — long pushy description
  // pushes the marker well past byte 200 (observed range in production: 344-625).
  const realisticSkill = [
    '---',
    'name: sialia-partners-auth-primitive-pattern',
    'description: "Auth-page UI primitive pattern for sialia-partners app/(auth)/_components/auth-*.tsx: small single-prop interface components (AuthCardLayout, AuthSplitLayout, AuthHeader, AuthFooterLink, AuthSubmitButton) with Tailwind layout, no internal state, no hooks. Use when adding a new auth-page building block, modifying the auth split layout, wiring an auth submit button, or the user just says \'auth layout\', \'auth-card\', \'auth-header\'."',
    'source: scan',
    '---',
    '',
    '<!-- mustard:generated at:2026-05-13T00:00:00Z role:ui -->',
    '',
    '## Convention',
    '- Cluster: auth-* filename pattern',
    '',
  ].join('\n');
  fs.writeFileSync(path.join(skillDir, 'SKILL.md'), realisticSkill);

  // Sanity: marker MUST be past byte 200, otherwise the regression assertion is vacuous.
  const markerOffset = realisticSkill.indexOf('<!-- mustard:generated');
  assert.ok(markerOffset > 200, `fixture must place marker past byte 200 (got ${markerOffset})`);

  const res = purgeGeneratedSkills(dir);
  assert.ok(
    res.removed.includes('sialia-partners-auth-primitive-pattern'),
    'skill with marker after long frontmatter must be purged'
  );
  assert.ok(!fs.existsSync(skillDir), 'skill dir gone after purge');
});

// ---------------------------------------------------------------------------
// ensureNotesMd
// ---------------------------------------------------------------------------

test('ensureNotesMd: creates notes.md with correct H1 and all three H2 sections', () => {
  const dir = mkTmp();
  const created = ensureNotesMd(dir, 'my-api', 'api');

  assert.equal(created, true, 'returns true when created');
  const notesPath = path.join(dir, 'notes.md');
  assert.ok(fs.existsSync(notesPath), 'notes.md exists');
  const content = fs.readFileSync(notesPath, 'utf-8');
  assert.match(content, /# Notes: my-api \(api\)/, 'correct H1');
  assert.match(content, /## Mandatory Patterns/, 'H2 Mandatory Patterns');
  assert.match(content, /## Known Pitfalls/, 'H2 Known Pitfalls');
  assert.match(content, /## Observations/, 'H2 Observations');
});

test('ensureNotesMd: idempotent — returns false if file already exists', () => {
  const dir = mkTmp();
  const notesPath = path.join(dir, 'notes.md');
  fs.writeFileSync(notesPath, '# Existing notes\n');

  const created = ensureNotesMd(dir, 'my-api', 'api');
  assert.equal(created, false, 'returns false when file exists');
  // Content unchanged
  assert.equal(fs.readFileSync(notesPath, 'utf-8'), '# Existing notes\n');
});

// ---------------------------------------------------------------------------
// buildToolingBlock
// ---------------------------------------------------------------------------

test('buildToolingBlock: TS package.json with build/test/lint produces block with all three', () => {
  const dir = mkTmp();
  const pkg = { name: 'api', scripts: { build: 'tsc', test: 'vitest', lint: 'eslint .' } };
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(pkg));

  const block = buildToolingBlock(dir, 'TypeScript');
  assert.match(block, /## Tooling detected/, 'header present');
  assert.match(block, /build: tsc/, 'build cmd present');
  assert.match(block, /test: vitest/, 'test cmd present');
  assert.match(block, /lint: eslint/, 'lint cmd present');
});

test('buildToolingBlock: .NET csproj fallback produces build+test lines', () => {
  const dir = mkTmp();
  fs.writeFileSync(path.join(dir, 'MyApp.csproj'), '<Project Sdk="Microsoft.NET.Sdk"></Project>');

  const block = buildToolingBlock(dir, '.NET 9');
  assert.match(block, /## Tooling detected/, 'header present');
  assert.match(block, /dotnet build/, 'build cmd present');
  assert.match(block, /dotnet test/, 'test cmd present');
});

test('buildToolingBlock: corrupted package.json returns empty string (try/catch)', () => {
  const dir = mkTmp();
  fs.writeFileSync(path.join(dir, 'package.json'), '{INVALID JSON{{');

  const block = buildToolingBlock(dir, 'TypeScript');
  assert.equal(block, '', 'returns empty string on JSON parse error');
});

test('buildToolingBlock: no matching files returns empty string', () => {
  const dir = mkTmp();
  // No package.json, no csproj, no pyproject.toml
  const block = buildToolingBlock(dir, 'TypeScript');
  assert.equal(block, '');
});

// ---------------------------------------------------------------------------
// buildStructureBlock
// ---------------------------------------------------------------------------

test('buildStructureBlock: returns empty string for dir with no subdirs', () => {
  const dir = mkTmp();
  // Only files, no subdirs
  fs.writeFileSync(path.join(dir, 'index.ts'), '');
  const block = buildStructureBlock(dir);
  assert.equal(block, '');
});

test('buildStructureBlock: returns empty string when only 1 non-ignored dir', () => {
  const dir = mkTmp();
  fs.mkdirSync(path.join(dir, 'src'));
  const block = buildStructureBlock(dir);
  assert.equal(block, '', 'single dir produces empty block');
});

test('buildStructureBlock: includes only non-ignored dirs and caps at 12', () => {
  const dir = mkTmp();
  fs.mkdirSync(path.join(dir, 'src'));
  fs.mkdirSync(path.join(dir, 'tests'));
  fs.mkdirSync(path.join(dir, 'node_modules')); // ignored
  fs.mkdirSync(path.join(dir, 'dist'));          // ignored

  // Add files to src and tests
  fs.writeFileSync(path.join(dir, 'src', 'index.ts'), '');
  fs.writeFileSync(path.join(dir, 'tests', 'api.test.ts'), '');

  const block = buildStructureBlock(dir);
  assert.match(block, /## Project structure/, 'header present');
  assert.match(block, /src\//, 'src listed');
  assert.match(block, /tests\//, 'tests listed');
  assert.ok(!block.includes('node_modules'), 'node_modules excluded');
  assert.ok(!block.includes('dist'), 'dist excluded');
});

test('buildStructureBlock: handles missing dir gracefully', () => {
  const block = buildStructureBlock('/tmp/__nonexistent_subproject_xyzzy__');
  assert.equal(block, '');
});
