#!/usr/bin/env bun
"use strict";

/**
 * skill-validate.js
 *
 * Validates SKILL.md files across the project:
 *   - ROOT `.claude/skills/` (registry-generated role skills)
 *   - Each subproject's `{sub}/.claude/skills/` (agent-generated step 4.6 skills)
 *
 * Three modes:
 *   (default) Pure validation (YAML frontmatter, kebab-case name, description length/triggers,
 *             `source: scan|manual` field). Does NOT write/modify anything.
 *   --factual Additional factual checks against the codebase (cluster backing, sample existence,
 *             no-code rule, references integrity). Only applies to scan-generated skills
 *             (header `<!-- mustard:generated -->`). User-authored skills are skipped.
 *   --lines   Line-count check: reports per-skill lineCount and tier (ok|warn|strict-warn|block).
 *             Can be combined with --json and --factual.
 *
 * Usage:
 *   bun .claude/scripts/skill-validate.js                   # validate all (structural)
 *   bun .claude/scripts/skill-validate.js --json            # JSON output
 *   bun .claude/scripts/skill-validate.js --only scan       # skip manual skills
 *   bun .claude/scripts/skill-validate.js --quiet           # only show failures
 *   bun .claude/scripts/skill-validate.js --factual         # factual checks (--json also works)
 *   bun .claude/scripts/skill-validate.js --lines           # line-count check (--json also works)
 *   bun .claude/scripts/skill-validate.js --lines --json    # line-count as JSON with lineCount/tier
 *
 * Env:
 *   MUSTARD_SKILL_VALIDATE_MODE = strict (default) | warn | off
 *     Applies to --factual mode: strict exits non-zero on violations, warn prints but exits 0,
 *     off short-circuits the whole run (exit 0 without iterating).
 *   MUSTARD_SKILL_VALIDATE_LINES_MODE = warn (default) | strict | off
 *     Applies to --lines mode: strict exits 1 if any skill exceeds BLOCK_LINES (500),
 *     warn prints but exits 0, off skips the check entirely.
 *
 * Exit codes:
 *   0 — all skills valid (or none found, or off mode, or warn mode)
 *   1 — factual strict mode with violations, OR lines strict mode with block-tier skill
 *   2 — structural mode with at least one validation failure
 */

const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

const ROOT = path.resolve(__dirname, "..", "..");
const DETECT_CACHE_PATH = path.join(ROOT, ".claude", ".detect-cache.json");
const REGISTRY_PATH = path.join(ROOT, ".claude", "entity-registry.json");

const args = process.argv.slice(2);
const JSON_OUT = args.includes("--json");
const QUIET = args.includes("--quiet");
const FACTUAL = args.includes("--factual");
const LINES = args.includes("--lines");
const ONLY = (() => {
  const idx = args.indexOf("--only");
  return idx !== -1 && args[idx + 1] ? args[idx + 1] : null;
})();

const FACTUAL_MODE = (() => {
  const raw = (
    process.env.MUSTARD_SKILL_VALIDATE_MODE || "strict"
  ).toLowerCase();
  if (raw === "warn" || raw === "off" || raw === "strict") return raw;
  return "strict";
})();

// Line-count thresholds (used by --lines mode and skill-size-gate.js hook)
const WARN_LINES = 200;
const STRICT_WARN_LINES = 400;
const BLOCK_LINES = 500;

const LINES_MODE = (() => {
  const raw = (
    process.env.MUSTARD_SKILL_VALIDATE_LINES_MODE || "warn"
  ).toLowerCase();
  if (raw === "warn" || raw === "off" || raw === "strict") return raw;
  return "warn";
})();

function readJsonSafe(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf-8"));
  } catch {
    return null;
  }
}

/**
 * Attempt to validate a skill file using the skill-creator Python validator if available.
 * Fail-open: returns { ok: true, skipped: true } when Python or skill-creator is absent.
 * @param {string} skillPath - absolute path to SKILL.md
 * @returns {{ ok: boolean, skipped?: boolean, output?: string, errors?: string[] }}
 */
function validateWithPython(skillPath) {
  const validator = path.join(
    ROOT,
    ".claude",
    "skills",
    "skill-creator",
    "scripts",
    "quick_validate.py",
  );
  if (!fs.existsSync(validator)) return { ok: true, skipped: true };
  try {
    const out = execFileSync("python", [validator, skillPath], {
      encoding: "utf-8",
    });
    return { ok: true, output: out };
  } catch (err) {
    return { ok: false, errors: [err.stdout || err.message] };
  }
}

/**
 * Validate a SKILL.md body. Returns { ok, errors[] , source }.
 * @param {string} content
 * @returns {{ ok: boolean, errors: string[], source: string|null }}
 */
function validateSkill(content) {
  const errors = [];
  // Tolerate CRLF (Windows tools often author SKILL.md with CRLF line endings).
  const normalized = content.replace(/\r\n/g, "\n");
  const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
  if (!fm) {
    errors.push("missing YAML frontmatter");
    return { ok: false, errors, source: null };
  }

  const body = fm[1];
  const nameMatch = body.match(/^name:\s*(.+)$/m);
  const descMatch = body.match(
    /^description:\s*(?:"([\s\S]+?)"|([^\n]+(?:\n\s+[^\n]+)*))$/m,
  );
  const sourceMatch = body.match(/^source:\s*(scan|manual)$/m);

  if (!nameMatch) {
    errors.push('frontmatter: missing "name"');
  } else if (!/^[a-z][a-z0-9-]+$/.test(nameMatch[1].trim())) {
    errors.push(`name not kebab-case: ${nameMatch[1]}`);
  }

  if (!descMatch) {
    errors.push('frontmatter: missing "description"');
  } else {
    const raw = (descMatch[1] || descMatch[2] || "")
      .replace(/\s+/g, " ")
      .trim();
    if (raw.length < 50)
      errors.push(`description too short (${raw.length} chars, min 50)`);
    if (raw.length > 600)
      errors.push(`description too long (${raw.length} chars, max 600)`);
    if (
      !/\b(use when|when the user|add|create|new|detect|check|write|even if)\b/i.test(
        raw,
      )
    ) {
      errors.push(
        "description lacks trigger words (use when / when / add / create / ...)",
      );
    }
  }

  if (!sourceMatch)
    errors.push('frontmatter: missing "source" (expected scan|manual)');

  return {
    ok: errors.length === 0,
    errors,
    source: sourceMatch ? sourceMatch[1] : null,
  };
}

/**
 * Collect every SKILL.md under a skills/ directory (first level only).
 * @param {string} skillsDir
 * @returns {string[]} absolute SKILL.md paths
 */
function collectSkills(skillsDir) {
  if (!fs.existsSync(skillsDir)) return [];
  let entries;
  try {
    entries = fs.readdirSync(skillsDir, { withFileTypes: true });
  } catch {
    return [];
  }
  const out = [];
  for (const e of entries) {
    if (!e.isDirectory()) continue;
    const candidate = path.join(skillsDir, e.name, "SKILL.md");
    if (fs.existsSync(candidate)) out.push(candidate);
  }
  return out;
}

/**
 * Build the list of skill directories to validate:
 *   - ROOT/.claude/skills
 *   - every subproject/.claude/skills from detect cache
 *   - fallback: first-level directories under ROOT when cache is empty
 * @returns {Array<{ dir: string, label: string }>}
 */
function collectSkillDirs() {
  const dirs = [];
  dirs.push({ dir: path.join(ROOT, ".claude", "skills"), label: "<root>" });

  const cache = readJsonSafe(DETECT_CACHE_PATH);
  const subs = cache?.subprojects || [];
  for (const sub of subs) {
    const p = path.join(ROOT, sub.path, ".claude", "skills");
    dirs.push({ dir: p, label: sub.name });
  }

  // Bug 4 fix: if no subprojects found in cache (cache absent or empty), fall back to
  // scanning first-level directories under ROOT for {dir}/.claude/skills/ directories.
  if (subs.length === 0) {
    try {
      const entries = fs.readdirSync(ROOT, { withFileTypes: true });
      for (const e of entries) {
        if (!e.isDirectory()) continue;
        if (e.name.startsWith(".")) continue;
        const candidate = path.join(ROOT, e.name, ".claude", "skills");
        if (fs.existsSync(candidate)) {
          dirs.push({ dir: candidate, label: e.name });
        }
      }
    } catch (err) {
      process.stderr.write(
        `[skill-validate] fallback discovery failed: ${err.message}\n`,
      );
    }
  }

  return dirs;
}

// ============================================================================
// FACTUAL MODE
// ============================================================================

/** Slugify a string into lowercase kebab-case (matches skill-name suffix convention). */
function slugify(s) {
  if (!s) return "";
  return String(s)
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1-$2")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/**
 * Load all cluster suffixes with fileCount >= 3 from the entity registry.
 * @returns {{ suffixes: Set<string>, rawClusters: Array }}
 */
function loadClusterIndex() {
  const registry = readJsonSafe(REGISTRY_PATH);
  const suffixes = new Set();
  const rawClusters = [];
  if (!registry || !registry._patterns) return { suffixes, rawClusters };
  for (const stack of Object.keys(registry._patterns)) {
    const disc = registry._patterns[stack]?.discovered || [];
    for (const c of disc) {
      if (typeof c !== "object" || c == null) continue;
      const fc = typeof c.fileCount === "number" ? c.fileCount : 0;
      if (fc < 3) continue;
      const sfx = slugify(c.suffix || c.label || "");
      if (sfx) suffixes.add(sfx);
      rawClusters.push({ ...c, _stack: stack, _slugSuffix: sfx });
    }
  }
  return { suffixes, rawClusters };
}

/** Parse YAML-ish `name:` from frontmatter (cheap, no external dep). */
function extractSkillName(content) {
  const normalized = content.replace(/\r\n/g, "\n");
  const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
  if (!fm) return null;
  const nm = fm[1].match(/^name:\s*(.+)$/m);
  return nm ? nm[1].trim() : null;
}

/**
 * Extract path references from a SKILL.md body, limited to the
 * `## Real examples` / `## Samples in this project` sections. Accepts any of:
 *   - `- {name} — \`{path}\``
 *   - `- \`{path}\``
 *   - `- {name}: \`{path}\``
 *
 * @param {string} content
 * @returns {string[]} paths (may be project-relative)
 */
function extractSamplePaths(content) {
  const normalized = content.replace(/\r\n/g, "\n");
  const lines = normalized.split("\n");
  const paths = [];
  let inSection = false;
  const sectionRe =
    /^##\s+(Real examples|Samples in this project|Real examples in this codebase)/i;
  for (const line of lines) {
    if (/^##\s+/.test(line)) {
      inSection = sectionRe.test(line);
      continue;
    }
    if (!inSection) continue;
    // list item with a backticked path
    const m = line.match(/^\s*-\s+.*?`([^`]+)`/);
    if (m && m[1]) {
      // Bug 2 fix: only accept filesystem paths — must contain a path separator OR end with
      // a known file extension. Discard function calls, keywords, and other prose in backticks.
      const candidate = m[1].trim();
      const looksLikePath =
        /[/\\]/.test(candidate) || /\.[a-zA-Z0-9]{1,6}$/.test(candidate);
      if (candidate && !/^https?:/i.test(candidate) && looksLikePath)
        paths.push(candidate);
    }
  }
  return paths;
}

/** Count fenced code blocks in a SKILL.md body (outside frontmatter). */
function countFencedBlocks(content) {
  const normalized = content.replace(/\r\n/g, "\n");
  // strip frontmatter
  const stripped = normalized.replace(/^---\n[\s\S]*?\n---\n?/, "");
  const matches = stripped.match(/^```/gm);
  return matches ? matches.length : 0;
}

/**
 * Extract `Source: \`{path}\`` lines from references/examples.md.
 * @param {string} content
 * @returns {string[]}
 */
function extractReferenceSources(content) {
  const normalized = content.replace(/\r\n/g, "\n");
  const out = [];
  const re = /^\s*Source:\s*`([^`]+)`/gm;
  let m;
  while ((m = re.exec(normalized)) !== null) {
    if (m[1]) out.push(m[1].trim());
  }
  return out;
}

/**
 * Resolve a possibly-relative path. Tries subproject root first (skills authored
 * under {sub}/.claude/skills/ naturally reference paths from the subproject POV),
 * falls back to project ROOT. Absolute paths checked as-is.
 */
function pathExistsUnderRoot(p, subprojectRoot) {
  if (!p) return false;
  try {
    if (path.isAbsolute(p)) return fs.existsSync(p);
    if (subprojectRoot && fs.existsSync(path.resolve(subprojectRoot, p)))
      return true;
    return fs.existsSync(path.resolve(ROOT, p));
  } catch {
    return false;
  }
}

/**
 * Run factual checks on a single SKILL.md. Returns separate violations and warnings.
 * Returns gated=true when skill lacks the `<!-- mustard:generated -->` header.
 *
 * NO_CLUSTER policy: the skill name not matching a registry cluster suffix is a
 * structural mismatch, not necessarily invention. Many semantic skills (e.g.
 * `graphql-v15-canonical`, `approval-queue`) intentionally use names not derived
 * from any single file-suffix cluster. Only escalate to a violation when there
 * is no backing evidence at all (zero valid samples AND zero valid references) —
 * i.e. the name doesn't match a cluster AND nothing on disk supports it. With
 * any valid backing path, NO_CLUSTER is downgraded to a warning.
 *
 * @param {string} skillPath - absolute path to SKILL.md
 * @param {{ suffixes: Set<string> }} clusterIndex
 * @returns {{ gated: boolean, violations: Array<{ code: string, detail: string }>, warnings: Array<{ code: string, detail: string }> }}
 */
function factualCheckSkill(skillPath, clusterIndex, subprojectRoot) {
  const violations = [];
  const warnings = [];
  let content;
  try {
    content = fs.readFileSync(skillPath, "utf-8");
  } catch (err) {
    // Fail-open: unreadable file is not a factual violation.
    process.stderr.write(
      `[skill-validate] unreadable: ${skillPath}: ${err.message}\n`,
    );
    return { gated: true, violations, warnings };
  }

  // 1. Header gate
  if (!/<!--\s*mustard:generated/.test(content)) {
    return { gated: true, violations, warnings };
  }

  // 2. Existence gate — sample paths must exist. Track valid count for NO_CLUSTER demotion.
  const samples = extractSamplePaths(content);
  let validSampleCount = 0;
  for (const p of samples) {
    if (pathExistsUnderRoot(p, subprojectRoot)) {
      validSampleCount++;
    } else {
      violations.push({ code: "STALE_SAMPLE", detail: p });
    }
  }

  // 3. No-code gate — no fenced blocks allowed in SKILL.md body.
  const fenceCount = countFencedBlocks(content);
  if (fenceCount > 0) {
    violations.push({
      code: "CODE_IN_BODY",
      detail: `${fenceCount} fenced code block(s)`,
    });
  }

  // 4. References gate — Source: `{path}` entries in references/examples.md must exist.
  let validReferenceCount = 0;
  const refsFile = path.join(
    path.dirname(skillPath),
    "references",
    "examples.md",
  );
  if (fs.existsSync(refsFile)) {
    let refsContent;
    try {
      refsContent = fs.readFileSync(refsFile, "utf-8");
    } catch (err) {
      process.stderr.write(
        `[skill-validate] unreadable references: ${refsFile}: ${err.message}\n`,
      );
      refsContent = null;
    }
    if (refsContent) {
      const sources = extractReferenceSources(refsContent);
      for (const p of sources) {
        if (pathExistsUnderRoot(p, subprojectRoot)) {
          validReferenceCount++;
        } else {
          violations.push({ code: "STALE_REFERENCE", detail: p });
        }
      }
    }
  }

  // 5. Frequency gate — skill name must contain some cluster suffix as a token.
  // Skill naming convention per scan-format §10: `{subproject-short}-{suffix-slug}-pattern`.
  // Matching strips the trailing `-pattern` token (Mustard's own prescribed trailer) and
  // then tests every contiguous sub-sequence of the remaining tokens — so the cluster's
  // suffix (whatever the codebase itself shows) matches regardless of its position in the
  // skill name. Cluster suffixes come 100% from the user's registry; nothing is hardcoded.
  // Demoted to warning when the skill has at least one valid backing path (sample or reference).
  const name = extractSkillName(content);
  if (name) {
    const rawParts = name.split("-").filter(Boolean);
    // Remove trailing "pattern" token (convention suffix) before matching.
    const parts =
      rawParts[rawParts.length - 1] === "pattern"
        ? rawParts.slice(0, -1)
        : rawParts;

    if (parts.length >= 1) {
      let matched = false;

      // 5a. Try every contiguous sub-sequence of 1..N tokens (covers both single-word
      //     suffixes anywhere in the name AND multi-word suffixes like "view-model").
      outer: for (let len = parts.length; len >= 1; len--) {
        for (let start = 0; start <= parts.length - len; start++) {
          const candidate = parts.slice(start, start + len).join("-");
          if (clusterIndex.suffixes.has(candidate)) {
            matched = true;
            break outer;
          }
        }
      }

      // 5b. Fallback: full original name is also acceptable (no subproject prefix in registry).
      if (!matched && clusterIndex.suffixes.has(name)) matched = true;

      if (!matched && clusterIndex.suffixes.size > 0) {
        const hasBackingEvidence = validSampleCount + validReferenceCount > 0;
        const entry = {
          code: "NO_CLUSTER",
          detail: `skill name "${name}" does not match any _patterns[*].discovered[].suffix with fileCount >= 3`,
        };
        if (hasBackingEvidence) {
          warnings.push(entry);
        } else {
          violations.push(entry);
        }
      }
      // If clusterIndex is empty, we cannot enforce — skip rather than false-flag.
    }
  }

  return { gated: false, violations, warnings };
}

/** Run factual checks across all known skill directories. */
function runFactualMode() {
  // off mode: short-circuit before any filesystem iteration.
  if (FACTUAL_MODE === "off") {
    const payload = { mode: "off", total: 0, violations: [] };
    if (JSON_OUT) process.stdout.write(JSON.stringify(payload, null, 2) + "\n");
    else console.log("skill-validate (factual): mode=off, skipping.");
    process.exit(0);
  }

  let clusterIndex = { suffixes: new Set(), rawClusters: [] };
  try {
    clusterIndex = loadClusterIndex();
  } catch (err) {
    process.stderr.write(
      `[skill-validate] cluster index load failed: ${err.message}\n`,
    );
  }

  const locations = collectSkillDirs();
  const report = [];
  let total = 0;
  const violationsAll = [];
  const warningsAll = [];

  for (const { dir, label } of locations) {
    let files = [];
    try {
      files = collectSkills(dir);
    } catch (err) {
      process.stderr.write(
        `[skill-validate] collect failed for ${dir}: ${err.message}\n`,
      );
      continue;
    }
    // dir = {subprojectRoot}/.claude/skills — strip 2 levels to get subproject root
    const subprojectRoot = path.dirname(path.dirname(dir));
    for (const file of files) {
      total++;
      let result;
      try {
        result = factualCheckSkill(file, clusterIndex, subprojectRoot);
      } catch (err) {
        process.stderr.write(
          `[skill-validate] check failed for ${file}: ${err.message}\n`,
        );
        continue;
      }
      if (result.gated) continue;
      const rel = path.relative(ROOT, file).replace(/\\/g, "/");
      const skillName =
        extractSkillName(fsReadSafe(file)) || path.basename(path.dirname(file));
      for (const v of result.violations) {
        violationsAll.push({
          skill: skillName,
          file: rel,
          code: v.code,
          detail: v.detail,
          location: label,
        });
      }
      for (const w of result.warnings || []) {
        warningsAll.push({
          skill: skillName,
          file: rel,
          code: w.code,
          detail: w.detail,
          location: label,
        });
      }
      report.push({
        skill: skillName,
        file: rel,
        violations: result.violations.length,
        warnings: (result.warnings || []).length,
      });
    }
  }

  if (JSON_OUT) {
    const payload = {
      mode: FACTUAL_MODE,
      total,
      violations: violationsAll,
      warnings: warningsAll,
    };
    process.stdout.write(JSON.stringify(payload, null, 2) + "\n");
  } else {
    const groupBy = (items) => {
      const m = new Map();
      for (const it of items) {
        if (!m.has(it.file)) m.set(it.file, []);
        m.get(it.file).push(it);
      }
      return m;
    };

    if (violationsAll.length === 0 && warningsAll.length === 0) {
      console.log(
        `skill-validate (factual): ${total} skill(s) scanned, 0 violations. mode=${FACTUAL_MODE}`,
      );
    } else {
      console.log(
        `skill-validate (factual): ${violationsAll.length} violation(s), ${warningsAll.length} warning(s) across ${total} skill(s). mode=${FACTUAL_MODE}`,
      );
      if (violationsAll.length > 0) {
        for (const [file, vs] of groupBy(violationsAll)) {
          console.log(`  ${file}`);
          for (const v of vs) {
            console.log(`    [${v.code}] ${v.detail}`);
          }
        }
      }
      if (warningsAll.length > 0) {
        console.log(`  --- warnings (non-blocking) ---`);
        for (const [file, ws] of groupBy(warningsAll)) {
          console.log(`  ${file}`);
          for (const w of ws) {
            console.log(`    [WARN ${w.code}] ${w.detail}`);
          }
        }
      }
    }
  }

  if (FACTUAL_MODE === "warn") process.exit(0);
  // strict mode: exit 1 only if a real violation surfaced (warnings never block)
  process.exit(violationsAll.length > 0 ? 1 : 0);
}

function fsReadSafe(filePath) {
  try {
    return fs.readFileSync(filePath, "utf-8");
  } catch {
    return "";
  }
}

// ============================================================================
// LINES MODE (--lines flag)
// ============================================================================

/** Return tier string for a given line count. */
function linesTier(lineCount) {
  if (lineCount >= BLOCK_LINES) return "block";
  if (lineCount >= STRICT_WARN_LINES) return "strict-warn";
  if (lineCount >= WARN_LINES) return "warn";
  return "ok";
}

/** Run line-count check across all skill dirs. Called when --lines is present. */
function runLinesMode() {
  if (LINES_MODE === "off") {
    if (JSON_OUT) {
      process.stdout.write(
        JSON.stringify({ mode: "off", total: 0, results: [] }, null, 2) + "\n",
      );
    } else {
      console.log("skill-validate (lines): mode=off, skipping.");
    }
    process.exit(0);
  }

  const locations = collectSkillDirs();
  const results = [];
  let hasBlock = false;

  for (const { dir, label } of locations) {
    let files = [];
    try {
      files = collectSkills(dir);
    } catch (err) {
      process.stderr.write(
        `[skill-validate] collect failed for ${dir}: ${err.message}\n`,
      );
      continue;
    }
    for (const file of files) {
      const content = fsReadSafe(file);
      const lineCount = content ? content.split("\n").length : 0;
      const tier = linesTier(lineCount);
      if (tier === "block") hasBlock = true;
      const rel = path.relative(ROOT, file).replace(/\\/g, "/");
      results.push({ file: rel, lineCount, tier, location: label });
    }
  }

  if (JSON_OUT) {
    process.stdout.write(
      JSON.stringify(
        { mode: LINES_MODE, total: results.length, results },
        null,
        2,
      ) + "\n",
    );
  } else {
    const notable = results.filter((r) => r.tier !== "ok");
    if (notable.length === 0) {
      console.log(
        `skill-validate (lines): ${results.length} skill(s) scanned, all within thresholds. mode=${LINES_MODE}`,
      );
    } else {
      console.log(
        `skill-validate (lines): ${notable.length} skill(s) above threshold. mode=${LINES_MODE}`,
      );
      for (const r of notable) {
        console.log(
          `  [LINES] ${r.file}: ${r.lineCount} lines — tier=${r.tier}`,
        );
      }
    }
  }

  if (LINES_MODE === "strict" && hasBlock) process.exit(1);
  process.exit(0);
}

// ============================================================================
// STRUCTURAL MODE (original behavior)
// ============================================================================

function main() {
  if (FACTUAL) {
    runFactualMode();
    return;
  }
  if (LINES) {
    runLinesMode();
    return;
  }

  const locations = collectSkillDirs();
  const results = [];

  for (const { dir, label } of locations) {
    const files = collectSkills(dir);
    for (const file of files) {
      let content;
      try {
        content = fs.readFileSync(file, "utf-8");
      } catch {
        results.push({
          location: label,
          path: file,
          ok: false,
          errors: ["unreadable"],
          source: null,
        });
        continue;
      }
      const res = validateSkill(content);
      if (ONLY && res.source && res.source !== ONLY) continue;
      results.push({
        location: label,
        path: path.relative(ROOT, file).replace(/\\/g, "/"),
        ok: res.ok,
        errors: res.errors,
        source: res.source,
      });
    }
  }

  const failures = results.filter((r) => !r.ok);
  const summary = {
    total: results.length,
    ok: results.length - failures.length,
    failed: failures.length,
  };

  if (JSON_OUT) {
    process.stdout.write(JSON.stringify({ summary, results }, null, 2) + "\n");
  } else {
    const rowsToShow = QUIET ? failures : results;
    if (!rowsToShow.length) {
      console.log("skill-validate: no SKILL.md files found.");
    } else {
      console.log("skill-validate:");
      for (const r of rowsToShow) {
        const tag = r.ok ? "[ok]  " : "[fail]";
        const errs = r.errors.length ? ` — ${r.errors.join("; ")}` : "";
        console.log(`  ${tag} ${r.path}${errs}`);
      }
    }
    console.log(
      `\nskill-validate: ${summary.ok}/${summary.total} ok, ${summary.failed} failed.`,
    );
  }

  process.exit(failures.length > 0 ? 2 : 0);
}

// Fail-open wrapper keeps parent processes alive even on a crash.
if (require.main === module) {
  // Self-test mode: MUSTARD_SKILL_VALIDATE_SELFTEST=1
  // Regression test for the matching algorithm. Uses abstract token labels
  // (alpha/beta/multi-word) rather than real-world suffixes — the algorithm is
  // agnostic; any string a user's registry produces must match the same way.
  if (process.env.MUSTARD_SKILL_VALIDATE_SELFTEST === "1") {
    const testSuffixes = new Set([
      "alpha",
      "beta",
      "gamma",
      "delta",
      "compound-word",
    ]);
    const testIndex = { suffixes: testSuffixes, rawClusters: [] };

    // Helper: run the matching logic extracted from factualCheckSkill.
    function testMatch(skillName) {
      const rawParts = skillName.split("-").filter(Boolean);
      const parts =
        rawParts[rawParts.length - 1] === "pattern"
          ? rawParts.slice(0, -1)
          : rawParts;
      if (parts.length < 1) return false;
      for (let len = parts.length; len >= 1; len--) {
        for (let start = 0; start <= parts.length - len; start++) {
          const candidate = parts.slice(start, start + len).join("-");
          if (testIndex.suffixes.has(candidate)) return true;
        }
      }
      return testIndex.suffixes.has(skillName);
    }

    const cases = [
      { name: "sub-alpha-pattern", expect: true }, // single-token suffix mid-name
      { name: "sub-beta-pattern", expect: true }, // same, different suffix
      { name: "sub-compound-word-pattern", expect: true }, // multi-token suffix
      { name: "sub-unknown-pattern", expect: false }, // suffix not in cluster set
    ];

    let allPassed = true;
    for (const { name: skillName, expect } of cases) {
      const got = testMatch(skillName);
      const status = got === expect ? "PASS" : "FAIL";
      if (got !== expect) allPassed = false;
      const label = expect ? "MATCH" : "NO_CLUSTER";
      process.stdout.write(
        `[self-test] ${status}  ${skillName} → ${got ? "MATCH" : "NO_CLUSTER"} (expected: ${label})\n`,
      );
    }
    process.exit(allPassed ? 0 : 1);
  }

  try {
    main();
  } catch (err) {
    process.stderr.write(
      `[skill-validate] Fatal error: ${err.message}\n${err.stack}\n`,
    );
    process.exit(0);
  }
}

module.exports = {
  validateSkill,
  validateWithPython,
  collectSkills,
  collectSkillDirs,
  factualCheckSkill,
  loadClusterIndex,
  extractSamplePaths,
  extractReferenceSources,
  countFencedBlocks,
  slugify,
};
