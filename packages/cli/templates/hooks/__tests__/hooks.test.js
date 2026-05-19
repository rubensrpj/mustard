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

// ─── bash-safety.js ──────────────────────────────────────────────────────────

describe("bash-safety.js", () => {
  const hook = "bash-safety.js";

  it("should block rm -rf", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "rm -rf /" },
    });
    assert.ok(result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny decision for rm -rf");
  });

  it("should block force push", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "git push --force origin main" },
    });
    assert.ok(result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny decision for force push");
  });

  it("should allow normal git", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "git status" },
    });
    assert.equal(result.code, 0);
    // No output means approve (exit 0 silently)
    if (result.parsed) {
      assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, "deny");
    }
  });

  it("should allow dotnet build", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "dotnet build" },
    });
    assert.equal(result.code, 0);
    if (result.parsed) {
      assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, "deny");
    }
  });
});

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

// ─── spec-hygiene.js metrics emission ───────────────────────────────────────

describe("spec-hygiene.js metrics emission", () => {
  const hook = "spec-hygiene.js";

  it("should emit spec-hygiene-move with tokens_saved > 0 when an active spec is auto-moved", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "spec-hygiene-metrics-"));
    try {
      const specName = "2026-04-10-test-completed";
      const specDir = path.join(tmpDir, ".claude", "spec", "active", specName);
      fs.mkdirSync(specDir, { recursive: true });
      // A spec marked completed with all checklist items done → auto-move.
      const body = [
        "# Test",
        "",
        "### Status: completed | Phase: CLOSE | Scope: light",
        "",
        "## Checklist",
        "",
        "- [x] step one",
        "- [x] step two",
        "",
        // Pad the file so tokensSaved > 0 (file size / 4 must round up)
        "## Body",
        "lorem ipsum ".repeat(50),
        "",
      ].join("\n");
      fs.writeFileSync(path.join(specDir, "spec.md"), body);

      const result = await runHook(hook, {
        hook_event_name: "SessionStart",
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);

      // Spec must have moved
      const completedSpec = path.join(tmpDir, ".claude", "spec", "completed", specName, "spec.md");
      assert.ok(fs.existsSync(completedSpec), "spec must be relocated to completed/");

      // Metric must be emitted
      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "spec-hygiene-move.jsonl");
      assert.ok(fs.existsSync(metricsFile), "spec-hygiene-move.jsonl must exist");
      const entry = JSON.parse(fs.readFileSync(metricsFile, "utf8").trim().split("\n").pop());
      assert.equal(entry.event, "spec-hygiene-move");
      assert.ok(entry.tokens_saved > 0, "tokens_saved must be > 0");
      assert.ok(entry.tokens_affected > 0);
      assert.ok(/stale spec/i.test(entry.note));
      assert.ok(entry.from && entry.to, "extras (from/to) must be present");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── bash-native-redirect.js ────────────────────────────────────────────────

describe("bash-native-redirect.js", () => {
  const hook = "bash-native-redirect.js";

  it("should deny simple grep command", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "grep -r pattern src/" },
    });
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny for grep command"
    );
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecisionReason?.includes("Grep"),
      "Should suggest Grep tool"
    );
  });

  it("should deny cat and suggest Read", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "cat src/main.ts" },
    });
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny for cat"
    );
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecisionReason?.includes("Read"),
      "Should suggest Read tool"
    );
  });

  it("should deny ls and suggest Glob", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "ls -la src/" },
    });
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny for ls"
    );
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecisionReason?.includes("Glob"),
      "Should suggest Glob"
    );
  });

  it("should deny head/tail/find", async () => {
    for (const cmd of ["head -20 file.txt", "tail -50 app.log", "find . -name '*.ts'"]) {
      const result = await runHook(hook, {
        tool_name: "Bash",
        tool_input: { command: cmd },
      });
      assert.ok(
        result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
        `Expected deny for: ${cmd}`
      );
    }
  });

  it("should allow piped commands through", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "grep foo bar.txt | wc -l" },
    });
    assert.equal(result.code, 0);
    if (result.parsed?.hookSpecificOutput) {
      assert.notEqual(result.parsed.hookSpecificOutput.permissionDecision, "deny");
    }
  });

  it("should allow chained commands through", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "grep foo bar.txt && echo found" },
    });
    assert.equal(result.code, 0);
    if (result.parsed?.hookSpecificOutput) {
      assert.notEqual(result.parsed.hookSpecificOutput.permissionDecision, "deny");
    }
  });

  it("should warn on piped commands with redirectable first segment", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "grep foo bar.txt | sort | uniq" },
    });
    assert.equal(result.code, 0);
    const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
    assert.ok(ctx.includes("Grep"), "Should suggest Grep for piped grep command");
    assert.ok(ctx.includes("Native Tool Redirect"), "Should include redirect warning");
    assert.equal(result.parsed?.hookSpecificOutput?.permissionDecision, "allow", "Should allow, not deny");
  });

  it("should allow rtk-prefixed commands", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "rtk grep -r pattern src/" },
    });
    assert.equal(result.code, 0);
    if (result.parsed?.hookSpecificOutput) {
      assert.notEqual(result.parsed.hookSpecificOutput.permissionDecision, "deny");
    }
  });

  it("should allow non-mapped commands (git, npm)", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "git status" },
    });
    assert.equal(result.code, 0);
    if (result.parsed?.hookSpecificOutput) {
      assert.notEqual(result.parsed.hookSpecificOutput.permissionDecision, "deny");
    }
  });

  it("should allow sed -i (write operation)", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "sed -i 's/old/new/g' file.txt" },
    });
    assert.equal(result.code, 0);
    if (result.parsed?.hookSpecificOutput) {
      assert.notEqual(result.parsed.hookSpecificOutput.permissionDecision, "deny");
    }
  });

  it("should allow commands with output redirect", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "cat file.txt > output.txt" },
    });
    assert.equal(result.code, 0);
    if (result.parsed?.hookSpecificOutput) {
      assert.notEqual(result.parsed.hookSpecificOutput.permissionDecision, "deny");
    }
  });

  it("should handle env var prefix before command", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "NODE_ENV=test grep pattern file.txt" },
    });
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny for grep after env prefix"
    );
  });

  it("should strip 2>/dev/null before analysis", async () => {
    const result = await runHook(hook, {
      tool_name: "Bash",
      tool_input: { command: "grep pattern file 2>/dev/null" },
    });
    assert.ok(
      result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny even with 2>/dev/null"
    );
  });

  it("should emit metrics on redirect", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "bash-redir-metrics-"));
    try {
      await runHook(hook, {
        tool_name: "Bash",
        tool_input: { command: "grep pattern file.txt" },
      }, { cwd: tmpDir, projectDir: tmpDir });

      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "bash-native-redirect.jsonl");
      assert.ok(fs.existsSync(metricsFile), "Metrics file should exist");
      const entry = JSON.parse(fs.readFileSync(metricsFile, "utf8").trim());
      assert.equal(entry.event, "bash-native-redirect");
      assert.equal(entry.note, "redirected");
      assert.equal(entry.from, "grep");
      assert.equal(entry.to, "Grep");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── knowledge-extract prescriptions ────────────────────────────────────────

describe("knowledge-extract prescriptions", () => {
  const {
    extractPatternsFromStates,
    extractFrictionFromStates,
    derivePrescription,
  } = require("../_lib/knowledge-extract.js");

  it("extractPatternsFromStates should NOT emit friction entries (atrito is not knowledge)", () => {
    // Friction signals (high-hook-retry, heavy-pipeline) moved to friction.json.
    const states = [{
      specName: "login-feature",
      metrics: { retries: 9, apiCalls: 99, toolBreakdown: { Bash: 9, Edit: 17, Agent: 3 } },
    }];
    const patterns = extractPatternsFromStates(states);
    assert.equal(patterns.length, 0, "knowledge.json must stay free of friction telemetry");
  });

  it("should emit L0-violation prescription when Bash+Edit dominate over Agent with high retries", () => {
    // Bash(9) + Edit(17) = 26; Agent(3)*3 = 9; 26 > 9. retries=4 > 2.
    const states = [{
      specName: "login-feature",
      metrics: {
        retries: 4,
        apiCalls: 40,
        toolBreakdown: { Bash: 9, Edit: 17, Agent: 3 },
      },
    }];

    const friction = extractFrictionFromStates(states);
    // retries > 2 triggers the high-hook-retry friction entry
    const retryEntry = friction.find(p => p.name === "high-hook-retry-login-feature");
    assert.ok(retryEntry, "Expected high-hook-retry entry");
    assert.equal(retryEntry.type, "friction", "Friction entries carry type 'friction'");
    assert.equal(retryEntry.retryCount, 4, "Honest retryCount field replaces occurrences");
    assert.equal(retryEntry.occurrences, undefined, "No meaningless occurrences field");
    assert.ok(retryEntry.prescription, "Expected prescription field");
    assert.ok(
      /delegate investigation via Task\(general-purpose\)/.test(retryEntry.prescription),
      "Prescription should instruct delegation via Task(general-purpose)"
    );
    assert.ok(retryEntry.tags.includes("prescriptive"), "Tags should include 'prescriptive'");
    assert.ok(retryEntry.tags.includes("hook-retry"));
    assert.ok(retryEntry.tags.includes("pipeline"));
    assert.ok(retryEntry.tags.includes("friction"));
    assert.ok(retryEntry.description.includes("4 hook-level retries"));
  });

  it("should emit fragmentation prescription when apiCalls > 50 AND retries > 3", () => {
    // apiCalls=81, retries=5, balanced tool usage (no L0-violation match)
    const states = [{
      specName: "big-refactor",
      metrics: {
        retries: 5,
        apiCalls: 81,
        toolBreakdown: { Bash: 10, Edit: 10, Agent: 8 },
      },
    }];

    const friction = extractFrictionFromStates(states);
    // apiCalls > 50 triggers heavy-pipeline; retries > 2 also triggers high-hook-retry.
    const heavyEntry = friction.find(p => p.name === "heavy-pipeline-big-refactor");
    assert.ok(heavyEntry, "Expected heavy-pipeline entry");
    assert.equal(heavyEntry.type, "friction", "Friction entries carry type 'friction'");
    assert.equal(heavyEntry.apiCalls, 81, "Honest apiCalls field");
    assert.equal(heavyEntry.occurrences, undefined, "No meaningless occurrences field");
    assert.ok(heavyEntry.prescription, "Expected prescription field");
    assert.ok(
      /split into at least 2 smaller pipelines/.test(heavyEntry.prescription),
      "Prescription should suggest splitting into smaller pipelines"
    );
    assert.ok(heavyEntry.tags.includes("prescriptive"));
    assert.ok(heavyEntry.tags.includes("optimization"));
    assert.ok(heavyEntry.tags.includes("pipeline"));
    assert.ok(heavyEntry.description.includes("81 API calls"));
  });

  it("should emit reactive-iteration prescription when Edit > 15 and Write < 3", () => {
    // Edit=20 > 15, Write=1 < 3, retries=3 to trigger the high-hook-retry entry
    // (needs retries > 2 OR apiCalls > 50 to produce any entry at all).
    // Pick retries=3 and small Bash/Agent to avoid L0-violation heuristic dominance
    // but note: the heuristic checks order — L0 fires first if bash+edit>3*agent AND retries>2.
    // Use bash=0, edit=20, agent=10 so bash+edit=20 vs 3*agent=30 → 20 < 30 → L0 skipped.
    const states = [{
      specName: "tweak-hell",
      metrics: {
        retries: 3,
        apiCalls: 40,
        toolBreakdown: { Bash: 0, Edit: 20, Write: 1, Agent: 10 },
      },
    }];

    const friction = extractFrictionFromStates(states);
    const retryEntry = friction.find(p => p.name === "high-hook-retry-tweak-hell");
    assert.ok(retryEntry, "Expected high-hook-retry entry");
    assert.ok(retryEntry.prescription, "Expected prescription field");
    assert.ok(
      /investigate with Read\+Grep BEFORE editing/.test(retryEntry.prescription),
      "Prescription should instruct Read+Grep investigation before editing"
    );
    assert.ok(retryEntry.tags.includes("prescriptive"));
  });

  it("should NOT add prescription or prescriptive tag when no heuristic matches", () => {
    // retries=3 to trigger high-hook-retry entry, but balanced tools so none of the
    // heuristics fire (edit<=15, apiCalls<=50, bash+edit not >3*agent).
    const states = [{
      specName: "mild-case",
      metrics: {
        retries: 3,
        apiCalls: 10,
        toolBreakdown: { Bash: 2, Edit: 2, Agent: 5, Write: 1 },
      },
    }];

    const friction = extractFrictionFromStates(states);
    const retryEntry = friction.find(p => p.name === "high-hook-retry-mild-case");
    assert.ok(retryEntry, "Expected high-hook-retry entry");
    assert.equal(retryEntry.prescription, undefined, "No prescription when no heuristic matches");
    assert.ok(!retryEntry.tags.includes("prescriptive"),
      "'prescriptive' tag must NOT be added when no prescription");
    assert.ok(retryEntry.tags.includes("hook-retry"));
    assert.ok(retryEntry.description);
    assert.equal(retryEntry.source, "session-knowledge");
  });

  it("derivePrescription should return null for empty / trivial metrics", () => {
    assert.equal(derivePrescription({}), null);
    assert.equal(derivePrescription({ retries: 1, apiCalls: 10, toolBreakdown: {} }), null);
    assert.equal(derivePrescription(null), null);
  });
});

// ─── rtk-rewrite.js (PR1 — honest metrics) ──────────────────────────────────
//
// Source-level assertions. We can't reliably run the hook end-to-end in CI
// because it requires `rtk` to be installed AND to produce a rewrite. The
// important invariant for PR1 is that the source no longer contains the
// fake heuristic that used to compute tokens_saved from cmd.length.

describe("rtk-rewrite.js (source-level)", () => {
  const src = fs.readFileSync(path.join(HOOKS_DIR, "rtk-rewrite.js"), "utf8");

  it("must not contain SAVINGS_RATES heuristic table", () => {
    assert.ok(
      !/SAVINGS_RATES/.test(src),
      "SAVINGS_RATES heuristic removed — real numbers come from rtk-gain"
    );
  });

  it("must not pass tokensSaved to emitMetric", () => {
    // Find the emitMetric call block and ensure tokensSaved is absent.
    const match = src.match(/emitMetric\(['"]rtk-rewrite['"][\s\S]*?\)\);/);
    assert.ok(match, "rtk-rewrite emitMetric call must be present");
    assert.ok(
      !/tokensSaved/.test(match[0]),
      "tokensSaved must not be emitted by rtk-rewrite (PR1)"
    );
  });

  it("must still emit tokensAffected", () => {
    const match = src.match(/emitMetric\(['"]rtk-rewrite['"][\s\S]*?\)\);/);
    assert.ok(match, "rtk-rewrite emitMetric call must be present");
    assert.ok(
      /tokensAffected/.test(match[0]),
      "tokensAffected is the only telemetry rtk-rewrite should emit"
    );
  });
});
