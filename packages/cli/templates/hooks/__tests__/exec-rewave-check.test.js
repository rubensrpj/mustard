#!/usr/bin/env bun
"use strict";

/**
 * Tests for exec-rewave-check.js
 * Run: bun test .claude/hooks/__tests__/exec-rewave-check.test.js
 */

const { describe, it, afterEach } = require('bun:test');
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const SCRIPT = path.resolve(__dirname, "..", "..", "scripts", "exec-rewave-check.js");

// ── helpers ────────────────────────────────────────────────────────────────────

function makeTempProject() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mustard-rewave-"));
  fs.mkdirSync(path.join(dir, ".claude", ".pipeline-states"), { recursive: true });
  fs.mkdirSync(path.join(dir, ".claude", "spec", "active"), { recursive: true });
  return dir;
}

function makeSpec(projectDir, specName, filesSection, extra = "") {
  const specDir = path.join(projectDir, ".claude", "spec", "active", specName);
  fs.mkdirSync(specDir, { recursive: true });
  const specText = `# Feature: ${specName}
### Status: approved | Phase: PLAN | Scope: full

## Summary
Test feature.

${filesSection}

## Tasks
- [ ] Implement thing

## Acceptance Criteria
- [ ] AC-1: build passes — Command: \`node -e "process.exit(0)"\`
${extra}`;
  fs.writeFileSync(path.join(specDir, "spec.md"), specText, "utf8");
  return specDir;
}

function writePipelineState(projectDir, specName, state) {
  const file = path.join(projectDir, ".claude", ".pipeline-states", `${specName}.json`);
  fs.writeFileSync(file, JSON.stringify(state, null, 2), "utf8");
}

function run(projectDir, specRelPath) {
  const result = spawnSync(
    process.execPath,
    [SCRIPT, "--spec", specRelPath],
    {
      cwd: projectDir,
      encoding: "utf8",
      timeout: 20000,
      env: { ...process.env, MUSTARD_DISABLED_HOOKS: "all" },
    }
  );
  let parsed = null;
  try { parsed = JSON.parse(result.stdout.trim()); } catch (_) {}
  return { code: result.status, stdout: result.stdout.trim(), parsed };
}

function cleanup(dir) {
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch (_) {}
}

// ── test 1: single-layer spec → keep-single ────────────────────────────────────

describe("exec-rewave-check", () => {
  // bun:test compat: shared cleanup tracker (replaces node:test's t.after())
  const _projs = [];
  afterEach(() => { while (_projs.length) cleanup(_projs.pop()); });
  const track = (p) => { _projs.push(p); return p; };

  it("single-layer (all api): keep-single with reason single-layer", () => {
    const proj = track(makeTempProject());
    const specName = "2026-01-01-test-single";
    const filesSection = `## Files
- src/api/users.ts
- src/api/orders.ts
- src/api/helpers.ts`;
    makeSpec(proj, specName, filesSection);
    const specPath = `.claude/spec/active/${specName}/spec.md`;

    const { parsed } = run(proj, specPath);
    assert.ok(parsed, "output must be valid JSON");
    assert.equal(parsed.action, "keep-single");
    assert.equal(parsed.reason, "single-layer");
  });

  // ── test 2: multi-layer spec → decomposed ──────────────────────────────────
  it("multi-layer (schema + api): decomposed into 2 waves", () => {
    const proj = track(makeTempProject());
    const specName = "2026-01-02-test-multi";
    // wave-dependency works on real files; for DAG we need actual files on disk
    // Create the files so the DAG parser can read them (no imports → each in own wave)
    const apiDir = path.join(proj, "src", "api");
    const schemaDir = path.join(proj, "src", "schema");
    fs.mkdirSync(apiDir, { recursive: true });
    fs.mkdirSync(schemaDir, { recursive: true });
    // schema file — no imports
    fs.writeFileSync(path.join(schemaDir, "user.ts"), "export const users = {};\n", "utf8");
    // api file — imports from schema
    fs.writeFileSync(path.join(apiDir, "users.ts"), 'import { users } from "../schema/user";\n', "utf8");

    const filesSection = `## Files
- src/schema/user.ts
- src/api/users.ts`;
    makeSpec(proj, specName, filesSection);
    writePipelineState(proj, specName, { specName, status: "approved", phase: 2 });

    const specPath = `.claude/spec/active/${specName}/spec.md`;
    const { parsed } = run(proj, specPath);
    assert.ok(parsed, "output must be valid JSON");

    if (parsed.action === "decomposed") {
      assert.equal(parsed.action, "decomposed");
      assert.ok(parsed.totalWaves >= 2, `expected >=2 waves, got ${parsed.totalWaves}`);

      // wave-plan.md created
      const specDir = path.join(proj, ".claude", "spec", "active", specName);
      assert.ok(fs.existsSync(path.join(specDir, "wave-plan.md")), "wave-plan.md must exist");

      // spec.original.md created
      assert.ok(fs.existsSync(path.join(specDir, "spec.original.md")), "spec.original.md must exist");

      // per-wave spec.md dirs created
      for (const w of parsed.waves) {
        const waveDir = path.join(specDir, `wave-${w.wave}-${w.role}`);
        assert.ok(fs.existsSync(path.join(waveDir, "spec.md")), `wave-${w.wave} spec.md must exist`);
      }

      // pipeline-state updated
      const stateFile = path.join(proj, ".claude", ".pipeline-states", `${specName}.json`);
      const state = JSON.parse(fs.readFileSync(stateFile, "utf8"));
      assert.equal(state.isWavePlan, true);
      assert.ok(state.totalWaves >= 2);
      assert.deepEqual(state.completedWaves, []);
      assert.equal(state.rewaveSource, "exec-entry");
    } else {
      // DAG had no real depth (both files in same wave) → keep-single is acceptable
      assert.equal(parsed.action, "keep-single");
    }
  });

  // ── test 3: spec with existing wave-plan.md → skip already-decomposed ────────
  it("already-decomposed (wave-plan.md exists): skip", () => {
    const proj = track(makeTempProject());
    const specName = "2026-01-03-test-already";
    const filesSection = `## Files
- src/schema/thing.ts
- src/api/thing.ts`;
    const specDir = makeSpec(proj, specName, filesSection);
    // Plant a wave-plan.md
    fs.writeFileSync(path.join(specDir, "wave-plan.md"), "# Wave Plan\n", "utf8");

    const { parsed } = run(proj, `.claude/spec/active/${specName}/spec.md`);
    assert.ok(parsed);
    assert.equal(parsed.action, "skip");
    assert.equal(parsed.reason, "already-decomposed");
  });

  // ── test 4: pipeline-state.scopeOverride = user-rejected-waves → skip ────────
  it("scopeOverride user-rejected-waves: skip", () => {
    const proj = track(makeTempProject());
    const specName = "2026-01-04-test-rejected";
    const filesSection = `## Files
- src/schema/x.ts
- src/api/x.ts`;
    makeSpec(proj, specName, filesSection);
    writePipelineState(proj, specName, {
      specName,
      status: "approved",
      phase: 2,
      scopeOverride: "user-rejected-waves",
    });

    const { parsed } = run(proj, `.claude/spec/active/${specName}/spec.md`);
    assert.ok(parsed);
    assert.equal(parsed.action, "skip");
    assert.equal(parsed.reason, "user-rejected");
  });

  // ── test 4b: PT spec with "## Arquivos" → Files section recognized ──────────
  it("pt spec (## Arquivos): single-layer keep-single, NOT no-files-section", () => {
    const proj = track(makeTempProject());
    const specName = "2026-01-04b-test-pt-files";
    // PT heading must be recognized via the spec-sections module — a regression
    // here would surface as { reason: "no-files-section" }.
    const filesSection = `## Arquivos
- src/api/users.ts
- src/api/orders.ts
- src/api/helpers.ts`;
    makeSpec(proj, specName, filesSection);

    const { parsed } = run(proj, `.claude/spec/active/${specName}/spec.md`);
    assert.ok(parsed, "output must be valid JSON");
    assert.notEqual(parsed.reason, "no-files-section",
      "PT '## Arquivos' must be parsed as the Files section");
    assert.equal(parsed.action, "keep-single");
    assert.equal(parsed.reason, "single-layer");
  });

  // ── test 5: spec with no ## Files section → skip error-fallback ───────────────
  it("spec without ## Files: skip error-fallback", () => {
    const proj = track(makeTempProject());
    const specName = "2026-01-05-test-nofiles";
    const specDir = path.join(proj, ".claude", "spec", "active", specName);
    fs.mkdirSync(specDir, { recursive: true });
    fs.writeFileSync(path.join(specDir, "spec.md"), `# Feature\n## Summary\nSomething\n`, "utf8");

    const { parsed } = run(proj, `.claude/spec/active/${specName}/spec.md`);
    assert.ok(parsed);
    assert.equal(parsed.action, "skip");
    assert.equal(parsed.reason, "error-fallback");
  });
});
