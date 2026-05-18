#!/usr/bin/env bun
"use strict";

/**
 * wave-size-check.js
 *
 * Advisory audit of per-wave size inside a wave-plan. exec-rewave-check.js only
 * decomposes a flat spec; once a spec is a wave-plan, nothing flags an oversized
 * *individual* wave (e.g. 14 backend files in one agent). This script audits each
 * wave and WARNS (never blocks — heuristics in Mustard are always advisory) so the
 * user can choose to re-plan.
 *
 * CLI:
 *   bun .claude/scripts/wave-size-check.js --spec-dir <path-to-spec-dir>
 *
 * Output: one JSON line on stdout, always exit 0 (fail-open).
 *   { action: "skip",    reason: "not-a-wave-plan" | "no-spec-dir-arg" | "error-fallback", error? }
 *   { action: "audited", specDir, limit, oversizedCount, waves: [
 *       { wave, folder, fileCount, layerCount, oversized, reason } |
 *       { wave, folder, status: "stub" | "unknown" }
 *     ] }
 *
 * Env:
 *   MUSTARD_WAVE_SIZE_LIMIT — file-count threshold (default 10, floor 3)
 */

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const { detectRole, parseFilesSection } = require("./_lib/wave-lib");
const parseFiles = parseFilesSection;

function emit(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function parseArgs(argv) {
  const args = { specDir: null };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--spec-dir") args.specDir = argv[++i];
  }
  return args;
}

// ── resolve the file-count threshold (default 10, floor 3) ────────────────────
function resolveLimit() {
  const parsed = parseInt(process.env.MUSTARD_WAVE_SIZE_LIMIT, 10);
  if (Number.isNaN(parsed)) return 10;
  return parsed < 3 ? 3 : parsed;
}

// ── enumerate waves via wave-tree.js; fall back to on-disk wave-N-* dirs ───────
function enumerateWaves(specDir, scriptsDir) {
  const scriptPath = path.join(scriptsDir, "wave-tree.js");
  if (fs.existsSync(scriptPath)) {
    const result = spawnSync(
      process.execPath,
      [scriptPath, "--spec-dir", specDir, "--format", "json"],
      { encoding: "utf8", timeout: 15000 }
    );
    if (result.status === 0 && !result.error) {
      try {
        const parsed = JSON.parse(result.stdout.trim());
        if (
          parsed &&
          parsed.kind === "wave-plan" &&
          Array.isArray(parsed.waves) &&
          parsed.waves.length > 0
        ) {
          return { kind: "wave-plan", waves: parsed.waves };
        }
        // wave-tree reported "single"/"empty" → definitively not a wave-plan.
        if (parsed && parsed.kind && parsed.kind !== "wave-plan") {
          return { kind: parsed.kind, waves: [] };
        }
        // kind "wave-plan" but zero rows parsed → fall through to disk scan.
      } catch (_e) {
        /* fall through to disk scan */
      }
    }
  }
  // Fallback: a wave-plan.md + wave-N-* folders means this IS a wave-plan even
  // when wave-tree.js is unavailable (stale install) or its table parse missed.
  if (!fs.existsSync(path.join(specDir, "wave-plan.md"))) {
    return { kind: "unknown", waves: [] };
  }
  const folders = fs
    .readdirSync(specDir, { withFileTypes: true })
    .filter((d) => d.isDirectory() && /^wave-\d+/i.test(d.name))
    .map((d) => d.name)
    .sort((a, b) => {
      const na = parseInt((a.match(/\d+/) || [0])[0], 10);
      const nb = parseInt((b.match(/\d+/) || [0])[0], 10);
      return na - nb;
    });
  return { kind: "wave-plan", waves: folders.map((f) => ({ folder: f })) };
}

// ── extract wave number from a folder name or wave-tree label ─────────────────
function waveNumberOf(wave) {
  const src = `${wave.folder || ""} ${wave.label || ""}`;
  const m = src.match(/\d+/);
  return m ? parseInt(m[0], 10) : null;
}

// ── try to extract a wave's file list from the wave-plan.md (for stub waves) ──
// Real wave-plan.md formats observed: a markdown table row (no Files cell), or a
// "### Wave N" section with a "Files (N): a, b, c" line (exec-rewave layout).
function filesFromWavePlan(specDir, waveNum) {
  const wavePlanPath = path.join(specDir, "wave-plan.md");
  if (waveNum == null || !fs.existsSync(wavePlanPath)) return null;
  let text;
  try {
    text = fs.readFileSync(wavePlanPath, "utf8");
  } catch (_e) {
    return null;
  }
  const lines = text.split(/\r?\n/);

  // 1. "### Wave N" section → "Files (N): a, b, c"
  const headerRe = new RegExp(`^#{2,4}\\s*Wave\\s*${waveNum}\\b`, "i");
  for (let i = 0; i < lines.length; i++) {
    if (!headerRe.test(lines[i].trim())) continue;
    for (let j = i + 1; j < lines.length; j++) {
      const l = lines[j].trim();
      if (/^#{2,4}\s/.test(l)) break; // next section
      const fm = l.match(/^Files\s*\(\d+\)\s*:\s*(.+)$/i);
      if (fm) {
        return fm[1]
          .split(",")
          .map((s) => s.trim().replace(/^`|`$/g, ""))
          .filter(Boolean);
      }
    }
  }

  // 2. table row "| W3 | ... |" with a cell that looks like a file list.
  const rowRe = new RegExp(`^\\|\\s*W?${waveNum}\\b`, "i");
  for (const line of lines) {
    if (!rowRe.test(line.trim())) continue;
    const cells = line
      .split("|")
      .map((c) => c.trim())
      .filter((c) => c.length > 0);
    for (const c of cells) {
      // a cell carrying a comma-separated list of path-like tokens
      if (/[\/\\]/.test(c) && c.includes(",")) {
        const parts = c
          .split(",")
          .map((s) => s.trim().replace(/^`|`$/g, ""))
          .filter((s) => /[\/\\]/.test(s));
        if (parts.length > 0) return parts;
      }
    }
  }
  return null;
}

// ── call scope-decompose.js ───────────────────────────────────────────────────
function runScopeDecompose(signals, scriptsDir) {
  const scriptPath = path.join(scriptsDir, "scope-decompose.js");
  if (!fs.existsSync(scriptPath)) return null;
  const result = spawnSync(process.execPath, [scriptPath], {
    input: JSON.stringify(signals),
    encoding: "utf8",
    timeout: 10000,
  });
  if (result.status !== 0 || result.error) return null;
  try {
    return JSON.parse(result.stdout.trim());
  } catch (_e) {
    return null;
  }
}

// ── audit a single wave (returns the per-wave result object) ──────────────────
function auditWave(wave, specDir, scriptsDir, limit) {
  const folder = wave.folder || null;
  const waveNum = waveNumberOf(wave);

  // Prefer the wave's own spec.md ## Files section (most precise).
  let files = null;
  let source = null;
  if (folder) {
    const waveSpecPath = path.join(specDir, folder, "spec.md");
    if (fs.existsSync(waveSpecPath)) {
      try {
        files = parseFiles(fs.readFileSync(waveSpecPath, "utf8"));
        if (files && files.length > 0) source = "wave-spec";
      } catch (_e) {
        /* ignore */
      }
    }
  }

  // Stub wave (no expanded spec): try the wave-plan.md.
  if (!files || files.length === 0) {
    const planFiles = filesFromWavePlan(specDir, waveNum);
    if (planFiles && planFiles.length > 0) {
      files = planFiles;
      source = "wave-plan";
    }
  }

  if (!files || files.length === 0) {
    // No spec.md → likely a stub; no file list anywhere → unknown.
    const waveSpecExists =
      folder && fs.existsSync(path.join(specDir, folder, "spec.md"));
    return {
      wave: waveNum,
      folder,
      status: waveSpecExists ? "unknown" : "stub",
    };
  }

  const fileCount = files.length;
  const roles = files.map(detectRole);
  const uniqueRoles = new Set(roles);
  const layerCount =
    uniqueRoles.size === 1 && uniqueRoles.has("lib") ? 1 : uniqueRoles.size;

  const decision = runScopeDecompose(
    { fileCount, layerCount, newEntityCount: 0, knowledgeMatches: [] },
    scriptsDir
  );

  const reasons = [];
  if (decision && decision.decompose === true && decision.reason) {
    reasons.push(decision.reason);
  }
  if (fileCount > limit) {
    reasons.push(`file-count:${fileCount}>${limit}`);
  }
  const oversized = reasons.length > 0;

  return {
    wave: waveNum,
    folder,
    fileCount,
    layerCount,
    oversized,
    reason: reasons.join("; "),
    source,
  };
}

// ── main ──────────────────────────────────────────────────────────────────────
function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.specDir) {
    emit({ action: "skip", reason: "no-spec-dir-arg" });
    return;
  }

  try {
    const specDir = path.isAbsolute(args.specDir)
      ? args.specDir
      : path.resolve(process.cwd(), args.specDir);

    if (!fs.existsSync(specDir)) {
      emit({ action: "skip", reason: "error-fallback", error: "spec-dir-not-found" });
      return;
    }

    const scriptsDir = path.dirname(path.resolve(__filename));
    const enumerated = enumerateWaves(specDir, scriptsDir);

    if (enumerated.kind !== "wave-plan" || enumerated.waves.length === 0) {
      emit({ action: "skip", reason: "not-a-wave-plan" });
      return;
    }

    const limit = resolveLimit();
    const waves = enumerated.waves.map((w) =>
      auditWave(w, specDir, scriptsDir, limit)
    );
    const oversizedCount = waves.filter((w) => w.oversized === true).length;

    emit({ action: "audited", specDir, limit, oversizedCount, waves });
  } catch (err) {
    emit({ action: "skip", reason: "error-fallback", error: err.message });
  }
}

main();
