#!/usr/bin/env bun

/**
 * wave-tree.js
 *
 * Renders an ASCII or JSON tree of wave status for a given spec-dir.
 *
 * CLI:
 *   bun wave-tree.js --spec-dir <path> [--format ascii|json]
 *
 * Behavior:
 *   - missing --spec-dir              → stderr + exit 1
 *   - spec-dir does not exist         → stdout "(no spec at <p>)" + exit 0
 *   - <dir>/wave-plan.md exists       → parse table, render each wave folder
 *   - else if <dir>/spec.md exists    → single-spec line
 *
 * Fail-open everywhere else.
 */

"use strict";

const fs = require("node:fs");
const path = require("node:path");

const STATUS_ICONS = {
  completed: "[v]",
  implementing: "[>]",
  "closed-followup": "[~]",
  blocked: "[!]",
  rejected: "[!]",
};

function iconFor(status) {
  if (!status) return "[ ]";
  const s = String(status).toLowerCase().trim();
  return STATUS_ICONS[s] || "[ ]";
}

function parseArgs(argv) {
  const args = { specDir: null, format: "ascii" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--spec-dir") {
      args.specDir = argv[++i];
    } else if (a === "--format") {
      args.format = argv[++i] || "ascii";
    }
  }
  return args;
}

function readStatus(specFile) {
  try {
    if (!fs.existsSync(specFile)) return "queued";
    const content = fs.readFileSync(specFile, "utf8");
    const m = content.match(/^###\s*Status:\s*([a-z-]+)/im);
    return m ? m[1].toLowerCase() : "queued";
  } catch (_err) {
    return "queued";
  }
}

function parseWavePlan(wavePlanFile, specDir) {
  const content = fs.readFileSync(wavePlanFile, "utf8");
  const lines = content.split(/\r?\n/);
  const waves = [];
  const rowRe = /^\|\s*(W?\d+|Wave\s*\d+)\s*\|(.+)$/i;
  for (const line of lines) {
    const m = line.match(rowRe);
    if (!m) continue;
    const label = m[1].trim();
    const cells = m[2].split("|").map((c) => c.trim()).filter((c) => c.length > 0);
    // Find a cell that looks like a folder name (contains '-' or 'wave')
    let folder = null;
    for (let i = cells.length - 1; i >= 0; i--) {
      const c = cells[i];
      if (/^wave-\d+[-_a-z0-9]*/i.test(c) || /^[a-z0-9][-_a-z0-9]+$/i.test(c)) {
        folder = c;
        break;
      }
    }
    if (!folder) {
      // derive from label: "Wave 1" → "wave-1"
      const numMatch = label.match(/\d+/);
      folder = numMatch ? `wave-${numMatch[0]}` : label.toLowerCase().replace(/\s+/g, "-");
    }
    waves.push({ label, folder });
  }

  // Resolve actual folders on disk (may be wave-1-backend even when table says wave-1)
  const entries = fs.existsSync(specDir)
    ? fs.readdirSync(specDir, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name)
    : [];
  for (const w of waves) {
    if (!entries.includes(w.folder)) {
      const num = (w.folder.match(/\d+/) || w.label.match(/\d+/) || [])[0];
      if (num) {
        const match = entries.find((e) => new RegExp(`^wave-${num}(?:[-_]|$)`, "i").test(e));
        if (match) w.folder = match;
      }
    }
    const specFile = path.join(specDir, w.folder, "spec.md");
    w.status = readStatus(specFile);
    w.icon = iconFor(w.status);
  }
  return waves;
}

function renderAscii(root, waves) {
  const maxLen = waves.reduce((a, w) => Math.max(a, w.folder.length), 0);
  const pad = (s) => s + " ".repeat(Math.max(0, maxLen - s.length + 2));
  const lines = [`Roadmap: ${root}`];
  waves.forEach((w, i) => {
    const branch = i === waves.length - 1 ? "└─" : "├─";
    lines.push(`${branch} ${w.icon} ${pad(w.folder)}(${w.status})`);
  });
  return lines.join("\n");
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.specDir) {
    process.stderr.write("missing --spec-dir\n");
    process.exit(1);
  }
  const dir = path.resolve(args.specDir);
  if (!fs.existsSync(dir)) {
    process.stdout.write(`(no spec at ${args.specDir})\n`);
    process.exit(0);
  }
  const root = path.basename(dir);
  const wavePlan = path.join(dir, "wave-plan.md");
  const singleSpec = path.join(dir, "spec.md");

  try {
    if (fs.existsSync(wavePlan)) {
      const waves = parseWavePlan(wavePlan, dir);
      if (args.format === "json") {
        process.stdout.write(JSON.stringify({ kind: "wave-plan", root, waves }) + "\n");
      } else {
        process.stdout.write(renderAscii(root, waves) + "\n");
      }
      process.exit(0);
    }
    if (fs.existsSync(singleSpec)) {
      const status = readStatus(singleSpec);
      const icon = iconFor(status);
      if (args.format === "json") {
        process.stdout.write(
          JSON.stringify({ kind: "single", root, spec: { name: root, status, icon } }) + "\n",
        );
      } else {
        process.stdout.write(`Spec: ${root}  ${icon} (${status})\n`);
      }
      process.exit(0);
    }
    if (args.format === "json") {
      process.stdout.write(JSON.stringify({ kind: "empty", root, waves: [] }) + "\n");
    } else {
      process.stdout.write(`(no spec at ${args.specDir})\n`);
    }
    process.exit(0);
  } catch (_err) {
    process.stdout.write(`(no spec at ${args.specDir})\n`);
    process.exit(0);
  }
}

main();
