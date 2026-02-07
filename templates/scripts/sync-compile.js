#!/usr/bin/env node

/**
 * sync-compile.js
 *
 * Compiles context files for each agent detected by sync-detect.js.
 *
 * Steps:
 *   1. Runs sync-detect.js to discover subprojects and agents.
 *   2. For each subproject with commands, copies command .md files
 *      to .claude/context/{agent}/cmd-{filename} (only if source is newer).
 *   3. For each agent, concatenates {agent}/*.md into
 *      .claude/context/{agent}.context.md (only if content hash changed).
 *   4. Prints JSON result: { copied, compiled, skipped }
 *
 * Uses only native Node.js modules. Works on Windows.
 */

const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

// Root of the monorepo (parent of .claude/scripts/)
const ROOT = path.resolve(__dirname, "..", "..");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Run sync-detect.js and return parsed JSON output.
 */
function runSyncDetect() {
  const detectPath = path.join(__dirname, "sync-detect.js");
  // Use require to get the output - but sync-detect.js writes to stdout,
  // so we need to capture it via child_process instead.
  const { execSync } = require("child_process");
  const output = execSync(`node "${detectPath}"`, {
    cwd: ROOT,
    encoding: "utf-8",
    stdio: ["pipe", "pipe", "pipe"],
  });
  return JSON.parse(output);
}

/**
 * Ensure a directory exists (recursive).
 */
function ensureDir(dirPath) {
  if (!fs.existsSync(dirPath)) {
    fs.mkdirSync(dirPath, { recursive: true });
  }
}

/**
 * Get mtime of a file, or 0 if it does not exist.
 */
function getMtime(filePath) {
  try {
    return fs.statSync(filePath).mtimeMs;
  } catch {
    return 0;
  }
}

/**
 * Read all .md files in a directory (excluding README.md).
 * Returns array of { name, content } sorted by name.
 */
function readMdFiles(dirPath) {
  const results = [];
  try {
    if (!fs.existsSync(dirPath)) return results;
    const files = fs.readdirSync(dirPath).filter(
      (f) => f.endsWith(".md") && f !== "README.md" && !f.endsWith(".context.md")
    );
    files.sort((a, b) => {
      const aIsCore = a.endsWith('.core.md');
      const bIsCore = b.endsWith('.core.md');
      if (aIsCore && !bIsCore) return -1;
      if (!aIsCore && bIsCore) return 1;
      return a.localeCompare(b);
    });
    for (const f of files) {
      const content = fs.readFileSync(path.join(dirPath, f), "utf-8");
      results.push({ name: f, content });
    }
  } catch {
    // ignore read errors
  }
  return results;
}

/**
 * Compact content: collapse excessive blank lines (max 1 between sections),
 * remove duplicate H1 headers (keep only the first).
 */
function compactContent(text) {
  // Collapse 3+ consecutive newlines to 2 (one blank line)
  let result = text.replace(/\n{3,}/g, "\n\n");
  // Remove duplicate H1 headers (keep only the first one)
  let foundH1 = false;
  result = result.split("\n").filter((line) => {
    if (/^# [^#]/.test(line)) {
      if (foundH1) return false;
      foundH1 = true;
    }
    return true;
  }).join("\n");
  return result;
}

/**
 * Compute SHA256 hash of a string.
 */
function sha256(str) {
  return crypto.createHash("sha256").update(str, "utf-8").digest("hex");
}

/**
 * Extract the compiled-from-hash from an existing .context.md file.
 * Returns the hash string or null if not found.
 */
function extractExistingHash(filePath) {
  try {
    if (!fs.existsSync(filePath)) return null;
    // Read only the first line to extract the hash comment
    const content = fs.readFileSync(filePath, "utf-8");
    const match = content.match(/^<!--\s*compiled-from-hash:\s*(\S+)\s*-->/);
    return match ? match[1] : null;
  } catch {
    return null;
  }
}


// ---------------------------------------------------------------------------
// Phase 1: Copy commands to context directories
// ---------------------------------------------------------------------------

/**
 * Copy subproject command files to the agent's context directory.
 * Returns number of files copied.
 */
function copyCommands(subprojects) {
  let copied = 0;

  for (const sub of subprojects) {
    if (!sub.commands || sub.commands.length === 0) continue;

    const agent = sub.agent;
    const destDir = path.join(ROOT, ".claude", "context", agent);
    ensureDir(destDir);

    const srcDir = path.join(ROOT, sub.path, ".claude", "commands");

    for (const cmdFile of sub.commands) {
      const srcPath = path.join(srcDir, cmdFile);
      const destFile = "cmd-" + cmdFile;
      const destPath = path.join(destDir, destFile);

      const srcMtime = getMtime(srcPath);
      const destMtime = getMtime(destPath);

      if (srcMtime > destMtime) {
        try {
          fs.copyFileSync(srcPath, destPath);
          copied++;
        } catch {
          // skip files we cannot copy
        }
      }
    }
  }

  return copied;
}

// ---------------------------------------------------------------------------
// Phase 2: Compile context files for each agent
// ---------------------------------------------------------------------------

/**
 * Compile .context.md for each agent.
 * Returns { compiled, skipped }.
 */
function compileContexts(agents) {
  let compiled = 0;
  let skipped = 0;

  const promptsDir = path.join(ROOT, ".claude", "prompts");
  ensureDir(promptsDir);

  for (const agent of agents) {
    const agentDir = path.join(ROOT, ".claude", "context", agent);

    // Read agent-specific context files (if directory exists)
    const agentFiles = fs.existsSync(agentDir) ? readMdFiles(agentDir) : [];

    // If no content, skip
    if (agentFiles.length === 0) {
      skipped++;
      continue;
    }

    // Build concatenated source for hashing
    const allSourceContent = agentFiles.map((f) => f.content).join("\n");

    const sourceHash = sha256(allSourceContent);

    // Check existing hash
    const outputPath = path.join(ROOT, ".claude", "context", `${agent}.context.md`);
    const existingHash = extractExistingHash(outputPath);

    if (existingHash === sourceHash) {
      skipped++;
      continue;
    }

    // Build the compiled content
    const agentTitle = agent.charAt(0).toUpperCase() + agent.slice(1);
    const parts = [];

    parts.push(`<!-- compiled-from-hash: ${sourceHash} -->`);
    parts.push(`<!-- compiled-at: ${new Date().toISOString()} -->`);
    parts.push(`# ${agentTitle} Compiled Context`);
    parts.push("");

    // Agent Context section
    parts.push(`## ${agentTitle} Context`);
    parts.push("");
    if (agentFiles.length > 0) {
      for (const f of agentFiles) {
        parts.push(f.content.trimEnd());
        parts.push("");
      }
    } else {
      parts.push("_No agent-specific context files._");
      parts.push("");
    }

    fs.writeFileSync(outputPath, compactContent(parts.join("\n")), "utf-8");
    compiled++;
  }

  return { compiled, skipped };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  // 1. Run sync-detect.js to discover subprojects and agents
  const detected = runSyncDetect();

  // 2. Copy commands from subprojects to context directories
  const copied = copyCommands(detected.subprojects);

  // 3. Compile context files for each agent
  const { compiled, skipped } = compileContexts(detected.agents);

  // 4. Output result
  const result = { copied, compiled, skipped };
  process.stdout.write(JSON.stringify(result, null, 2) + "\n");
}

main();
