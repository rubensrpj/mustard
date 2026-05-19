#!/usr/bin/env bun
"use strict";

/**
 * Tests for wave-size-check.js
 * Run: bun test templates/hooks/__tests__/wave-size-check.test.js
 */

const { describe, it, afterEach } = require('bun:test');
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const SCRIPT = path.resolve(__dirname, "..", "..", "scripts", "wave-size-check.js");

// ── helpers ────────────────────────────────────────────────────────────────────

function makeSpecDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "mustard-wavesize-"));
}

function writeWavePlan(specDir) {
  fs.writeFileSync(
    path.join(specDir, "wave-plan.md"),
    "# Wave Plan\n### Status: active | Phase: EXECUTE | Scope: wave-plan\n\n" +
      "## Waves\n| Wave | Pasta |\n| ---- | ----- |\n",
    "utf8"
  );
}

function writeWaveSpec(specDir, folder, files, status = "queued") {
  const waveDir = path.join(specDir, folder);
  fs.mkdirSync(waveDir, { recursive: true });
  const fileList = files.map((f) => `- ${f}`).join("\n");
  fs.writeFileSync(
    path.join(waveDir, "spec.md"),
    `# Wave\n### Status: ${status} | Phase: EXECUTE | Scope: full\n\n` +
      `## Summary\nbody\n\n## Files\n${fileList}\n\n## Tasks\n- [ ] do it\n`,
    "utf8"
  );
}

function run(specDir, env = {}) {
  const result = spawnSync(
    process.execPath,
    specDir === null ? [SCRIPT] : [SCRIPT, "--spec-dir", specDir],
    {
      encoding: "utf8",
      timeout: 20000,
      env: { ...process.env, MUSTARD_DISABLED_HOOKS: "all", ...env },
    }
  );
  let parsed = null;
  try { parsed = JSON.parse(result.stdout.trim()); } catch (_) {}
  return { code: result.status, stdout: result.stdout.trim(), parsed };
}

function cleanup(dir) {
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch (_) {}
}

// ── tests ──────────────────────────────────────────────────────────────────────

describe("wave-size-check", () => {
  const _dirs = [];
  afterEach(() => { while (_dirs.length) cleanup(_dirs.pop()); });
  const track = (d) => { _dirs.push(d); return d; };

  // (a) wave-plan with one large wave → oversized:true, counted in oversizedCount
  it("flags an oversized wave", () => {
    const specDir = track(makeSpecDir());
    writeWavePlan(specDir);
    // 14 backend files → multi-layer + file-count over default limit 10
    const big = [];
    for (let i = 0; i < 9; i++) big.push(`backend/api/handler${i}.ts`);
    for (let i = 0; i < 5; i++) big.push(`backend/schema/entity${i}.ts`);
    writeWaveSpec(specDir, "wave-1-backend", big);

    const { code, parsed } = run(specDir);
    assert.equal(code, 0);
    assert.ok(parsed, "output must be valid JSON");
    assert.equal(parsed.action, "audited");
    assert.equal(parsed.oversizedCount, 1);
    const w1 = parsed.waves.find((w) => w.wave === 1);
    assert.equal(w1.oversized, true);
    assert.equal(w1.fileCount, 14);
    assert.ok(/file-count:14>10/.test(w1.reason), `reason: ${w1.reason}`);
  });

  // (b) wave-plan with only small waves → oversizedCount:0
  it("reports oversizedCount 0 when all waves are small", () => {
    const specDir = track(makeSpecDir());
    writeWavePlan(specDir);
    // 3 files, single layer (all api) → not oversized
    writeWaveSpec(specDir, "wave-1-api", [
      "src/api/a.ts",
      "src/api/b.ts",
      "src/api/c.ts",
    ]);

    const { code, parsed } = run(specDir);
    assert.equal(code, 0);
    assert.equal(parsed.action, "audited");
    assert.equal(parsed.oversizedCount, 0);
    const w1 = parsed.waves.find((w) => w.wave === 1);
    assert.equal(w1.oversized, false);
  });

  // (c) missing --spec-dir / nonexistent dir → skip, exit 0 (fail-open)
  it("missing --spec-dir → skip, exit 0", () => {
    const { code, parsed } = run(null);
    assert.equal(code, 0);
    assert.ok(parsed);
    assert.equal(parsed.action, "skip");
    assert.equal(parsed.reason, "no-spec-dir-arg");
  });

  it("nonexistent spec-dir → skip, exit 0", () => {
    const { code, parsed } = run(path.join(os.tmpdir(), "mustard-no-such-dir-xyz"));
    assert.equal(code, 0);
    assert.ok(parsed);
    assert.equal(parsed.action, "skip");
    assert.equal(parsed.reason, "error-fallback");
  });

  it("spec-dir without wave-plan.md → skip not-a-wave-plan", () => {
    const specDir = track(makeSpecDir());
    fs.writeFileSync(
      path.join(specDir, "spec.md"),
      "# Spec\n### Status: approved | Phase: PLAN | Scope: full\n",
      "utf8"
    );
    const { code, parsed } = run(specDir);
    assert.equal(code, 0);
    assert.equal(parsed.action, "skip");
    assert.equal(parsed.reason, "not-a-wave-plan");
  });

  // (d) MUSTARD_WAVE_SIZE_LIMIT respected
  it("respects MUSTARD_WAVE_SIZE_LIMIT", () => {
    const specDir = track(makeSpecDir());
    writeWavePlan(specDir);
    // 4 files, single layer (all api) → not oversized at default 10
    writeWaveSpec(specDir, "wave-1-api", [
      "src/api/a.ts",
      "src/api/b.ts",
      "src/api/c.ts",
      "src/api/d.ts",
    ]);

    // limit 3 → 4 files exceeds → oversized
    const low = run(specDir, { MUSTARD_WAVE_SIZE_LIMIT: "3" });
    assert.equal(low.parsed.limit, 3);
    assert.equal(low.parsed.oversizedCount, 1);
    assert.ok(/file-count:4>3/.test(low.parsed.waves[0].reason));

    // limit 1 → floored to 3 (floor enforced)
    const floored = run(specDir, { MUSTARD_WAVE_SIZE_LIMIT: "1" });
    assert.equal(floored.parsed.limit, 3);

    // limit 99 → 4 files well under → not oversized
    const high = run(specDir, { MUSTARD_WAVE_SIZE_LIMIT: "99" });
    assert.equal(high.parsed.limit, 99);
    assert.equal(high.parsed.oversizedCount, 0);
  });

  // stub wave (no spec.md) → status "stub"
  it("classifies a wave with no spec.md as stub", () => {
    const specDir = track(makeSpecDir());
    writeWavePlan(specDir);
    writeWaveSpec(specDir, "wave-1-api", ["src/api/a.ts"]);
    // wave-2 folder with no spec.md
    fs.mkdirSync(path.join(specDir, "wave-2-backend"), { recursive: true });

    const { parsed } = run(specDir);
    assert.equal(parsed.action, "audited");
    const w2 = parsed.waves.find((w) => w.wave === 2);
    assert.equal(w2.status, "stub");
  });
});
