#!/usr/bin/env node

/**
 * sync-detect.js
 *
 * Detects subprojects automatically by reading git submodule status
 * (or scanning for folders with CLAUDE.md as fallback).
 *
 * For each subproject:
 *   - Reads CLAUDE.md to detect technology/agent type
 *   - Scans .claude/commands/*.md for available commands
 *
 * Also reads .claude/context/ subdirectories to build the agents list.
 *
 * Outputs JSON to stdout.
 */

const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

// Root of the monorepo (parent of .claude/scripts/)
const ROOT = path.resolve(__dirname, "..", "..");

// ---------------------------------------------------------------------------
// Agent type detection patterns (applied to CLAUDE.md content)
// ---------------------------------------------------------------------------

const AGENT_PATTERNS = [
  {
    agent: "backend",
    patterns: [/\.NET/i, /dotnet/i, /FastEndpoints/i, /\bC#\b/, /ASP\.NET/i],
  },
  {
    agent: "frontend",
    patterns: [/\bReact\b/, /\bNext\.?js\b/i, /\bVue\b/i, /\bAngular\b/i, /\bFrontend\b/i],
  },
  {
    agent: "database",
    patterns: [/\bDrizzle\b/i, /\bPrisma\b/i, /\bDatabase\b/i, /\bPostgreSQL\b/i, /\bMySQL\b/i],
  },
];

/**
 * Detect the agent type from a CLAUDE.md content string.
 * Returns the first matching agent or "general" as fallback.
 */
function detectAgent(content) {
  for (const { agent, patterns } of AGENT_PATTERNS) {
    for (const re of patterns) {
      if (re.test(content)) {
        return agent;
      }
    }
  }
  return "general";
}

// ---------------------------------------------------------------------------
// Subproject discovery
// ---------------------------------------------------------------------------

/**
 * Try to get subproject paths from `git submodule status`.
 * Returns an array of relative paths (e.g. ["Competi.Backend", ...]) or null on failure.
 */
function getSubmodulePaths() {
  try {
    const output = execSync("git submodule status", {
      cwd: ROOT,
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    });

    const paths = [];
    for (const line of output.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      // Format: " <hash> <path> (<branch>)" or "+<hash> <path> (<branch>)"
      const parts = trimmed.replace(/^[+ -]/, "").split(/\s+/);
      if (parts.length >= 2) {
        paths.push(parts[1]);
      }
    }
    return paths.length > 0 ? paths : null;
  } catch {
    return null;
  }
}

/**
 * Fallback: scan root directory for folders that contain a CLAUDE.md file.
 */
function scanForSubprojects() {
  const paths = [];
  try {
    const entries = fs.readdirSync(ROOT, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      if (entry.name.startsWith(".")) continue;
      if (entry.name === "node_modules") continue;

      const claudePath = path.join(ROOT, entry.name, "CLAUDE.md");
      if (fs.existsSync(claudePath)) {
        paths.push(entry.name);
      }
    }
  } catch {
    // ignore
  }
  return paths;
}

// ---------------------------------------------------------------------------
// Commands discovery
// ---------------------------------------------------------------------------

/**
 * List .md files inside <subprojectDir>/.claude/commands/
 * Returns array of filenames (e.g. ["module.md", "create-tests.md"]).
 */
function getCommands(subprojectAbsPath) {
  const commandsDir = path.join(subprojectAbsPath, ".claude", "commands");
  try {
    if (!fs.existsSync(commandsDir)) return [];
    return fs
      .readdirSync(commandsDir)
      .filter((f) => f.endsWith(".md"))
      .sort();
  } catch {
    return [];
  }
}

// ---------------------------------------------------------------------------
// Agents discovery (from .claude/context/ subdirectories)
// ---------------------------------------------------------------------------

/**
 * Reads subdirectories of .claude/context/. Each subdirectory (except "shared")
 * is treated as an agent. Always includes "orchestrator".
 */
function getAgents() {
  const contextDir = path.join(ROOT, ".claude", "context");
  const agents = new Set(["orchestrator"]);

  try {
    if (fs.existsSync(contextDir)) {
      const entries = fs.readdirSync(contextDir, { withFileTypes: true });
      for (const entry of entries) {
        if (entry.isDirectory() && entry.name !== "shared") {
          agents.add(entry.name);
        }
      }
    }
  } catch {
    // ignore
  }

  return Array.from(agents).sort();
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  // 1. Discover subproject paths
  let subprojectPaths = getSubmodulePaths();
  if (!subprojectPaths) {
    subprojectPaths = scanForSubprojects();
  }

  // 2. Filter to only those with a CLAUDE.md, then build subproject entries
  const subprojects = [];

  for (const relPath of subprojectPaths) {
    const absPath = path.join(ROOT, relPath);
    const claudeFile = path.join(absPath, "CLAUDE.md");

    if (!fs.existsSync(claudeFile)) continue;

    let content = "";
    try {
      content = fs.readFileSync(claudeFile, "utf-8");
    } catch {
      continue;
    }

    const name = path.basename(relPath);
    const agent = detectAgent(content);
    const commands = getCommands(absPath);

    subprojects.push({
      name,
      path: relPath.split(path.sep).join("/"), // normalize to forward slashes in output
      agent,
      commands,
    });
  }

  // 3. Discover agents
  const agents = getAgents();

  // 4. Output
  const result = {
    subprojects,
    agents,
    contextDir: ".claude/context",
    promptsDir: ".claude/prompts",
  };

  process.stdout.write(JSON.stringify(result, null, 2) + "\n");
}

main();
