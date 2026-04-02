#!/usr/bin/env node

/**
 * memory-persist.js
 *
 * Receives a JSON entry from stdin and persists decisions/lessons to
 * {projectDir}/.claude/memory/.
 *
 * Input schema (stdin):
 *   {
 *     "type": "decision" | "lesson",
 *     "content": "Description of the decision or lesson",
 *     "source": "pipeline-name or agent-type",
 *     "context": "optional additional context",
 *     "cwd": "/optional/project/root"
 *   }
 *
 * Behaviour:
 *   1. Parse JSON from stdin.
 *   2. Resolve project dir from input.cwd || process.cwd().
 *   3. Ensure {projectDir}/.claude/memory/ exists.
 *   4. Read existing decisions.json or lessons.json (based on input.type).
 *   5. Append new entry with id, timestamp, content, source, context.
 *   6. Prune if exceeds 50 entries (oldest removed).
 *   7. Write back.
 *   8. Exit 0 always (fail-open).
 *
 * File structures:
 *   decisions.json: {"entries": [{"id": "...", "timestamp": "ISO", "content": "...", "source": "...", "context": "..."}]}
 *   lessons.json:   same structure
 *
 * @version 1.0.0
 */

"use strict";

const fs   = require("fs");
const path = require("path");

const MAX_ENTRIES = 50;

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

let raw = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (raw += chunk));
process.stdin.on("end", () => {
  let input;
  try {
    input = JSON.parse(raw);
  } catch (err) {
    process.stderr.write(`[memory-persist] Failed to parse stdin JSON: ${err.message}\n`);
    process.exit(0);
  }

  try {
    const entryType = String(input.type || "").trim();
    if (entryType !== "decision" && entryType !== "lesson") {
      process.stderr.write(`[memory-persist] Invalid type "${entryType}" — must be "decision" or "lesson"\n`);
      process.exit(0);
    }

    // 1. Resolve project dir.
    const projectDir = (typeof input.cwd === "string" && input.cwd.length > 0)
      ? input.cwd
      : process.cwd();

    // 2. Ensure memory dir exists.
    const memDir = path.join(projectDir, ".claude", "memory");
    fs.mkdirSync(memDir, { recursive: true });

    // 3. Determine target file.
    const fileName = entryType === "decision" ? "decisions.json" : "lessons.json";
    const filePath = path.join(memDir, fileName);

    // 4. Read existing entries.
    let data = { entries: [] };
    try {
      if (fs.existsSync(filePath)) {
        const parsed = JSON.parse(fs.readFileSync(filePath, "utf8"));
        if (parsed && Array.isArray(parsed.entries)) {
          data = parsed;
        }
      }
    } catch {
      data = { entries: [] };
    }

    // 5. Build new entry.
    const timestamp = new Date().toISOString();
    const id = `${entryType}-${Date.now()}`;
    const entry = {
      id,
      timestamp,
      content: String(input.content || ""),
      source:  String(input.source  || ""),
      context: String(input.context || ""),
    };

    data.entries.push(entry);

    // 6. Prune oldest if over limit.
    if (data.entries.length > MAX_ENTRIES) {
      data.entries.splice(0, data.entries.length - MAX_ENTRIES);
    }

    // 7. Write back.
    fs.writeFileSync(filePath, JSON.stringify(data, null, 2), "utf8");

  } catch (err) {
    process.stderr.write(`[memory-persist] Unexpected error: ${err.message}\n`);
  }

  // Always exit 0 (fail-open).
  process.exit(0);
});
