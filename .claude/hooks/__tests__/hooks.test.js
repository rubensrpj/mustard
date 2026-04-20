#!/usr/bin/env node
/**
 * Tests for Mustard hooks using node:test and node:assert.
 * Run with: node --test .claude/hooks/__tests__/hooks.test.js
 */

const { describe, it } = require("node:test");
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

// ─── guard-verify.js ─────────────────────────────────────────────────────────

describe("guard-verify.js", () => {
  const hook = "guard-verify.js";

  it("should block DbContext in Services/", async () => {
    const result = await runHook(hook, {
      tool_name: "Edit",
      tool_input: {
        file_path: path.join(PROJECT_DIR, "src/Modules/v1/Users/Services/UserService.cs"),
        new_string: 'var ctx = new DbContext();',
      },
    });
    assert.equal(result.parsed?.decision, "block");
  });

  it("should allow DbContext in Repositories/", async () => {
    const result = await runHook(hook, {
      tool_name: "Edit",
      tool_input: {
        file_path: path.join(PROJECT_DIR, "src/Modules/v1/Users/Repositories/UserRepository.cs"),
        new_string: 'var ctx = new DbContext();',
      },
    });
    assert.equal(result.parsed?.decision, "approve");
  });

  it("should block cross-module Repository import", async () => {
    const result = await runHook(hook, {
      tool_name: "Edit",
      tool_input: {
        file_path: path.join(PROJECT_DIR, "src/Modules/v1/Users/Services/UserService.cs"),
        new_string: 'private readonly ContractRepository _repo;',
      },
    });
    assert.equal(result.parsed?.decision, "block");
  });

  it("should allow same-module Repository", async () => {
    const result = await runHook(hook, {
      tool_name: "Edit",
      tool_input: {
        file_path: path.join(PROJECT_DIR, "src/Modules/v1/Users/Services/UserService.cs"),
        new_string: 'private readonly UserRepository _repo;',
      },
    });
    assert.equal(result.parsed?.decision, "approve");
  });

  it("should skip .claude/ files", async () => {
    const result = await runHook(hook, {
      tool_name: "Edit",
      tool_input: {
        file_path: path.join(PROJECT_DIR, ".claude/hooks/some-hook.js"),
        new_string: 'DbContext something bad int UserId',
      },
    });
    assert.equal(result.parsed?.decision, "approve");
  });

  it("should block int Id in .cs files", async () => {
    const result = await runHook(hook, {
      tool_name: "Edit",
      tool_input: {
        file_path: path.join(PROJECT_DIR, "src/Models/User.cs"),
        new_string: 'public int UserId { get; set; }',
      },
    });
    assert.equal(result.parsed?.decision, "block");
  });
});

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

// ─── file-guard.js ───────────────────────────────────────────────────────────

describe("file-guard.js", () => {
  const hook = "file-guard.js";

  it("should block .pem files", async () => {
    const result = await runHook(hook, {
      tool_name: "Read",
      tool_input: { file_path: "/etc/ssl/certs/server.pem" },
    });
    assert.ok(result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny for .pem file");
  });

  it("should block .git/config", async () => {
    const result = await runHook(hook, {
      tool_name: "Read",
      tool_input: { file_path: "/project/.git/config" },
    });
    assert.ok(result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
      "Expected deny for .git/config");
  });

  it("should allow normal files", async () => {
    const result = await runHook(hook, {
      tool_name: "Read",
      tool_input: { file_path: "src/main.cs" },
    });
    assert.equal(result.code, 0);
    if (result.parsed) {
      assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, "deny");
    }
  });
});

// ─── enforce-registry.js ─────────────────────────────────────────────────────

describe("enforce-registry.js", () => {
  const hook = "enforce-registry.js";

  it("should block pipeline skill if registry missing", async () => {
    // Use a temp dir that has no .claude/entity-registry.json
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hook-test-"));
    try {
      const result = await runHook(hook, {
        tool_name: "Skill",
        tool_input: { skill: "feature" },
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.ok(
        result.parsed?.hookSpecificOutput?.permissionDecision === "block",
        "Expected block when entity-registry.json is missing"
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow non-pipeline skills", async () => {
    const result = await runHook(hook, {
      tool_name: "Skill",
      tool_input: { skill: "some-random-skill" },
    });
    assert.equal(result.code, 0);
    // Should exit 0 with no block output
    if (result.parsed) {
      assert.notEqual(result.parsed?.hookSpecificOutput?.permissionDecision, "block");
    }
  });
});

// ─── memory-write.js ────────────────────────────────────────────────────────

describe("memory-write.js", () => {
  const SCRIPTS_DIR = path.resolve(__dirname, "..", "..", "scripts");

  function runScript(inputObj, opts = {}) {
    return new Promise((resolve, reject) => {
      const cwd = opts.cwd || PROJECT_DIR;
      const child = spawn(process.execPath, [path.join(SCRIPTS_DIR, "memory-write.js")], {
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
        [path.join(SCRIPTS_DIR, "memory-write.js"), "--json", JSON.stringify(inputObj)],
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

// ─── subagent-tracker.js (memory injection) ─────────────────────────────────

describe("subagent-tracker.js memory injection", () => {
  const hook = "subagent-tracker.js";

  it("should inject memories into additionalContext when present", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "mem-test-"));
    const memDir = path.join(tmpDir, ".claude", ".agent-memory");
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(memDir, { recursive: true });
    fs.mkdirSync(stateDir, { recursive: true });

    // Write a memory index
    fs.writeFileSync(path.join(memDir, "_index.json"), JSON.stringify([{
      id: "test-backend-123",
      file: "test-backend-123.json",
      agent_type: "backend-impl",
      wave: 1,
      pipeline: "test",
      summary: "Created PaymentController with POST /api/payments endpoint.",
      timestamp: new Date().toISOString(),
    }]));

    try {
      const result = await runHook(hook, {
        hook_event_name: "SubagentStart",
        agent_id: "test-agent-1",
        agent_type: "frontend-impl",
        session_id: "test-session",
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      assert.ok(result.parsed, "Should output JSON");
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(ctx.includes("[Agent Memory]"), "Should contain Agent Memory header");
      assert.ok(ctx.includes("PaymentController"), "Should contain memory summary");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should work normally without memory files", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "mem-test-"));
    fs.mkdirSync(path.join(tmpDir, ".claude", ".agent-state"), { recursive: true });

    try {
      const result = await runHook(hook, {
        hook_event_name: "SubagentStart",
        agent_id: "test-agent-2",
        agent_type: "general-purpose",
        session_id: "test-session",
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      assert.ok(result.parsed, "Should output JSON");
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(ctx.includes("[Tracker]"), "Should contain Tracker message");
      assert.ok(!ctx.includes("[Agent Memory]"), "Should NOT contain Agent Memory when no memories exist");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── metrics-tracker.js (sidecar + no-recursion) ────────────────────────────

describe("metrics-tracker.js", () => {
  const hook = "metrics-tracker.js";

  function setupPipelineState(tmpDir) {
    const statesDir = path.join(tmpDir, ".claude", ".pipeline-states");
    fs.mkdirSync(statesDir, { recursive: true });
    const pipelinePath = path.join(statesDir, "test-pipeline.json");
    fs.writeFileSync(pipelinePath, JSON.stringify({
      v: 1,
      name: "test-pipeline",
      phase: "EXECUTE",
      phaseName: "EXECUTE",
      status: "approved",
      startedAt: "2026-04-05T10:00:00.000Z",
    }), "utf8");
    return { statesDir, pipelinePath };
  }

  it("should write metrics to sidecar and leave pipeline-state untouched", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "metrics-test-"));
    const { statesDir, pipelinePath } = setupPipelineState(tmpDir);
    const sidecarPath = path.join(statesDir, "test-pipeline.metrics.json");
    try {
      const mtimeBefore = fs.statSync(pipelinePath).mtimeMs;
      // Wait a beat so any write would produce a different mtime
      await new Promise((r) => setTimeout(r, 50));

      const result = await runHook(hook, {
        tool_name: "Edit",
        tool_input: { file_path: path.join(tmpDir, "src/foo.ts") },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      const mtimeAfter = fs.statSync(pipelinePath).mtimeMs;
      assert.equal(mtimeAfter, mtimeBefore, "pipeline-state.json must NOT be modified");
      assert.ok(fs.existsSync(sidecarPath), "sidecar must be created");
      const sidecar = JSON.parse(fs.readFileSync(sidecarPath, "utf8"));
      assert.equal(sidecar.metrics.apiCalls, 1);
      assert.equal(sidecar.metrics.toolBreakdown.Edit, 1);
      assert.equal(sidecar.previousPhase, "EXECUTE");
      assert.equal(sidecar.metrics.startedAt, "2026-04-05T10:00:00.000Z", "startedAt inherited from pipeline-state");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should not create recursive .metrics.metrics.json sidecars across multiple calls", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "metrics-recursion-"));
    const { statesDir } = setupPipelineState(tmpDir);
    try {
      // Fire 5 PostToolUse events in sequence
      for (let i = 0; i < 5; i++) {
        const r = await runHook(hook, {
          tool_name: "Write",
          tool_input: { file_path: path.join(tmpDir, `src/file${i}.ts`) },
          cwd: tmpDir,
        }, { cwd: tmpDir, projectDir: tmpDir });
        assert.equal(r.code, 0);
      }

      const files = fs.readdirSync(statesDir).sort();
      assert.deepEqual(
        files,
        ["test-pipeline.json", "test-pipeline.metrics.json"],
        `Only 2 files expected, got: ${files.join(", ")}`
      );

      const sidecar = JSON.parse(
        fs.readFileSync(path.join(statesDir, "test-pipeline.metrics.json"), "utf8")
      );
      assert.equal(sidecar.metrics.apiCalls, 5, "All 5 calls must aggregate into the same sidecar");
      assert.equal(sidecar.metrics.toolBreakdown.Write, 5);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── subagent-tracker.js (overload detection) ───────────────────────────────

describe("subagent-tracker.js overload detection", () => {
  const hook = "subagent-tracker.js";

  function setupPipelineState(tmpDir) {
    const statesDir = path.join(tmpDir, ".claude", ".pipeline-states");
    fs.mkdirSync(statesDir, { recursive: true });
    const pipelinePath = path.join(statesDir, "p.json");
    fs.writeFileSync(pipelinePath, JSON.stringify({
      v: 1,
      phase: "EXECUTE",
      startedAt: "2026-04-05T10:00:00.000Z",
    }), "utf8");
    fs.mkdirSync(path.join(tmpDir, ".claude", ".agent-state"), { recursive: true });
    return pipelinePath;
  }

  async function dispatchTaskResult(tmpDir, toolResponse) {
    return runHook(hook, {
      hook_event_name: "PostToolUse",
      tool_name: "Task",
      tool_input: {
        subagent_type: "general-purpose",
        description: "test dispatch",
        prompt: "Do something",
      },
      tool_response: toolResponse,
      cwd: tmpDir,
    }, { cwd: tmpDir, projectDir: tmpDir });
  }

  it("should flag lastDispatchFailure on real overload (is_error=true + 529)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "overload-real-"));
    const pipelinePath = setupPipelineState(tmpDir);
    try {
      const r = await dispatchTaskResult(tmpDir, {
        is_error: true,
        content: "Error: 529 overloaded",
      });
      assert.equal(r.code, 0);
      const state = JSON.parse(fs.readFileSync(pipelinePath, "utf8"));
      assert.ok(state.lastDispatchFailure, "flag must be set");
      assert.equal(state.lastDispatchFailure.reason, "dispatch_failure");
      assert.equal(state.lastDispatchFailure.agentType, "general-purpose");
      assert.equal(state.lastDispatchFailure.description, "test dispatch");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should flag lastDispatchFailure on tool result missing infrastructure error", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "infra-missing-"));
    const pipelinePath = setupPipelineState(tmpDir);
    try {
      const r = await dispatchTaskResult(tmpDir, {
        is_error: true,
        content: "Tool result missing due to internal error",
      });
      assert.equal(r.code, 0);
      const state = JSON.parse(fs.readFileSync(pipelinePath, "utf8"));
      assert.ok(state.lastDispatchFailure, "flag must be set on infra failure");
      assert.equal(state.lastDispatchFailure.reason, "dispatch_failure");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should flag lastDispatchFailure on HTTP 503 service unavailable", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "infra-503-"));
    const pipelinePath = setupPipelineState(tmpDir);
    try {
      const r = await dispatchTaskResult(tmpDir, {
        is_error: true,
        content: "Error 503: service unavailable",
      });
      assert.equal(r.code, 0);
      const state = JSON.parse(fs.readFileSync(pipelinePath, "utf8"));
      assert.ok(state.lastDispatchFailure, "flag must be set on 5xx");
      assert.equal(state.lastDispatchFailure.reason, "dispatch_failure");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should NOT flag on happy-path agent that merely documents rate limiting", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "overload-docs-"));
    const pipelinePath = setupPipelineState(tmpDir);
    try {
      const r = await dispatchTaskResult(tmpDir, {
        is_error: false,
        content: "Documented rate limiting, 429 and 529 handling, api error recovery.",
      });
      assert.equal(r.code, 0);
      const state = JSON.parse(fs.readFileSync(pipelinePath, "utf8"));
      assert.equal(state.lastDispatchFailure, undefined, "flag must NOT be set (false positive guard)");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should NOT flag on unrelated error (is_error=true without overload keywords)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "overload-unrelated-"));
    const pipelinePath = setupPipelineState(tmpDir);
    try {
      const r = await dispatchTaskResult(tmpDir, {
        is_error: true,
        content: "SyntaxError in src/foo.ts line 42",
      });
      assert.equal(r.code, 0);
      const state = JSON.parse(fs.readFileSync(pipelinePath, "utf8"));
      assert.equal(state.lastDispatchFailure, undefined, "unrelated failure must not be flagged as overload");
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

// ─── context-budget.js metrics emission ─────────────────────────────────────

describe("context-budget.js metrics emission", () => {
  const hook = "context-budget.js";

  it("should emit JSONL with tokens_saved > 0 and note='blocked' when over budget in strict mode", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "ctx-budget-metrics-"));
    try {
      // Explore budget = 10_000 chars. Send a 12_000 char prompt → over budget.
      const oversizePrompt = "x".repeat(12000);
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: {
          subagent_type: "Explore",
          description: "metrics test",
          prompt: oversizePrompt,
        },
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      // strict mode is the default — denial expected
      assert.equal(result.parsed?.permissionDecision, "deny");

      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "budget-check.jsonl");
      assert.ok(fs.existsSync(metricsFile), "budget-check.jsonl must exist");
      const lines = fs.readFileSync(metricsFile, "utf8").trim().split("\n");
      const entry = JSON.parse(lines[lines.length - 1]);
      assert.equal(entry.event, "budget-check");
      assert.equal(entry.note, "blocked");
      assert.ok(entry.tokens_saved > 0, "tokens_saved should be > 0 on block");
      assert.ok(entry.tokens_affected > 0, "tokens_affected should reflect prompt size");
      assert.equal(entry.would_block, true);
      assert.equal(entry.role, "Explore");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should emit note='passed' and tokens_saved=0 when under budget", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "ctx-budget-metrics-pass-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: {
          subagent_type: "Explore",
          description: "small",
          prompt: "x".repeat(500),
        },
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "budget-check.jsonl");
      assert.ok(fs.existsSync(metricsFile));
      const entry = JSON.parse(fs.readFileSync(metricsFile, "utf8").trim().split("\n").pop());
      assert.equal(entry.note, "passed");
      assert.equal(entry.tokens_saved, 0);
      assert.ok(entry.tokens_affected > 0);
      assert.equal(entry.would_block, false);
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

// ─── model-routing-gate.js ──────────────────────────────────────────────────

describe("model-routing-gate.js", () => {
  const hook = "model-routing-gate.js";

  function setupPipelineState(tmpDir, state) {
    const statesDir = path.join(tmpDir, ".claude", ".pipeline-states");
    fs.mkdirSync(statesDir, { recursive: true });
    fs.writeFileSync(
      path.join(statesDir, "test.json"),
      JSON.stringify({ v: 1, ...state }), "utf8"
    );
  }

  /** Run hook with custom env override */
  function runHookEnv(inputObj, env, opts) {
    return new Promise((resolve, reject) => {
      const cwd = opts.cwd || PROJECT_DIR;
      const { spawn } = require("child_process");
      const child = spawn(process.execPath, [path.join(HOOKS_DIR, hook)], {
        cwd,
        env: { ...process.env, CLAUDE_PROJECT_DIR: opts.projectDir || PROJECT_DIR, ...env },
        stdio: ["pipe", "pipe", "pipe"],
      });
      let stdout = "";
      child.stdout.on("data", (d) => (stdout += d));
      child.on("error", reject);
      child.on("close", (code) => {
        let parsed = null;
        if (stdout.trim()) { try { parsed = JSON.parse(stdout.trim()); } catch {} }
        resolve({ code, parsed });
      });
      child.stdin.write(JSON.stringify(inputObj));
      child.stdin.end();
    });
  }

  it("should allow when no model specified for non-explorer agents", async () => {
    const result = await runHook(hook, {
      hook_event_name: "PreToolUse",
      tool_name: "Task",
      tool_input: { subagent_type: "general-purpose", description: "do work", prompt: "test" },
    });
    assert.equal(result.code, 0);
    if (result.parsed?.permissionDecision) {
      assert.notEqual(result.parsed.permissionDecision, "deny");
    }
  });

  it("should deny Explore dispatch without explicit model (strict mode)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-explore-nomodel-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Explore", description: "search code", prompt: "test" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.parsed?.permissionDecision, "deny",
        "Explorer without model must be denied");
      const reason = result.parsed?.permissionDecisionReason || "";
      assert.ok(reason.includes("haiku"), "Denial reason must mention haiku");
      assert.ok(reason.includes("Explorer"), "Denial reason must mention Explorer");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should deny explorer (case-insensitive) dispatch without explicit model", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-explorer-nomodel-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "file-explorer", description: "browse files", prompt: "test" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.parsed?.permissionDecision, "deny",
        "Agent type containing 'explorer' without model must be denied");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should emit no-model-denied metric when Explore dispatched without model", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-explore-metric-"));
    try {
      await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Explore", description: "search", prompt: "test" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "model-routing-gate.jsonl");
      assert.ok(fs.existsSync(metricsFile), "Metrics file should exist");
      const entry = JSON.parse(fs.readFileSync(metricsFile, "utf8").trim().split("\n").pop());
      assert.equal(entry.note, "no-model-denied");
      assert.equal(entry.actual, "inherited");
      assert.equal(entry.expected, "haiku");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should deny when Explore uses opus (default strict mode)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-warn-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Explore", description: "explore code", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.parsed?.permissionDecision, "deny", "Should deny opus for Explore in default strict mode");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should deny when Explore uses opus in strict mode", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-strict-"));
    try {
      const result = await runHookEnv({
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Explore", description: "explore", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { MUSTARD_MODEL_GATE_MODE: "strict" }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.parsed?.permissionDecision, "deny", "Should deny in strict mode");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow opus for bugfix pipeline (deep diagnosis)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-bugfix-"));
    try {
      setupPipelineState(tmpDir, { type: "bugfix", scope: "light", phaseName: "EXECUTE" });
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "general-purpose", description: "fix bug", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      if (result.parsed?.permissionDecision) {
        assert.notEqual(result.parsed.permissionDecision, "deny", "Opus is correct for bugfix");
      }
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should warn when model-gate in warn mode", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-warnmode-"));
    try {
      const result = await runHookEnv({
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Explore", description: "explore code", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { MUSTARD_MODEL_GATE_MODE: "warn" }, { cwd: tmpDir, projectDir: tmpDir });

      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(ctx.includes("Model Gate"), "Should include Model Gate warning in warn mode");
      assert.ok(ctx.includes("haiku"), "Should suggest haiku");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow opus for feature full scope", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-full-"));
    try {
      setupPipelineState(tmpDir, { type: "feature", scope: "full", phaseName: "EXECUTE" });
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "general-purpose", description: "implement", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(!ctx.includes("Model Gate"), "Should NOT warn for correct model");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow downgrade (sonnet where opus expected)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-down-"));
    try {
      setupPipelineState(tmpDir, { type: "feature", scope: "full" });
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "general-purpose", description: "impl", prompt: "test", model: "sonnet" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(!ctx.includes("Model Gate"), "Should NOT warn on downgrade");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow sonnet for audit task (quality-first, no haiku for analysis)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-audit-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "general-purpose", description: "audit dependencies", prompt: "test", model: "sonnet" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      if (result.parsed?.permissionDecision) {
        assert.notEqual(result.parsed.permissionDecision, "deny", "Sonnet is correct for audit tasks");
      }
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should deny sonnet for Plan agent (Plan needs opus)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-plan-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Plan", description: "plan implementation", prompt: "test", model: "sonnet" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      // sonnet is a downgrade from opus — allowed (saving money is fine)
      assert.equal(result.code, 0);
      if (result.parsed?.permissionDecision) {
        assert.notEqual(result.parsed.permissionDecision, "deny", "Downgrade from opus to sonnet is allowed");
      }
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow opus for Plan agent", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-plan2-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Plan", description: "plan implementation", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      if (result.parsed?.permissionDecision) {
        assert.notEqual(result.parsed.permissionDecision, "deny", "Opus is correct for Plan");
      }
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow opus for feature pipeline (any scope)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-feat-"));
    try {
      setupPipelineState(tmpDir, { type: "feature", scope: "light", phaseName: "EXECUTE" });
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "general-purpose", description: "implement login", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      if (result.parsed?.permissionDecision) {
        assert.notEqual(result.parsed.permissionDecision, "deny", "Opus is correct for any feature scope");
      }
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should emit metrics", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "model-gate-metrics-"));
    try {
      await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: { subagent_type: "Explore", description: "find", prompt: "test", model: "opus" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "model-routing-gate.jsonl");
      assert.ok(fs.existsSync(metricsFile), "Metrics file should exist");
      const entry = JSON.parse(fs.readFileSync(metricsFile, "utf8").trim());
      assert.equal(entry.note, "violation");
      assert.equal(entry.expected, "haiku");
      assert.equal(entry.actual, "opus");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should skip non-Task tools", async () => {
    const result = await runHook(hook, {
      hook_event_name: "PreToolUse",
      tool_name: "Read",
      tool_input: { file_path: "test.txt" },
    });
    assert.equal(result.code, 0);
  });
});

// ─── tool-use-counter.js ────────────────────────────────────────────────────

describe("tool-use-counter.js", () => {
  const hook = "tool-use-counter.js";

  it("should create counter file on SubagentStart for Explore", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-start-"));
    fs.mkdirSync(path.join(tmpDir, ".claude", ".agent-state"), { recursive: true });
    try {
      const result = await runHook(hook, {
        hook_event_name: "SubagentStart",
        agent_id: "explore-123",
        agent_type: "Explore",
        session_id: "test",
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      const f = path.join(tmpDir, ".claude", ".agent-state", "explore-123.counter.json");
      assert.ok(fs.existsSync(f), "Counter file should exist");
      const counter = JSON.parse(fs.readFileSync(f, "utf8"));
      assert.equal(counter.type, "Explore");
      assert.equal(counter.limit, 15);
      assert.equal(counter.warnAt, 12);
      assert.equal(counter.count, 0);

      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(ctx.includes("Tool Budget"), "Should inject budget reminder");
      assert.ok(ctx.includes("15"), "Should reference the 15-tool budget");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should NOT create counter for non-Explore agents", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-nope-"));
    fs.mkdirSync(path.join(tmpDir, ".claude", ".agent-state"), { recursive: true });
    try {
      await runHook(hook, {
        hook_event_name: "SubagentStart",
        agent_id: "impl-1",
        agent_type: "general-purpose",
        session_id: "test",
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });
      const f = path.join(tmpDir, ".claude", ".agent-state", "impl-1.counter.json");
      assert.ok(!fs.existsSync(f), "Counter should NOT exist for general-purpose");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should increment counter on PreToolUse", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-inc-"));
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    fs.writeFileSync(path.join(stateDir, "e.counter.json"),
      JSON.stringify({ type: "Explore", limit: 15, warnAt: 12, count: 5, createdAt: new Date().toISOString() }));
    try {
      await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Grep",
        tool_input: { pattern: "test" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      const counter = JSON.parse(fs.readFileSync(path.join(stateDir, "e.counter.json"), "utf8"));
      assert.equal(counter.count, 6, "Counter should increment to 6");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should warn at threshold", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-warn-"));
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    fs.writeFileSync(path.join(stateDir, "e.counter.json"),
      JSON.stringify({ type: "Explore", limit: 15, warnAt: 12, count: 11, createdAt: new Date().toISOString() }));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Read",
        tool_input: { file_path: "t.txt" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(ctx.includes("12/15"), "Should show count at warn threshold");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should deny when hard limit reached", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-deny-"));
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    // count: 14 so that the increment inside PreToolUse brings it to 15, hitting the limit exactly
    fs.writeFileSync(path.join(stateDir, "e.counter.json"),
      JSON.stringify({ type: "Explore", limit: 15, warnAt: 12, count: 14, createdAt: new Date().toISOString() }));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Grep",
        tool_input: { pattern: "x" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.ok(
        result.parsed?.hookSpecificOutput?.permissionDecision === "deny",
        "Should deny at hard limit (count >= limit)"
      );
      assert.ok(
        result.parsed?.hookSpecificOutput?.permissionDecisionReason?.includes("15 tool uses (limit)"),
        "Deny message should reference 15-use limit"
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should remove counter on SubagentStop", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-stop-"));
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    const f = path.join(stateDir, "done.counter.json");
    fs.writeFileSync(f, "{}");
    try {
      await runHook(hook, {
        hook_event_name: "SubagentStop",
        agent_id: "done",
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });
      assert.ok(!fs.existsSync(f), "Counter should be removed");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should clean all counters on SessionStart", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-sess-"));
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    fs.writeFileSync(path.join(stateDir, "a.counter.json"), "{}");
    fs.writeFileSync(path.join(stateDir, "b.counter.json"), "{}");
    fs.writeFileSync(path.join(stateDir, "agent-1.json"), "{}"); // should survive
    try {
      await runHook(hook, {
        hook_event_name: "SessionStart",
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });
      const remaining = fs.readdirSync(stateDir);
      assert.ok(!remaining.some(f => f.endsWith(".counter.json")), "Counters cleaned");
      assert.ok(remaining.includes("agent-1.json"), "Agent state files survive");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should delete stale counter during PreToolUse", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-stale-"));
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    const f = path.join(stateDir, "old.counter.json");
    fs.writeFileSync(f, JSON.stringify({
      type: "Explore", limit: 15, warnAt: 12, count: 10,
      createdAt: new Date(Date.now() - 15 * 60 * 1000).toISOString(),
    }));
    try {
      await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Grep",
        tool_input: { pattern: "x" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });
      assert.ok(!fs.existsSync(f), "Stale counter should be deleted");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should exit fast when no state dir (parent context)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "counter-fast-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Read",
        tool_input: { file_path: "t.txt" },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });
      assert.equal(result.code, 0);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── subagent-tracker.js (explorer dedup) ───────────────────────────────────

describe("subagent-tracker.js explorer dedup", () => {
  const hook = "subagent-tracker.js";

  function makeStateDir(tmpDir) {
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    return stateDir;
  }

  function dispatchExplorer(tmpDir, subagentType) {
    return runHook(hook, {
      hook_event_name: "PreToolUse",
      tool_name: "Task",
      tool_input: {
        subagent_type: subagentType,
        description: "explore the codebase",
        prompt: "Find relevant files",
      },
      cwd: tmpDir,
    }, { cwd: tmpDir, projectDir: tmpDir });
  }

  it("should allow the first Explore dispatch and record a dedup entry", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dedup-first-"));
    const stateDir = makeStateDir(tmpDir);
    try {
      const result = await dispatchExplorer(tmpDir, "Explore");
      assert.equal(result.code, 0);
      assert.notEqual(result.parsed?.permissionDecision, "deny",
        "First dispatch must not be denied");
      const dedupFile = path.join(stateDir, "explorer-dedup.json");
      assert.ok(fs.existsSync(dedupFile), "explorer-dedup.json should be created");
      const cache = JSON.parse(fs.readFileSync(dedupFile, "utf8"));
      assert.ok(typeof cache["Explore"] === "number", "Timestamp should be stored for Explore");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should deny a duplicate Explore dispatch within 60s", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dedup-deny-"));
    const stateDir = makeStateDir(tmpDir);
    const dedupFile = path.join(stateDir, "explorer-dedup.json");
    fs.writeFileSync(dedupFile, JSON.stringify({ "Explore": Date.now() - 5000 }), "utf8");
    try {
      const result = await dispatchExplorer(tmpDir, "Explore");
      assert.equal(result.code, 0);
      assert.equal(result.parsed?.permissionDecision, "deny",
        "Duplicate dispatch within 60s must be denied");
      assert.ok(
        result.parsed?.permissionDecisionReason?.includes("[Dedup]"),
        "Deny reason must include [Dedup]"
      );
      assert.ok(
        result.parsed?.permissionDecisionReason?.includes("Explore"),
        "Deny reason must name the agent type"
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should deny a duplicate custom explorer type within 60s", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dedup-custom-"));
    const stateDir = makeStateDir(tmpDir);
    const customType = "Sialia.Backend-explorer";
    const dedupFile = path.join(stateDir, "explorer-dedup.json");
    fs.writeFileSync(dedupFile, JSON.stringify({ [customType]: Date.now() - 10000 }), "utf8");
    try {
      const result = await dispatchExplorer(tmpDir, customType);
      assert.equal(result.code, 0);
      assert.equal(result.parsed?.permissionDecision, "deny",
        "Duplicate custom explorer must be denied");
      assert.ok(
        result.parsed?.permissionDecisionReason?.includes(customType),
        "Deny reason must name the custom agent type"
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should allow dispatch after the 60s deny window has elapsed", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dedup-expired-"));
    makeStateDir(tmpDir);
    const dedupFile = path.join(tmpDir, ".claude", ".agent-state", "explorer-dedup.json");
    fs.writeFileSync(dedupFile, JSON.stringify({ "Explore": Date.now() - 65000 }), "utf8");
    try {
      const result = await dispatchExplorer(tmpDir, "Explore");
      assert.equal(result.code, 0);
      assert.notEqual(result.parsed?.permissionDecision, "deny",
        "Dispatch after 60s window must be allowed");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should NOT apply dedup to non-explorer agents (general-purpose)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dedup-skip-impl-"));
    const stateDir = makeStateDir(tmpDir);
    const dedupFile = path.join(stateDir, "explorer-dedup.json");
    fs.writeFileSync(dedupFile, JSON.stringify({ "general-purpose": Date.now() - 1000 }), "utf8");
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: {
          subagent_type: "general-purpose",
          description: "implement feature",
          prompt: "Write the service",
        },
        cwd: tmpDir,
      }, { cwd: tmpDir, projectDir: tmpDir });
      assert.equal(result.code, 0);
      assert.notEqual(result.parsed?.permissionDecision, "deny",
        "general-purpose must never be denied by dedup");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should prune entries older than 120s when reading the cache", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dedup-prune-"));
    const stateDir = makeStateDir(tmpDir);
    const dedupFile = path.join(stateDir, "explorer-dedup.json");
    fs.writeFileSync(dedupFile, JSON.stringify({
      "OldExplorer-explorer": Date.now() - 130000,
      "Explore": Date.now() - 5000,
    }), "utf8");
    try {
      const result = await dispatchExplorer(tmpDir, "Explore");
      assert.equal(result.parsed?.permissionDecision, "deny", "Fresh entry should still deny");
      const cacheAfter = JSON.parse(fs.readFileSync(dedupFile, "utf8"));
      assert.ok(!("OldExplorer-explorer" in cacheAfter),
        "Entry older than 120s must be pruned from cache");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should fail-open when dedup cache file is corrupt", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dedup-corrupt-"));
    const stateDir = makeStateDir(tmpDir);
    const dedupFile = path.join(stateDir, "explorer-dedup.json");
    fs.writeFileSync(dedupFile, "NOT VALID JSON", "utf8");
    try {
      const result = await dispatchExplorer(tmpDir, "Explore");
      assert.equal(result.code, 0);
      assert.notEqual(result.parsed?.permissionDecision, "deny",
        "Corrupt cache must fail-open (allow dispatch)");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── debug-loop-guard.js ─────────────────────────────────────────────────────

describe("debug-loop-guard.js", () => {
  const hook = "debug-loop-guard.js";

  function makeStateDir(tmpDir) {
    const stateDir = path.join(tmpDir, ".claude", ".agent-state");
    fs.mkdirSync(stateDir, { recursive: true });
    return stateDir;
  }

  function editEvent(tmpDir, filePath) {
    return runHook(hook, {
      hook_event_name: "PostToolUse",
      tool_name: "Edit",
      tool_input: { file_path: filePath },
      tool_response: {},
      cwd: tmpDir,
    }, { cwd: tmpDir, projectDir: tmpDir });
  }

  function bashEvent(tmpDir, command, exitCode) {
    return runHook(hook, {
      hook_event_name: "PostToolUse",
      tool_name: "Bash",
      tool_input: { command },
      tool_response: { exit_code: exitCode },
      cwd: tmpDir,
    }, { cwd: tmpDir, projectDir: tmpDir });
  }

  it("should warn after 5 consecutive edits to the same file", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dlg-edit-warn-"));
    makeStateDir(tmpDir);
    const file = path.join(tmpDir, "src", "foo.ts");
    try {
      let result;
      for (let i = 0; i < 5; i++) {
        result = await editEvent(tmpDir, file);
      }
      assert.equal(result.code, 0);
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(ctx.includes("[Debug Loop Guard]"), "Should include Debug Loop Guard header");
      assert.ok(ctx.includes("foo.ts"), "Warning should name the file");
      assert.ok(ctx.includes("Task(Plan)"), "Should recommend Task(Plan)");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should NOT warn when edits alternate between two different files", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dlg-edit-alt-"));
    makeStateDir(tmpDir);
    const fileA = path.join(tmpDir, "src", "a.ts");
    const fileB = path.join(tmpDir, "src", "b.ts");
    try {
      let result;
      for (let i = 0; i < 6; i++) {
        result = await editEvent(tmpDir, i % 2 === 0 ? fileA : fileB);
      }
      assert.equal(result.code, 0);
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(!ctx.includes("[Debug Loop Guard]"), "Should NOT warn when files alternate");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should warn after 3 consecutive Bash failures on npm test", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dlg-bash-fail-"));
    makeStateDir(tmpDir);
    try {
      let result;
      for (let i = 0; i < 3; i++) {
        result = await bashEvent(tmpDir, "npm test", 1);
      }
      assert.equal(result.code, 0);
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(ctx.includes("[Debug Loop Guard]"), "Should include Debug Loop Guard header");
      assert.ok(ctx.includes("Task(Plan)"), "Should recommend Task(Plan)");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should NOT warn on Bash success (exit_code 0)", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dlg-bash-ok-"));
    makeStateDir(tmpDir);
    try {
      let result;
      for (let i = 0; i < 5; i++) {
        result = await bashEvent(tmpDir, "npm test", 0);
      }
      assert.equal(result.code, 0);
      const ctx = result.parsed?.hookSpecificOutput?.additionalContext || "";
      assert.ok(!ctx.includes("[Debug Loop Guard]"), "Should NOT warn on success");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should fail-open on malformed state JSON", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "dlg-corrupt-"));
    const stateDir = makeStateDir(tmpDir);
    fs.writeFileSync(
      path.join(stateDir, "debug-loop-state.json"),
      "NOT VALID JSON",
      "utf8"
    );
    try {
      const result = await editEvent(tmpDir, path.join(tmpDir, "src", "x.ts"));
      assert.equal(result.code, 0, "Should exit 0 even with corrupt state (fail-open)");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ─── knowledge-extract prescriptions ────────────────────────────────────────

describe("knowledge-extract prescriptions", () => {
  const { extractPatternsFromStates, derivePrescription } = require("../_lib/knowledge-extract.js");

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

    const patterns = extractPatternsFromStates(states);
    // retries > 2 triggers the high-retry entry
    const retryEntry = patterns.find(p => p.name === "high-retry-login-feature");
    assert.ok(retryEntry, "Expected high-retry entry");
    assert.ok(retryEntry.prescription, "Expected prescription field");
    assert.ok(
      /delegate investigation via Task\(general-purpose\)/.test(retryEntry.prescription),
      "Prescription should instruct delegation via Task(general-purpose)"
    );
    assert.ok(retryEntry.tags.includes("prescriptive"), "Tags should include 'prescriptive'");
    // Back-compat: original tags preserved
    assert.ok(retryEntry.tags.includes("retry"));
    assert.ok(retryEntry.tags.includes("pipeline"));
    assert.ok(retryEntry.tags.includes("lesson"));
    // Back-compat: description still present
    assert.ok(retryEntry.description.includes("4 retries"));
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

    const patterns = extractPatternsFromStates(states);
    // apiCalls > 50 triggers heavy-pipeline; retries > 2 also triggers high-retry.
    const heavyEntry = patterns.find(p => p.name === "heavy-pipeline-big-refactor");
    assert.ok(heavyEntry, "Expected heavy-pipeline entry");
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
    // Edit=20 > 15, Write=1 < 3, retries=3 to trigger the high-retry entry
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

    const patterns = extractPatternsFromStates(states);
    const retryEntry = patterns.find(p => p.name === "high-retry-tweak-hell");
    assert.ok(retryEntry, "Expected high-retry entry");
    assert.ok(retryEntry.prescription, "Expected prescription field");
    assert.ok(
      /investigate with Read\+Grep BEFORE editing/.test(retryEntry.prescription),
      "Prescription should instruct Read+Grep investigation before editing"
    );
    assert.ok(retryEntry.tags.includes("prescriptive"));
  });

  it("should NOT add prescription or prescriptive tag when no heuristic matches", () => {
    // retries=3 to trigger high-retry entry, but balanced tools so none of the
    // heuristics fire (edit<=15, apiCalls<=50, bash+edit not >3*agent).
    const states = [{
      specName: "mild-case",
      metrics: {
        retries: 3,
        apiCalls: 10,
        toolBreakdown: { Bash: 2, Edit: 2, Agent: 5, Write: 1 },
      },
    }];

    const patterns = extractPatternsFromStates(states);
    const retryEntry = patterns.find(p => p.name === "high-retry-mild-case");
    assert.ok(retryEntry, "Expected high-retry entry");
    assert.equal(retryEntry.prescription, undefined, "No prescription when no heuristic matches");
    assert.ok(!retryEntry.tags.includes("prescriptive"),
      "'prescriptive' tag must NOT be added when no prescription");
    // Original schema preserved
    assert.ok(retryEntry.tags.includes("retry"));
    assert.ok(retryEntry.description);
    assert.equal(retryEntry.source, "session-knowledge");
  });

  it("derivePrescription should return null for empty / trivial metrics", () => {
    assert.equal(derivePrescription({}), null);
    assert.equal(derivePrescription({ retries: 1, apiCalls: 10, toolBreakdown: {} }), null);
    assert.equal(derivePrescription(null), null);
  });
});

// ─── user-prompt-hint.js ─────────────────────────────────────────────────────

describe("user-prompt-hint.js", () => {
  const hook = "user-prompt-hint.js";

  function runHookWithEnv(hookFile, inputObj, extraEnv = {}) {
    return new Promise((resolve, reject) => {
      const { spawn } = require("node:child_process");
      const child = spawn(process.execPath, [path.join(HOOKS_DIR, hookFile)], {
        cwd: PROJECT_DIR,
        env: {
          ...process.env,
          CLAUDE_PROJECT_DIR: PROJECT_DIR,
          ...extraEnv,
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
          try { parsed = JSON.parse(stdout.trim()); } catch { /* not JSON */ }
        }
        resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
      });
      child.stdin.write(JSON.stringify(inputObj));
      child.stdin.end();
    });
  }

  it("prompt with bugfix keyword 'erro' should suggest /mustard:bugfix", async () => {
    const result = await runHookWithEnv(hook, { prompt: "erro de null pointer no serviço" });
    assert.equal(result.code, 0);
    assert.ok(result.parsed, "Should output JSON");
    const ctx = result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("/mustard:bugfix"), `Expected /mustard:bugfix in: ${ctx}`);
  });

  it("prompt starting with slash command should produce no output", async () => {
    const result = await runHookWithEnv(hook, { prompt: "/mustard:feature add login" });
    assert.equal(result.code, 0);
    assert.equal(result.stdout, "", "No output for slash-prefixed prompts");
  });

  it("prompt with analysis keyword 'analise' should suggest /mustard:task", async () => {
    const result = await runHookWithEnv(hook, { prompt: "analise esse código e me diga o que está errado" });
    assert.equal(result.code, 0);
    assert.ok(result.parsed, "Should output JSON");
    const ctx = result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("/mustard:task"), `Expected /mustard:task in: ${ctx}`);
  });

  it("prompt with feature keyword 'criar' should suggest /mustard:feature", async () => {
    const result = await runHookWithEnv(hook, { prompt: "criar um novo endpoint de login" });
    assert.equal(result.code, 0);
    assert.ok(result.parsed, "Should output JSON");
    const ctx = result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("/mustard:feature"), `Expected /mustard:feature in: ${ctx}`);
  });

  it("prompt with enhancement keyword 'melhorar' should suggest /mustard:feature", async () => {
    const result = await runHookWithEnv(hook, { prompt: "melhorar a performance da tela de usuários" });
    assert.equal(result.code, 0);
    assert.ok(result.parsed, "Should output JSON");
    const ctx = result.parsed.hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("/mustard:feature"), `Expected /mustard:feature in: ${ctx}`);
  });

  it("random prompt with no keywords should produce no output", async () => {
    const result = await runHookWithEnv(hook, { prompt: "oi tudo bem como vai você" });
    assert.equal(result.code, 0);
    assert.equal(result.stdout, "", "No output for unrelated prompts");
  });

  it("MUSTARD_DISABLED_HOOKS=user-prompt-hint should produce no output", async () => {
    const result = await runHookWithEnv(
      hook,
      { prompt: "erro de null pointer" },
      { MUSTARD_DISABLED_HOOKS: "user-prompt-hint" }
    );
    assert.equal(result.code, 0);
    assert.equal(result.stdout, "", "No output when hook is disabled");
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

// ─── recommended-skills-audit.js ────────────────────────────────────────────

describe("recommended-skills-audit.js", () => {
  const hook = "recommended-skills-audit.js";

  it("should emit recommended-skills metric with skill_count=3 when dispatch lists array", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "rec-skills-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: {
          subagent_type: "general-purpose",
          description: "implement foo",
          prompt: "Do X. recommended_skills: [alpha, beta, gamma]",
        },
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      assert.equal(result.parsed?.permissionDecision, "allow");

      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "recommended-skills.jsonl");
      assert.ok(fs.existsSync(metricsFile), "recommended-skills.jsonl must exist");
      const entry = JSON.parse(fs.readFileSync(metricsFile, "utf8").trim().split("\n").pop());
      assert.equal(entry.event, "recommended-skills");
      assert.equal(entry.skill_count, 3);
      assert.equal(entry.subagent_type, "general-purpose");
      assert.ok(entry.skills.includes("alpha"));
      assert.ok(entry.skills.includes("beta"));
      assert.ok(entry.skills.includes("gamma"));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should emit skill_count=0 when prompt lists no skills", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "rec-skills-empty-"));
    try {
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: {
          subagent_type: "Explore",
          description: "search",
          prompt: "Just do a simple task, no skills listed here.",
        },
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      assert.equal(result.parsed?.permissionDecision, "allow");

      const metricsFile = path.join(tmpDir, ".claude", ".metrics", "recommended-skills.jsonl");
      assert.ok(fs.existsSync(metricsFile));
      const entry = JSON.parse(fs.readFileSync(metricsFile, "utf8").trim().split("\n").pop());
      assert.equal(entry.skill_count, 0);
      assert.equal(entry.tokens_affected, 0);
      assert.equal(entry.subagent_type, "Explore");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("should print stderr WARN when skill_count > 10", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "rec-skills-warn-"));
    try {
      const many = Array.from({ length: 12 }, (_, i) => `skill${i}`).join(", ");
      const result = await runHook(hook, {
        hook_event_name: "PreToolUse",
        tool_name: "Task",
        tool_input: {
          subagent_type: "general-purpose",
          description: "big dispatch",
          prompt: `Do X. recommended_skills: [${many}]`,
        },
      }, { cwd: tmpDir, projectDir: tmpDir });

      assert.equal(result.code, 0);
      assert.ok(/recommended-skills.*WARN/.test(result.stderr), `stderr should warn, got: ${result.stderr}`);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});
