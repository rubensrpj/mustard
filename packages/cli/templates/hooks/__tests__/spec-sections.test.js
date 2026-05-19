#!/usr/bin/env bun
"use strict";

/**
 * Tests for scripts/_lib/spec-sections.js
 * Run: bun test templates/hooks/__tests__/spec-sections.test.js
 */

const { describe, it } = require("bun:test");
const assert = require("node:assert/strict");
const path = require("node:path");

const {
  SECTIONS,
  headingRegex,
  findSection,
  sectionHeading,
} = require(path.resolve(__dirname, "..", "..", "scripts", "_lib", "spec-sections.js"));

// ── headingRegex / findSection in both languages ───────────────────────────────

describe("spec-sections — every key resolves in pt and en", () => {
  for (const [key, variants] of Object.entries(SECTIONS)) {
    const enName = variants[0];
    const ptName = variants[variants.length - 1];

    it(`matches EN heading for "${key}"`, () => {
      const md = `# Spec\n\n## ${enName}\nbody for ${key}\n`;
      const found = findSection(md, key);
      assert.ok(found, `expected to find "${key}" via EN heading "${enName}"`);
      assert.ok(found.content.includes(`body for ${key}`));
    });

    it(`matches PT heading for "${key}"`, () => {
      const md = `# Spec\n\n## ${ptName}\ncorpo para ${key}\n`;
      const found = findSection(md, key);
      assert.ok(found, `expected to find "${key}" via PT heading "${ptName}"`);
      assert.ok(found.content.includes(`corpo para ${key}`));
    });
  }
});

// ── missing section ────────────────────────────────────────────────────────────

describe("spec-sections — missing section", () => {
  it("returns null when the section is absent", () => {
    const md = "# Spec\n\n## Summary\nonly a summary here\n";
    assert.equal(findSection(md, "acceptanceCriteria"), null);
  });

  it("returns null for empty or non-string input", () => {
    assert.equal(findSection("", "files"), null);
    assert.equal(findSection(null, "files"), null);
    assert.equal(findSection(undefined, "files"), null);
  });

  it("throws on an unknown key", () => {
    assert.throws(() => headingRegex("bogus"));
    assert.throws(() => sectionHeading("bogus"));
  });
});

// ── section boundaries ─────────────────────────────────────────────────────────

describe("spec-sections — boundaries", () => {
  it("stops content at the next ## heading", () => {
    const md =
      "# Spec\n\n## Files\n- a.ts\n- b.ts\n\n## Tasks\n- [ ] do it\n";
    const found = findSection(md, "files");
    assert.ok(found);
    assert.ok(found.content.includes("- a.ts"));
    assert.ok(found.content.includes("- b.ts"));
    assert.ok(!found.content.includes("do it"), "must not bleed into Tasks");
    // end index points exactly at the next heading
    assert.equal(md.slice(found.end).startsWith("## Tasks"), true);
  });

  it("captures a section at end-of-file (no following ## heading)", () => {
    const md = "# Spec\n\n## Summary\nlead\n\n## Files\n- last.ts\n- tail.ts";
    const found = findSection(md, "files");
    assert.ok(found);
    assert.ok(found.content.includes("- last.ts"));
    assert.ok(found.content.includes("- tail.ts"));
    assert.equal(found.end, md.length);
  });

  it("matches a heading with a parenthetical suffix", () => {
    const md =
      "# Spec\n\n## Acceptance Criteria (this pipeline)\n- AC1: it works\n";
    const found = findSection(md, "acceptanceCriteria");
    assert.ok(found, "suffix after heading name must still match");
    assert.ok(found.content.includes("AC1: it works"));
  });

  it("matches case-insensitively", () => {
    const md = "# Spec\n\n## files\n- x.ts\n";
    const found = findSection(md, "files");
    assert.ok(found);
    assert.ok(found.content.includes("- x.ts"));
  });

  it("matches the 'Checklist' EN alias for the tasks key", () => {
    const md = "# Spec\n\n## Checklist\n- [ ] step one\n";
    const found = findSection(md, "tasks");
    assert.ok(found);
    assert.ok(found.content.includes("step one"));
  });
});

// ── sectionHeading ─────────────────────────────────────────────────────────────

describe("spec-sections — sectionHeading", () => {
  it("returns EN heading by default", () => {
    assert.equal(sectionHeading("files"), "Files");
    assert.equal(sectionHeading("acceptanceCriteria"), "Acceptance Criteria");
  });

  it("returns EN heading when lang is 'en'", () => {
    assert.equal(sectionHeading("tasks", "en"), "Tasks");
    assert.equal(sectionHeading("boundaries", "en"), "Boundaries");
  });

  it("returns PT heading when lang is 'pt'", () => {
    assert.equal(sectionHeading("files", "pt"), "Arquivos");
    assert.equal(sectionHeading("tasks", "pt"), "Tarefas");
    assert.equal(
      sectionHeading("acceptanceCriteria", "pt"),
      "Critérios de Aceitação"
    );
    assert.equal(sectionHeading("decisions", "pt"), "Decisões não-óbvias");
    assert.equal(sectionHeading("symptom", "pt"), "Sintoma");
  });

  it("a PT heading written by sectionHeading is found by findSection", () => {
    for (const key of Object.keys(SECTIONS)) {
      const heading = sectionHeading(key, "pt");
      const md = `# Spec\n\n## ${heading}\nbody\n`;
      assert.ok(findSection(md, key), `round-trip failed for "${key}"`);
    }
  });
});
