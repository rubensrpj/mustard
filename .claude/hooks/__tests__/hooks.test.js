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
      assert.equal(state.lastDispatchFailure.reason, "api_overload");
      assert.equal(state.lastDispatchFailure.agentType, "general-purpose");
      assert.equal(state.lastDispatchFailure.description, "test dispatch");
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
