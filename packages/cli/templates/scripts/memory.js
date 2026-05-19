#!/usr/bin/env bun
/**
 * memory.js — unified persistence CLI
 *
 * Subcommands:
 *   memory.js agent     [--json '<JSON>']  → .claude/.agent-memory/  (cap 20)
 *   memory.js decision  [--json '<JSON>']  → .claude/memory/decisions.json (cap 50)
 *   memory.js knowledge [--json '<JSON>']  → .claude/knowledge.json  (cap 200)
 *
 * Input modes (both supported):
 *   --json '<JSON>'   Windows-friendly CLI arg
 *   stdin piped JSON  POSIX fallback
 *
 * Exit 0 always (fail-open).
 *
 * @version 1.0.0
 */

"use strict";

const fs   = require("fs");
const path = require("path");

// ── Harness event bus (optional) ─────────────────────────────────────────────
let harnessEmit = null;
try {
  const he = require("../hooks/_lib/harness-event.js");
  harnessEmit = he.emit;
} catch (_) {} // fail-open

function emitEvent(eventName, payload, projectDir) {
  try {
    if (!harnessEmit) return;
    const sessionId =
      process.env.MUSTARD_SESSION_ID ||
      process.env.CLAUDE_SESSION_ID ||
      null;
    harnessEmit(eventName, payload, {
      cwd: projectDir,
      sessionId,
      actor: { kind: "hook", id: "memory" },
    });
  } catch (_) {} // fail-open
}

// ── Shared: read raw input ────────────────────────────────────────────────────

async function readInput() {
  const jsonArgIdx = process.argv.indexOf("--json");
  if (jsonArgIdx !== -1 && process.argv[jsonArgIdx + 1]) {
    return process.argv[jsonArgIdx + 1];
  }
  // stdin fallback
  let raw = "";
  for await (const chunk of process.stdin) {
    raw += chunk;
  }
  return raw;
}

// ── agent subcommand ──────────────────────────────────────────────────────────
// Mirrors memory-write.js exactly.
// Store: {projectDir}/.claude/.agent-memory/  Cap: 20

const AGENT_CAP = 20;

function truncateSummary(text, maxLen = 300) {
  if (text.length <= maxLen) return text;
  const slice = text.slice(0, maxLen);
  const lastBoundary = Math.max(
    slice.lastIndexOf("."),
    slice.lastIndexOf("!"),
    slice.lastIndexOf("?")
  );
  if (lastBoundary > 0) return text.slice(0, lastBoundary + 1);
  return text.slice(0, maxLen - 3) + "...";
}

function resolveSessionPrefix(projectDir) {
  const stateDir = path.join(projectDir, ".claude", ".agent-state");
  try {
    if (fs.existsSync(stateDir)) {
      const files = fs.readdirSync(stateDir).filter(
        (f) => f.endsWith(".json") && f !== "_queue.json"
      );
      for (const file of files) {
        try {
          const parsed = JSON.parse(
            fs.readFileSync(path.join(stateDir, file), "utf-8")
          );
          if (parsed && typeof parsed.session_id === "string" && parsed.session_id.length >= 1) {
            return parsed.session_id.slice(0, 8);
          }
        } catch { /* try next */ }
      }
    }
  } catch { /* fall through */ }
  return String(process.ppid);
}

function runAgent(input) {
  const projectDir = (typeof input.cwd === "string" && input.cwd.length > 0)
    ? input.cwd
    : process.cwd();

  const memDir = path.join(projectDir, ".claude", ".agent-memory");
  fs.mkdirSync(memDir, { recursive: true });

  const session8  = resolveSessionPrefix(projectDir);
  const agentType = String(input.agent_type || "unknown");
  const id        = `${session8}-${agentType}-${Date.now()}`;
  const filename  = `${id}.json`;

  const rawSummary      = String(input.summary || "");
  const truncatedSummary = truncateSummary(rawSummary);
  const timestamp        = new Date().toISOString();

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

  // Rolling index
  const indexPath = path.join(memDir, "_index.json");
  let index = [];
  try {
    const raw = fs.readFileSync(indexPath, "utf-8");
    index = JSON.parse(raw);
    if (!Array.isArray(index)) index = [];
  } catch { index = []; }

  index.push({
    id,
    file:       filename,
    agent_type: agentType,
    wave:       typeof input.wave === "number" ? input.wave : null,
    pipeline:   String(input.pipeline || ""),
    summary:    truncatedSummary,
    timestamp,
  });

  if (index.length > AGENT_CAP) {
    const excess = index.splice(0, index.length - AGENT_CAP);
    for (const old of excess) {
      try { fs.unlinkSync(path.join(memDir, old.file)); } catch { /* gone */ }
    }
  }

  fs.writeFileSync(indexPath, JSON.stringify(index, null, 2), "utf-8");
}

// ── decision subcommand ───────────────────────────────────────────────────────
// Mirrors memory-persist.js exactly.
// Store: {projectDir}/.claude/memory/decisions.json | lessons.json  Cap: 50

const DECISION_CAP = 50;

function runDecision(input) {
  const entryType = String(input.type || "").trim();
  if (entryType !== "decision" && entryType !== "lesson") {
    process.stderr.write(
      `[memory] decision: invalid type "${entryType}" — must be "decision" or "lesson"\n`
    );
    return;
  }

  const projectDir = (typeof input.cwd === "string" && input.cwd.length > 0)
    ? input.cwd
    : process.cwd();

  const memDir   = path.join(projectDir, ".claude", "memory");
  fs.mkdirSync(memDir, { recursive: true });

  const fileName = entryType === "decision" ? "decisions.json" : "lessons.json";
  const filePath = path.join(memDir, fileName);

  let data = { entries: [] };
  try {
    if (fs.existsSync(filePath)) {
      const parsed = JSON.parse(fs.readFileSync(filePath, "utf8"));
      if (parsed && Array.isArray(parsed.entries)) data = parsed;
    }
  } catch { data = { entries: [] }; }

  const timestamp = new Date().toISOString();
  const id        = `${entryType}-${Date.now()}`;
  const entry = {
    id,
    timestamp,
    content: String(input.content || ""),
    source:  String(input.source  || ""),
    context: String(input.context || ""),
  };

  data.entries.push(entry);

  if (data.entries.length > DECISION_CAP) {
    data.entries.splice(0, data.entries.length - DECISION_CAP);
  }

  // Harness events (Wave 2)
  if (entryType === "decision") {
    emitEvent("decision", {
      title:     entry.content.slice(0, 200),
      rationale: entry.context || null,
    }, projectDir);
  } else {
    emitEvent("lesson", {
      trigger:  entry.source || null,
      takeaway: entry.content.slice(0, 200),
    }, projectDir);
  }

  fs.writeFileSync(filePath, JSON.stringify(data, null, 2), "utf8");
}

// ── knowledge subcommand ──────────────────────────────────────────────────────
// Mirrors knowledge-update.js exactly.
// Store: {projectDir}/.claude/knowledge.json  Cap: 200 total / 80 per category

const KNOWLEDGE_CAP     = 200;
const KNOWLEDGE_CAP_CAT = 80;

function runKnowledge(input) {
  const cwd    = (typeof input.cwd === "string" && input.cwd.length > 0)
    ? input.cwd
    : process.cwd();
  const kbPath = path.join(cwd, ".claude", "knowledge.json");

  let kb = { version: 1, entries: [] };
  try {
    if (fs.existsSync(kbPath)) {
      kb = JSON.parse(fs.readFileSync(kbPath, "utf8"));
      if (!kb.entries) kb.entries = [];
    }
  } catch { kb = { version: 1, entries: [] }; }

  const type        = String(input.type || "pattern");
  const name        = String(input.name || "").trim();
  const description = String(input.description || "").trim();
  const source      = String(input.source || "unknown");
  const tags        = Array.isArray(input.tags) ? input.tags : [];
  const initialConfidence = (typeof input.confidence === "number" &&
    input.confidence >= 0 && input.confidence <= 1)
    ? input.confidence
    : 0.3;

  if (!name || !description) {
    process.stderr.write("[memory] knowledge: missing name or description\n");
    return;
  }

  const existingIdx = kb.entries.findIndex(
    (e) => e.name === name && e.type === type
  );
  const timestamp = new Date().toISOString();

  if (existingIdx >= 0) {
    const existing = kb.entries[existingIdx];
    existing.description = description;
    existing.source      = source;
    existing.tags        = tags;
    existing.updatedAt   = timestamp;
    const prevOccurrences = existing.occurrences != null ? existing.occurrences : 1;
    existing.occurrences  = prevOccurrences + 1;
    existing.confidence   = Math.min(1.0, 0.3 + (existing.occurrences * 0.1));
    existing.lastSeen     = timestamp;
  } else {
    kb.entries.push({
      id:          `${type}-${Date.now()}`,
      type,
      name,
      description,
      source,
      tags,
      confidence:  initialConfidence,
      occurrences: 1,
      createdAt:   timestamp,
      updatedAt:   timestamp,
      lastSeen:    timestamp,
    });
  }

  // Prune per category then global
  const byType = {};
  for (const e of kb.entries) {
    if (!byType[e.type]) byType[e.type] = [];
    byType[e.type].push(e);
  }
  const pruned = [];
  for (const entries of Object.values(byType)) {
    entries.sort((a, b) =>
      new Date(b.updatedAt || b.createdAt) - new Date(a.updatedAt || a.createdAt)
    );
    pruned.push(...entries.slice(0, KNOWLEDGE_CAP_CAT));
  }
  pruned.sort((a, b) =>
    new Date(b.updatedAt || b.createdAt) - new Date(a.updatedAt || a.createdAt)
  );
  kb.entries = pruned.slice(0, KNOWLEDGE_CAP);

  fs.mkdirSync(path.dirname(kbPath), { recursive: true });
  fs.writeFileSync(kbPath, JSON.stringify(kb, null, 2), "utf8");
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

async function main() {
  const subcommand = process.argv[2];

  if (!subcommand || !["agent", "decision", "knowledge"].includes(subcommand)) {
    process.stdout.write(
      "Usage: memory.js <agent|decision|knowledge> [--json '<JSON>']\n"
    );
    process.exit(0);
  }

  const raw = await readInput();

  let input;
  try {
    input = JSON.parse(raw);
  } catch (err) {
    process.stderr.write(`[memory] Failed to parse input JSON: ${err.message}\n`);
    process.exit(0);
  }

  try {
    if (subcommand === "agent")     runAgent(input);
    if (subcommand === "decision")  runDecision(input);
    if (subcommand === "knowledge") runKnowledge(input);
  } catch (err) {
    process.stderr.write(`[memory] Unexpected error (${subcommand}): ${err.message}\n`);
  }

  process.exit(0);
}

main();
