#!/usr/bin/env bun
"use strict";

/**
 * exec-rewave-check.js
 *
 * Pre-EXECUTE re-check: silently decomposes a single-spec into waves if signals
 * (layerCount >= 2) are found in the finalised spec's ## Files section.
 *
 * CLI:
 *   bun .claude/scripts/exec-rewave-check.js --spec .claude/spec/active/{name}/spec.md
 *
 * Output: one JSON line on stdout, always exit 0 (fail-open).
 *   { action: "skip",        reason: "already-decomposed" | "user-rejected" | "no-spec-arg" | "error-fallback", error? }
 *   { action: "keep-single", reason: "single-layer" | "no-dag-depth-or-error" | ..., signals? }
 *   { action: "decomposed",  totalWaves: N, waves: [{wave, role, files: count}, ...] }
 */

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const { detectRole, parseFilesSection } = require("./_lib/wave-lib");

function emit(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

// ── parse ## Files section from spec text ────────────────────────────────────
const parseFiles = parseFilesSection;

// ── extract optional newEntityCount from spec ─────────────────────────────────
function parseNewEntityCount(specText) {
  const m = specText.match(/new\s+entities?:\s*(\d+)/i) ||
            specText.match(/newEntityCount[:\s]+(\d+)/i);
  return m ? parseInt(m[1], 10) : 0;
}

// ── locate pipeline state ─────────────────────────────────────────────────────
function findPipelineState(specDir, projectRoot) {
  const specName = path.basename(specDir);
  const statesDir = path.join(projectRoot, ".claude", ".pipeline-states");
  const stateFile = path.join(statesDir, `${specName}.json`);
  try {
    const raw = fs.readFileSync(stateFile, "utf8");
    return { file: stateFile, state: JSON.parse(raw) };
  } catch {
    return { file: stateFile, state: null };
  }
}

// ── call scope-decompose.js ───────────────────────────────────────────────────
function runScopeDecompose(signals, scriptsDir) {
  const scriptPath = path.join(scriptsDir, "scope-decompose.js");
  const result = spawnSync(process.execPath, [scriptPath], {
    input: JSON.stringify(signals),
    encoding: "utf8",
    timeout: 10000,
  });
  if (result.status !== 0 || result.error) return null;
  try {
    return JSON.parse(result.stdout.trim());
  } catch {
    return null;
  }
}

// ── call wave-dependency.js ───────────────────────────────────────────────────
function runWaveDependency(files, projectRoot, scriptsDir) {
  const scriptPath = path.join(scriptsDir, "wave-dependency.js");
  const result = spawnSync(process.execPath, [scriptPath], {
    input: JSON.stringify({ files, projectRoot }),
    encoding: "utf8",
    cwd: projectRoot,
    timeout: 15000,
  });
  if (result.status !== 0 || result.error) return null;
  try {
    return JSON.parse(result.stdout.trim());
  } catch {
    return null;
  }
}

// ── generate wave-plan.md content ─────────────────────────────────────────────
function buildWavePlanMd(specName, wavesResult, specText, decomposeReason) {
  const now = new Date().toISOString();
  const summaryMatch = specText.match(/^##\s+Summary\s*\n([\s\S]*?)(?=\n##\s)/m);
  const summary = summaryMatch ? summaryMatch[1].trim() : "(see spec)";

  const waveLines = wavesResult.waves.map((w) => {
    const depends = w.dependsOn.length === 0 ? "none" : w.dependsOn.map((d) => `wave ${d}`).join(", ");
    return `### Wave ${w.wave} — ${w.roles.join("/")}
Depends on: ${depends}
Files (${w.files.length}): ${w.files.join(", ")}`;
  }).join("\n\n");

  return `<!-- mustard:generated -->
# Wave Plan: ${specName}
### Status: draft | Phase: EXECUTE | Scope: full | Decomposed: yes
### Checkpoint: ${now}
### Reason: ${decomposeReason}
### Source: exec-rewave-check (re-evaluated at EXECUTE entry)

## Summary
${summary}

## Waves
${waveLines}

## Rationale
Decomposed at EXECUTE entry by exec-rewave-check.js.
Threshold: layerCount >= 2 (reason: ${decomposeReason}).
`;
}

// ── generate per-wave spec.md content ─────────────────────────────────────────
function buildWaveSpecMd(parentSpecText, waveFiles, waveNum, waveRole, wavePlanRelPath) {
  // Extract Summary
  const summaryMatch = parentSpecText.match(/^##\s+Summary\s*\n([\s\S]*?)(?=\n##\s)/m);
  const summary = summaryMatch ? summaryMatch[1].trim() : "(see parent spec)";

  // Extract Tasks section (copy entirely — agent will filter)
  const tasksMatch = parentSpecText.match(/^##\s+Tasks\s*\n([\s\S]*?)(?=\n##\s|\s*$)/m);
  const tasks = tasksMatch ? tasksMatch[1].trim() : "";

  const fileList = waveFiles.map((f) => `- ${f}`).join("\n");

  return `<!-- mustard:generated -->
> Wave spec — see [../wave-plan.md](${wavePlanRelPath}) for overall plan.

# Wave ${waveNum} — ${waveRole}

## Summary
${summary}

## Files
${fileList}

## Tasks
${tasks}
`;
}

// ── main ──────────────────────────────────────────────────────────────────────
function main() {
  const args = process.argv.slice(2);
  const specArgIdx = args.indexOf("--spec");
  if (specArgIdx === -1 || !args[specArgIdx + 1]) {
    emit({ action: "skip", reason: "no-spec-arg" });
    return;
  }

  const specArg = args[specArgIdx + 1];
  // Resolve spec path relative to cwd
  const specFile = path.isAbsolute(specArg) ? specArg : path.resolve(process.cwd(), specArg);
  const specDir = path.dirname(specFile);
  // scriptsDir = same dir as this script
  const scriptsDir = path.dirname(path.resolve(__filename));
  // projectRoot = walk up until we find .claude dir (or fallback to cwd)
  const projectRoot = findProjectRoot(specDir) || process.cwd();

  try {
    // 1. Read spec
    let specText;
    try {
      specText = fs.readFileSync(specFile, "utf8");
    } catch {
      emit({ action: "skip", reason: "error-fallback", error: "spec-not-readable" });
      return;
    }

    const specName = path.basename(specDir);

    // 2. Skip if already decomposed (wave-plan.md exists in same dir)
    const wavePlanPath = path.join(specDir, "wave-plan.md");
    if (fs.existsSync(wavePlanPath)) {
      emit({ action: "skip", reason: "already-decomposed" });
      return;
    }

    // 3. Skip if pipeline-state says so
    const { file: stateFile, state } = findPipelineState(specDir, projectRoot);
    if (state) {
      if (state.isWavePlan === true) {
        emit({ action: "skip", reason: "already-decomposed" });
        return;
      }
      if (state.scopeOverride === "user-rejected-waves") {
        emit({ action: "skip", reason: "user-rejected" });
        return;
      }
    }

    // 4. Parse ## Files
    const filePaths = parseFiles(specText);
    if (!filePaths || filePaths.length === 0) {
      emit({ action: "skip", reason: "error-fallback", error: "no-files-section" });
      return;
    }

    // 5. Compute layerCount (unique roles, lib-only = 1 layer)
    const roles = filePaths.map(detectRole);
    const uniqueRoles = new Set(roles);
    // If only "lib" → 1 layer; otherwise count all unique roles
    const layerCount = (uniqueRoles.size === 1 && uniqueRoles.has("lib")) ? 1 : uniqueRoles.size;
    const fileCount = filePaths.length;
    const newEntityCount = parseNewEntityCount(specText);

    // 6. Call scope-decompose.js
    const signals = { fileCount, layerCount, newEntityCount, knowledgeMatches: [] };
    const decision = runScopeDecompose(signals, scriptsDir);

    if (!decision || decision.decompose === false) {
      const reason = decision ? decision.reason : "error-fallback";
      emit({ action: "keep-single", reason, signals });
      return;
    }

    // 7. decompose: true — call wave-dependency.js
    const dagResult = runWaveDependency(filePaths, projectRoot, scriptsDir);

    if (!dagResult || dagResult.error || !Array.isArray(dagResult.waves) || dagResult.waves.length < 2) {
      emit({ action: "keep-single", reason: "no-dag-depth-or-error", signals });
      return;
    }

    // 8. Write wave structure
    const wavePlanContent = buildWavePlanMd(specName, dagResult, specText, decision.reason);
    fs.writeFileSync(wavePlanPath, wavePlanContent, "utf8");

    const wavesMeta = [];
    for (const w of dagResult.waves) {
      const primaryRole = w.roles[0] || "lib";
      const waveDir = path.join(specDir, `wave-${w.wave}-${primaryRole}`);
      fs.mkdirSync(waveDir, { recursive: true });
      const waveSpecPath = path.join(waveDir, "spec.md");
      const waveSpecContent = buildWaveSpecMd(specText, w.files, w.wave, w.roles.join("/"), "../wave-plan.md");
      fs.writeFileSync(waveSpecPath, waveSpecContent, "utf8");
      wavesMeta.push({ wave: w.wave, role: primaryRole, files: w.files.length });
    }

    // 9. Rename original spec to spec.original.md
    const originalBackup = path.join(specDir, "spec.original.md");
    fs.renameSync(specFile, originalBackup);

    // 10. Update pipeline state
    const updatedState = Object.assign({}, state || { specName }, {
      specName,
      isWavePlan: true,
      currentWave: 1,
      totalWaves: dagResult.waves.length,
      completedWaves: [],
      failedWaves: [],
      rewaveSource: "exec-entry",
      updatedAt: new Date().toISOString(),
    });

    const statesDir = path.join(projectRoot, ".claude", ".pipeline-states");
    fs.mkdirSync(statesDir, { recursive: true });
    fs.writeFileSync(stateFile, JSON.stringify(updatedState, null, 2), "utf8");

    emit({ action: "decomposed", totalWaves: dagResult.waves.length, waves: wavesMeta });
  } catch (err) {
    emit({ action: "skip", reason: "error-fallback", error: err.message });
  }
}

function findProjectRoot(startDir) {
  let dir = startDir;
  for (let i = 0; i < 10; i++) {
    if (fs.existsSync(path.join(dir, ".claude"))) return dir;
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return null;
}

main();
