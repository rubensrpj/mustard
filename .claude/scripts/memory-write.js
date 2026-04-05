#!/usr/bin/env node

/**
 * memory-write.js
 *
 * Receives a JSON memory entry and persists it to
 * {projectDir}/.claude/.agent-memory/.
 *
 * Input (two modes):
 *   1. --json '<JSON>' CLI arg (Windows-friendly, avoids shell echo pipe issues)
 *   2. stdin piped JSON (POSIX)
 *
 * Input schema:
 *   {
 *     "agent_type": "templates-impl",
 *     "wave": 1,
 *     "pipeline": "2026-03-25-feature-name",
 *     "summary": "...",
 *     "details": { ... },
 *     "cwd": "/optional/project/root"   // optional override
 *   }
 *
 * Behaviour:
 *   1. Parse JSON from stdin.
 *   2. Resolve project dir from input.cwd || process.cwd().
 *   3. Ensure {projectDir}/.claude/.agent-memory/ exists.
 *   4. Derive session prefix from .agent-state/*.json or process.ppid.
 *   5. Write {id}.json with the full entry.
 *   6. Maintain a rolling _index.json (max 20 entries, oldest pruned with files).
 *   7. Exit 0 always (fail-open).
 *
 * @version 1.0.0
 */

"use strict";

const fs   = require("fs");
const path = require("path");

// ---------------------------------------------------------------------------
// Summary truncation
// ---------------------------------------------------------------------------

/**
 * Truncate a summary string to at most maxLen characters, breaking at the
 * last sentence boundary (`.`, `!`, `?`) before the limit.
 * Falls back to hard truncation at maxLen-3 with `...`.
 *
 * @param {string} text
 * @param {number} [maxLen=300]
 * @returns {string}
 */
function truncateSummary(text, maxLen = 300) {
  if (text.length <= maxLen) return text;

  // Search backwards from position maxLen for a sentence-ending punctuation.
  const slice = text.slice(0, maxLen);
  const lastBoundary = Math.max(
    slice.lastIndexOf("."),
    slice.lastIndexOf("!"),
    slice.lastIndexOf("?")
  );

  if (lastBoundary > 0) {
    return text.slice(0, lastBoundary + 1);
  }

  // No sentence boundary — hard truncate.
  return text.slice(0, maxLen - 3) + "...";
}

// ---------------------------------------------------------------------------
// Session prefix resolution
// ---------------------------------------------------------------------------

/**
 * Try to read any .json file from {projectDir}/.claude/.agent-state/
 * (excluding _queue.json) and extract session_id.
 * Returns first 8 characters of that ID, or falls back to process.ppid as string.
 *
 * @param {string} projectDir
 * @returns {string}
 */
function resolveSessionPrefix(projectDir) {
  const stateDir = path.join(projectDir, ".claude", ".agent-state");

  try {
    if (fs.existsSync(stateDir)) {
      const files = fs.readdirSync(stateDir).filter(
        (f) => f.endsWith(".json") && f !== "_queue.json"
      );

      for (const file of files) {
        try {
          const raw = fs.readFileSync(path.join(stateDir, file), "utf-8");
          const parsed = JSON.parse(raw);
          if (parsed && typeof parsed.session_id === "string" && parsed.session_id.length >= 1) {
            return parsed.session_id.slice(0, 8);
          }
        } catch {
          // try next file
        }
      }
    }
  } catch {
    // fall through to ppid fallback
  }

  return String(process.ppid);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  let raw = "";

  // --json arg mode (Windows-friendly: avoids shell echo pipe issues)
  const jsonArgIdx = process.argv.indexOf("--json");
  if (jsonArgIdx !== -1 && process.argv[jsonArgIdx + 1]) {
    raw = process.argv[jsonArgIdx + 1];
  } else {
    // stdin fallback (POSIX)
    for await (const chunk of process.stdin) {
      raw += chunk;
    }
  }

  let input;
  try {
    input = JSON.parse(raw);
  } catch (err) {
    process.stderr.write(`[memory-write] Failed to parse input JSON: ${err.message}\n`);
    process.exit(0);
  }

  try {
    // 1. Resolve project dir.
    const projectDir = (typeof input.cwd === "string" && input.cwd.length > 0)
      ? input.cwd
      : process.cwd();

    // 2. Ensure memory dir exists.
    const memDir = path.join(projectDir, ".claude", ".agent-memory");
    fs.mkdirSync(memDir, { recursive: true });

    // 3. Derive session prefix.
    const session8 = resolveSessionPrefix(projectDir);

    // 4. Generate ID and filename.
    const agentType = String(input.agent_type || "unknown");
    const id       = `${session8}-${agentType}-${Date.now()}`;
    const filename = `${id}.json`;

    // 5. Build truncated summary.
    const rawSummary     = String(input.summary || "");
    const truncatedSummary = truncateSummary(rawSummary);

    // 6. Build and write full memory entry.
    const timestamp = new Date().toISOString();
    const entry = {
      v:          1,
      id,
      session:    session8,
      agent_type: agentType,
      wave:       typeof input.wave === "number" ? input.wave : null,
      pipeline:   String(input.pipeline || ""),
      timestamp,
      summary:    truncatedSummary,
      details:    input.details || {},
    };

    fs.writeFileSync(
      path.join(memDir, filename),
      JSON.stringify(entry, null, 2),
      "utf-8"
    );

    // 7. Read existing index or start fresh.
    const indexPath = path.join(memDir, "_index.json");
    let index = [];
    try {
      const indexRaw = fs.readFileSync(indexPath, "utf-8");
      index = JSON.parse(indexRaw);
      if (!Array.isArray(index)) index = [];
    } catch {
      index = [];
    }

    // 8. Append new index entry.
    index.push({
      id,
      file:       filename,
      agent_type: agentType,
      wave:       typeof input.wave === "number" ? input.wave : null,
      pipeline:   String(input.pipeline || ""),
      summary:    truncatedSummary,
      timestamp,
    });

    // 9. Prune oldest entries if index exceeds 20.
    const MAX_INDEX = 20;
    if (index.length > MAX_INDEX) {
      const excess = index.splice(0, index.length - MAX_INDEX);
      for (const old of excess) {
        try {
          fs.unlinkSync(path.join(memDir, old.file));
        } catch {
          // file may already be gone — ignore
        }
      }
    }

    // 10. Write updated index.
    fs.writeFileSync(indexPath, JSON.stringify(index, null, 2), "utf-8");

  } catch (err) {
    process.stderr.write(`[memory-write] Unexpected error: ${err.message}\n`);
  }

  // Always exit 0 (fail-open).
  process.exit(0);
}

main();
