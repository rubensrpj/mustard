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
