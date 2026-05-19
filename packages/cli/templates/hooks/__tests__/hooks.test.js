#!/usr/bin/env bun
/**
 * Tests for Mustard hooks using node:test and node:assert.
 * Run with: bun test .claude/hooks/__tests__/hooks.test.js
 */

const { describe, it } = require('bun:test');
const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

const HOOKS_DIR = path.resolve(__dirname, "..");
const PROJECT_DIR = path.resolve(__dirname, "..", "..", "..");

function runHook(hookFile, inputObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const cwd = opts.cwd || PROJECT_DIR;
    const child = spawn(process.execPath, [path.join(HOOKS_DIR, hookFile)], {
      cwd,
      env: {
        ...process.env,
        CLAUDE_PROJECT_DIR: opts.projectDir || PROJECT_DIR,
      },
      stdio: ["pipe", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";

    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));

    child.on("error", reject);
    child.on("close", (code) => {
      let parsed = null;
      if (stdout.trim()) {
        try {
          parsed = JSON.parse(stdout.trim());
        } catch {
          // not JSON
        }
      }
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });

    child.stdin.write(JSON.stringify(inputObj));
    child.stdin.end();
  });
}

// guard-verify.js was ported to the Rust `post_edit` module (b3 Wave 4);
// its parity tests now live in `packages/rt/src/hooks/post_edit.rs`.

// bash-safety.js was ported to the Rust `bash_guard` module (b3 Wave 1); its
// parity tests now live in `packages/rt/src/hooks/bash_guard.rs`.

// file-guard.js was ported to the Rust `path_guard` module and
// enforce-registry.js to the Rust `enforce_registry` module (b3 Wave 4);
// their parity tests now live in `packages/rt/src/hooks/{path_guard,enforce_registry}.rs`.

// ─── memory.js agent ────────────────────────────────────────────────────────

describe("memory-write.js", () => {
  const SCRIPTS_DIR = path.resolve(__dirname, "..", "..", "scripts");

  function runScript(inputObj, opts = {}) {
    return new Promise((resolve, reject) => {
      const cwd = opts.cwd || PROJECT_DIR;
      const child = spawn(process.execPath, [path.join(SCRIPTS_DIR, "memory.js"), "agent"], {
        cwd,
        stdio: ["pipe", "pipe", "pipe"],
      });
      let stdout = "";
      let stderr = "";
      child.stdout.on("data", (d) => (stdout += d));
      child.stderr.on("data", (d) => (stderr += d));
      child.on("error", reject);
      child.on("close", (code) => {
        resolve({ code, stdout: stdout.trim(), stderr: stderr.trim() });
      });
      child.stdin.write(JSON.stringify(inputObj));
      child.stdin.end();
    });
  }

  function runScriptArg(inputObj, opts = {}) {
    return new Promise((resolve, reject) => {
      const cwd = opts.cwd || PROJECT_DIR;
      const child = spawn(
        process.execPath,
        [path.join(SCRIPTS_DIR, "memory.js"), "agent", "--json", JSON.stringify(inputObj)],
        { cwd, stdio: ["ignore", "pipe", "pipe"] }
      );
      let stdout = "";
      let stderr = "";
      child.stdout.on("data", (d) => (stdout += d));
      child.stderr.on("data", (d) => (stderr += d));
      child.on("error", reject);
      child.on("close", (code) => {
        resolve({ code, stdout: stdout.trim(), stderr: stderr.trim() });
      });
    });
  }

  it("should create memory entry and index", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "mem-test-"));
    const memDir = path.join(tmpDir, ".claude", ".agent-memory");
    try {
      const result = await runScript({
        cwd: tmpDir,
        agent_type: "test-impl",
        wave: 1,
        pipeline: "test-pipeline",
        summary: "Created TestService.cs with CQRS pattern.",
        details: { files_modified: ["TestService.cs"] },
      });
      assert.equal(result.code, 0, `Exit code should be 0, stderr: ${result.stderr}`);
      assert.ok(fs.existsSync(memDir), "Memory dir should exist");
      const indexPath = path.join(memDir, "_index.json");
      assert.ok(fs.existsSync(indexPath), "Index file should exist");
      const index = JSON.parse(fs.readFileSync(indexPath, "utf8"));
      assert.equal(index.length, 1, "Index should have 1 entry");
      assert.equal(index[0].agent_type, "test-impl");
      assert.equal(index[0].wave, 1);
      assert.ok(index[0].summary.includes("CQRS"), "Summary should contain CQRS");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should cap index at 20 entries", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "mem-test-"));
    try {
      // Write 22 entries
      for (let i = 0; i < 22; i++) {
        await runScript({
          cwd: tmpDir,
          agent_type: `agent-${i}`,
          wave: i,
          pipeline: "test-pipeline",
          summary: `Entry ${i}`,
          details: {},
        });
      }
      const indexPath = path.join(tmpDir, ".claude", ".agent-memory", "_index.json");
      const index = JSON.parse(fs.readFileSync(indexPath, "utf8"));
      assert.ok(index.length <= 20, `Index should be capped at 20, got ${index.length}`);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should exit 0 on invalid input (fail-open)", async () => {
    const result = await runScript("not valid json");
    assert.equal(result.code, 0, "Should exit 0 even on bad input");
  });

  it("should accept input via --json arg (Windows-friendly mode)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "mem-test-arg-"));
    const memDir = path.join(tmpDir, ".claude", ".agent-memory");
    try {
      const result = await runScriptArg({
        cwd: tmpDir,
        agent_type: "arg-impl",
        wave: 2,
        pipeline: "arg-pipeline",
        summary: "Wrote via --json arg mode.",
        details: { mode: "arg" },
      });
      assert.equal(result.code, 0, `Exit code should be 0, stderr: ${result.stderr}`);
      assert.ok(fs.existsSync(memDir), "Memory dir should exist");
      const indexPath = path.join(memDir, "_index.json");
      assert.ok(fs.existsSync(indexPath), "Index file should exist");
      const index = JSON.parse(fs.readFileSync(indexPath, "utf8"));
      assert.equal(index.length, 1, "Index should have 1 entry");
      assert.equal(index[0].agent_type, "arg-impl");
      assert.equal(index[0].wave, 2);
      assert.ok(index[0].summary.includes("arg mode"), "Summary should round-trip");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── _lib/metrics-emit.js ───────────────────────────────────────────────────

describe("_lib/metrics-emit.js", () => {
  const { emitMetric } = require("../_lib/metrics-emit.js");

  it("should append a valid JSONL line and create the metrics dir", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "metrics-emit-"));
    try {
      emitMetric("unit-test-event", {
        tokensAffected: 123,
        tokensSaved: 45,
        note: "hello",
        extras: { source: "test", count: 7 },
        cwd: tmpDir,
      });
      const file = path.join(tmpDir, ".claude", ".metrics", "unit-test-event.jsonl");
      assert.ok(fs.existsSync(file), "JSONL file should be created");
      const lines = fs.readFileSync(file, "utf8").trim().split("\n");
      assert.equal(lines.length, 1, "should have one line");
      const entry = JSON.parse(lines[0]);
      assert.equal(entry.event, "unit-test-event");
      assert.equal(entry.tokens_affected, 123);
      assert.equal(entry.tokens_saved, 45);
      assert.equal(entry.note, "hello");
      assert.equal(entry.source, "test");
      assert.equal(entry.count, 7);
      assert.ok(entry.ts, "ts must be set");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should fail-silent when the cwd is unwritable / invalid", () => {
    // Pointing cwd at an existing FILE (not dir) makes mkdir/append fail.
    const tmpFile = path.join(os.tmpdir(), `metrics-emit-fail-${Date.now()}.tmp`);
    fs.writeFileSync(tmpFile, "not-a-dir");
    try {
      // Must NOT throw
      assert.doesNotThrow(() => {
        emitMetric("should-not-throw", {
          tokensAffected: 1,
          tokensSaved: 1,
          note: "x",
          cwd: tmpFile, // a file, not a dir → mkdir under it will fail
        });
      });
    } finally {
      fs.rmSync(tmpFile, { force: true });
    }
  });

  it("should default missing fields to safe values", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "metrics-emit-defaults-"));
    try {
      emitMetric("defaults-event", { cwd: tmpDir });
      const file = path.join(tmpDir, ".claude", ".metrics", "defaults-event.jsonl");
      const entry = JSON.parse(fs.readFileSync(file, "utf8").trim());
      assert.equal(entry.tokens_affected, 0);
      assert.equal(entry.tokens_saved, 0);
      assert.equal(entry.note, "");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// spec-hygiene.js was ported to the Rust `session_start` module (b3 Wave 5);
// its auto-move parity tests now live in
// `packages/rt/src/hooks/session_start.rs`.

// bash-native-redirect.js and rtk-rewrite.js were ported to the Rust
// `bash_guard` module (b3 Waves 1-2); their parity tests now live in
// `packages/rt/src/hooks/bash_guard.rs`. The `_lib/knowledge-extract.js`
// friction/prescription logic was ported to the Rust `knowledge` module (b3
// Wave 5); its parity tests now live in `packages/rt/src/hooks/knowledge.rs`.
