#!/usr/bin/env node
// scripts/refactor-folder-per-component.mjs
//
// Wave 4 of spec 2026-05-23-dashboard-design-system.
//
// Refactors apps/dashboard/src/components/** from flat
// `dir/Name.tsx` into `dir/Name/index.tsx`, and moves the
// 8 domain dirs (specs, workspace, economy, knowledge,
// prd, telemetry, amend, trace) into a new `src/features/`
// namespace. 10 strays at the components/ root are
// relocated per the table in the spec.
//
// Usage:
//   node scripts/refactor-folder-per-component.mjs [--dry-run] [--verbose]
//
// Idempotent: re-running against an already-migrated tree
// must produce zero filesystem diff and zero rewrites.

import { execFileSync } from "node:child_process";
import {
  existsSync, mkdirSync, readdirSync, readFileSync,
  renameSync, rmdirSync, statSync, writeFileSync,
} from "node:fs";
import { dirname, join, posix, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const REPO_ROOT = resolve(dirname(__filename), "..");
const DASH_ROOT = join(REPO_ROOT, "apps", "dashboard");
const SRC_ROOT = join(DASH_ROOT, "src");
const COMPONENTS_ROOT = join(SRC_ROOT, "components");
const FEATURES_ROOT = join(SRC_ROOT, "features");

const args = new Set(process.argv.slice(2));
const DRY = args.has("--dry-run");
const VERBOSE = args.has("--verbose");

const DOMAIN_DIRS = new Set([
  "specs", "workspace", "economy", "knowledge",
  "prd", "telemetry", "amend", "trace",
]);
const SHARED_DIRS = new Set(["page", "layout", "ui"]);

// 10 root strays — explicit destination map per spec.
const STRAYS = {
  "AggregateOverview.tsx":  { feature: "workspace", domain: true,  name: "AggregateOverview" },
  "CommandPalette.tsx":     { feature: "layout",    domain: false, name: "CommandPalette" },
  "KnowledgeCard.tsx":      { feature: "knowledge", domain: true,  name: "KnowledgeCard" },
  "LivePipelineCard.tsx":   { feature: "workspace", domain: true,  name: "LivePipelineCard" },
  "Markdown.tsx":           { feature: "page",      domain: false, name: "Markdown" },
  "SpecSidePanel.tsx":      { feature: "specs",     domain: true,  name: "SpecSidePanel" },
  "SpecsList.tsx":          { feature: "specs",     domain: true,  name: "SpecsList" },
  "StatusDot.tsx":          { feature: "page",      domain: false, name: "StatusDot" },
  "WaveNav.tsx":            { feature: "specs",     domain: true,  name: "WaveNav" },
  "WorkspaceDigest.tsx":    { feature: "workspace", domain: true,  name: "WorkspaceDigest" },
};

// Helper modules in `components/specs/` shared by >1
// component — go to `features/specs/_shared/` per spec
// rule 4. (Underscore prefix means "not a component".)
const SHARED_HELPERS = {
  "spec-graph-layout.ts":   { feature: "specs" },
  "stage-from-status.ts":   { feature: "specs" },
  "spec-status.tsx":        { feature: "specs" },
};

// Phantom-token sweep — applied ONLY to files touched by the
// codemod (moved + barrel-written). Original Wave 3 surfacing.
const PHANTOM_RULES = [
  { re: /--color-ok\b/g,                replacement: "--intent-success" },
  { re: /--color-accent-mustard\b/g,    replacement: "--primary" },
  { re: /--color-error\b/g,             replacement: "--intent-error" },
  { re: /\btext-red-(?:400|500|600|700)\b/g, replacement: "text-[--intent-error]" },
  { re: /\bbg-red-(?:400|500|600|700)\b/g,   replacement: "bg-[--intent-error]/15" },
];

const moves = [];          // { from, to, kind, name, dir }
const importRewrites = []; // { file, count }
const phantomFixes = [];   // { file, count }
const log = (...a) => VERBOSE && console.log(...a);

function toPosix(p) { return p.split(sep).join("/"); }
function readFile(p) { return readFileSync(p, "utf8"); }
function writeFile(p, c) {
  if (!DRY) writeFileSync(p, c, "utf8");
}
function ensureDir(p) {
  if (!DRY) mkdirSync(p, { recursive: true });
}
function listEntries(p) {
  try { return readdirSync(p, { withFileTypes: true }); }
  catch { return []; }
}

// File-content equality check used in idempotency guard.
// For text files (.tsx/.ts/.md) we compare normalized
// content; binary files we compare bytes.
function filesEqual(a, b) {
  try {
    return readFileSync(a, "utf8") === readFileSync(b, "utf8");
  } catch { return false; }
}

// Try `git mv` (preserves history) then fall back to fs.rename
// for untracked files or non-git checkouts. Returns "git" or
// "fs" so the report can show how moves were performed.
function moveFile(from, to) {
  ensureDir(dirname(to));
  if (DRY) return "dry";
  try {
    execFileSync("git", ["mv", "-f", from, to], { cwd: REPO_ROOT, stdio: "pipe" });
    return "git";
  } catch {
    renameSync(from, to);
    return "fs";
  }
}

// =============================================================
// 1. DISCOVERY — build the move plan
// =============================================================

function planForDomainDir(dir) {
  // dir = "specs" | "workspace" | ...
  const srcDir = join(COMPONENTS_ROOT, dir);
  if (!existsSync(srcDir)) return;
  for (const e of listEntries(srcDir)) {
    if (e.isDirectory()) {
      if (e.name === "__tests__") {
        // Preserve __tests__/ at the feature root.
        const from = join(srcDir, "__tests__");
        const to = join(FEATURES_ROOT, dir, "__tests__");
        if (toPosix(from) !== toPosix(to)) {
          moves.push({ from, to, kind: "tests-dir", name: "__tests__", dir });
        }
      }
      continue;
    }
    if (!e.isFile()) continue;
    const isHelper = SHARED_HELPERS[e.name]?.feature === dir;
    if (isHelper) {
      const from = join(srcDir, e.name);
      const to = join(FEATURES_ROOT, dir, "_shared", e.name);
      moves.push({ from, to, kind: "helper", name: e.name, dir });
      continue;
    }
    if (!e.name.endsWith(".tsx")) continue;
    const compName = e.name.slice(0, -4);
    const from = join(srcDir, e.name);
    const to = join(FEATURES_ROOT, dir, compName, "index.tsx");
    moves.push({ from, to, kind: "component", name: compName, dir });
  }
}

function planForSharedDir(dir) {
  const srcDir = join(COMPONENTS_ROOT, dir);
  if (!existsSync(srcDir)) return;
  for (const e of listEntries(srcDir)) {
    if (!e.isFile()) continue;
    if (!e.name.endsWith(".tsx")) continue;
    if (e.name === "index.tsx") continue; // never happens; safety
    const compName = e.name.slice(0, -4);
    const from = join(srcDir, e.name);
    const to = join(srcDir, compName, "index.tsx");
    moves.push({ from, to, kind: "component", name: compName, dir });
  }
}

function planForStrays() {
  for (const e of listEntries(COMPONENTS_ROOT)) {
    if (!e.isFile()) continue;
    const map = STRAYS[e.name];
    if (!map) continue;
    const from = join(COMPONENTS_ROOT, e.name);
    const to = map.domain
      ? join(FEATURES_ROOT, map.feature, map.name, "index.tsx")
      : join(COMPONENTS_ROOT, map.feature, map.name, "index.tsx");
    moves.push({ from, to, kind: "stray", name: map.name, dir: map.feature });
  }
}

for (const d of DOMAIN_DIRS) planForDomainDir(d);
for (const d of SHARED_DIRS) planForSharedDir(d);
planForStrays();

// =============================================================
// 2. VALIDATE — destinations must not exist unless identical
// =============================================================

for (const mv of moves) {
  if (!existsSync(mv.from)) {
    // Source disappeared (already moved). Skip silently —
    // idempotency.
    mv.skip = true;
    continue;
  }
  if (existsSync(mv.to)) {
    if (statSync(mv.to).isDirectory()) {
      // Destination directory pre-exists (e.g. __tests__ move
      // when the feature dir was already created). For dir
      // moves: only skip if the directory is fully populated;
      // otherwise fall through and the inner files will be
      // moved individually. We only plan dir moves for
      // __tests__, which is atomic, so flag a skip when the
      // tests already live at the destination.
      if (mv.kind === "tests-dir") {
        mv.skip = filesEqual(
          join(mv.from, listEntries(mv.from)[0]?.name ?? ""),
          join(mv.to, listEntries(mv.to)[0]?.name ?? ""),
        );
      }
      continue;
    }
    if (filesEqual(mv.from, mv.to)) {
      mv.skip = true;
      continue;
    }
    console.error(
      `[refactor] destination exists and differs: ${toPosix(mv.to)}\n` +
      `  source: ${toPosix(mv.from)}`,
    );
    process.exit(2);
  }
}

// =============================================================
// 3. MOVE — git mv (history) with fs fallback
// =============================================================

let movedCount = 0;
for (const mv of moves) {
  if (mv.skip) { log("[skip]", toPosix(mv.from)); continue; }
  const how = moveFile(mv.from, mv.to);
  movedCount++;
  log(`[mv ${how}]`, toPosix(mv.from), "->", toPosix(mv.to));
}

// =============================================================
// 4. REWRITE IMPORTS — across all of src/
// =============================================================

// Build a map of legacy specifier -> new specifier.
//
// (a) Subdir imports like "@/components/specs/SpecCard"
//     -> "@/features/specs/SpecCard" (for the 8 domain dirs).
//     Shared dirs (page/layout/ui) keep "@/components/{dir}/{X}".
//     In both cases, X is now a folder; TypeScript resolves
//     X -> X/index.tsx automatically.
//
// (b) Root strays like "@/components/StatusDot"
//     -> destination per STRAYS table.
//
// (c) Helper specifiers "@/components/specs/spec-status"
//     -> "@/features/specs/_shared/spec-status".
//
// We rewrite by regex on the import source string. Both
// `import ... from "X"` and `... from "X";` forms are
// covered by matching the quoted specifier alone.

const SUBDIR_REWRITES = []; // { re, replacement }
for (const d of DOMAIN_DIRS) {
  // Helpers first (more specific) — must precede generic subdir rule.
  for (const [name, info] of Object.entries(SHARED_HELPERS)) {
    if (info.feature !== d) continue;
    const stem = name.replace(/\.(tsx?|jsx?)$/, "");
    SUBDIR_REWRITES.push({
      re: new RegExp(`@/components/${d}/${stem}(?=['"])`, "g"),
      replacement: `@/features/${d}/_shared/${stem}`,
    });
  }
  SUBDIR_REWRITES.push({
    re: new RegExp(`@/components/${d}/`, "g"),
    replacement: `@/features/${d}/`,
  });
}
// Strays (root) — exact-specifier match, not prefix.
for (const [file, info] of Object.entries(STRAYS)) {
  const stem = file.replace(/\.tsx$/, "");
  const dest = info.domain
    ? `@/features/${info.feature}/${info.name}`
    : `@/components/${info.feature}/${info.name}`;
  SUBDIR_REWRITES.push({
    re: new RegExp(`@/components/${stem}(?=['"])`, "g"),
    replacement: dest,
  });
}

// Sibling import depth-1 -> depth-2 adjustment INSIDE moved
// components. When `dir/Name.tsx` becomes `dir/Name/index.tsx`,
// any `from "./Sibling"` inside it now needs `from "../Sibling"`.
//
// Applied only to files whose post-move path ends with
// `/index.tsx` AND whose source originated as `dir/Name.tsx`.
const siblingFixTargets = new Set(
  moves
    .filter(mv => mv.kind === "component" || mv.kind === "stray")
    .map(mv => toPosix(mv.to)),
);

function rewriteSiblings(src) {
  // `from "./Sibling"` (with optional extension) -> `from "../Sibling"`
  let count = 0;
  const out = src.replace(
    /from\s+(['"])\.\/([^'"]+)\1/g,
    (m, q, spec) => { count++; return `from ${q}../${spec}${q}`; },
  );
  return { out, count };
}

function applyPhantomSweep(src) {
  let total = 0;
  let out = src;
  for (const rule of PHANTOM_RULES) {
    out = out.replace(rule.re, (m) => { total++; return rule.replacement; });
  }
  return { out, count: total };
}

function rewriteSubdirImports(src) {
  let total = 0;
  let out = src;
  for (const rule of SUBDIR_REWRITES) {
    out = out.replace(rule.re, (m) => { total++; return rule.replacement; });
  }
  return { out, count: total };
}

// Walk every .ts / .tsx under src/ and rewrite.
function walkSrc(root) {
  const out = [];
  for (const e of listEntries(root)) {
    if (e.name === "node_modules" || e.name === "__tests__") {
      // __tests__ kept but still scanned for imports.
    }
    const full = join(root, e.name);
    if (e.isDirectory()) out.push(...walkSrc(full));
    else if (e.isFile() && /\.(tsx?|jsx?)$/.test(e.name)) out.push(full);
  }
  return out;
}

const srcFiles = existsSync(SRC_ROOT) ? walkSrc(SRC_ROOT) : [];
for (const file of srcFiles) {
  if (DRY && !existsSync(file)) continue;
  let content;
  try { content = readFile(file); } catch { continue; }
  let changed = 0;

  // Subdir rewrites: "@/components/specs/X" -> "@/features/specs/X" etc.
  const subdir = rewriteSubdirImports(content);
  content = subdir.out;
  changed += subdir.count;

  // Sibling depth fix: only on files whose canonical path is
  // a moved component's new `Name/index.tsx`.
  if (siblingFixTargets.has(toPosix(file))) {
    const sib = rewriteSiblings(content);
    content = sib.out;
    changed += sib.count;
  }

  // Phantom token sweep — only on files inside features/ or
  // components/{page,layout,ui}/ that we just touched. Pages,
  // hooks, lib, styles are out of scope for this wave.
  const inScope =
    toPosix(file).startsWith(toPosix(FEATURES_ROOT) + "/") ||
    SHARED_DIRS.size && [...SHARED_DIRS].some(
      d => toPosix(file).startsWith(toPosix(join(COMPONENTS_ROOT, d)) + "/"),
    );
  let phantomCount = 0;
  if (inScope) {
    const ph = applyPhantomSweep(content);
    if (ph.count) {
      content = ph.out;
      phantomCount = ph.count;
      phantomFixes.push({ file: toPosix(file), count: ph.count });
    }
  }

  if (changed > 0 || phantomCount > 0) {
    writeFile(file, content);
    if (changed > 0) importRewrites.push({ file: toPosix(file), count: changed });
  }
}

// =============================================================
// 5. BARREL EMIT — one index.ts per features/{d} and
//    components/{page,layout,ui}/ that re-exports every
//    Component sub-folder. The existing components/page/index.ts
//    is replaced (it carried curated typed exports; this barrel
//    is the new contract per spec).
// =============================================================

function listComponentFoldersIn(root) {
  if (!existsSync(root)) return [];
  return listEntries(root)
    .filter(e =>
      e.isDirectory() &&
      !e.name.startsWith("_") &&
      e.name !== "__tests__",
    )
    .map(e => e.name)
    .sort();
}

function writeBarrel(root) {
  if (!existsSync(root)) return;
  const folders = listComponentFoldersIn(root);
  if (folders.length === 0) return;
  const lines = [
    "// AUTO-GENERATED by scripts/refactor-folder-per-component.mjs",
    "// (Wave 4 of spec 2026-05-23-dashboard-design-system).",
    "// Re-exports every component in this folder so callers can",
    "// import either granular or aggregated.",
    "",
    ...folders.map(name => `export * from "./${name}";`),
    "",
  ];
  const dest = join(root, "index.ts");
  const next = lines.join("\n");
  if (existsSync(dest) && readFile(dest) === next) return; // idempotent
  writeFile(dest, next);
}

for (const d of DOMAIN_DIRS) writeBarrel(join(FEATURES_ROOT, d));
for (const d of SHARED_DIRS) writeBarrel(join(COMPONENTS_ROOT, d));

// =============================================================
// 6. CLEANUP — remove empty domain dirs under components/.
// =============================================================

for (const d of DOMAIN_DIRS) {
  const p = join(COMPONENTS_ROOT, d);
  if (!existsSync(p)) continue;
  const remaining = listEntries(p);
  if (remaining.length === 0) {
    if (!DRY) rmdirSync(p);
    log("[rmdir]", toPosix(p));
  }
}

// =============================================================
// 7. REPORT
// =============================================================

console.log(`[refactor] ${DRY ? "(dry-run) " : ""}moved=${movedCount} ` +
            `import-rewrites=${importRewrites.length} ` +
            `phantom-tokens-fixed=${phantomFixes.reduce((s, p) => s + p.count, 0)}`);
if (VERBOSE) {
  for (const r of importRewrites) console.log(`  rw ${r.count}x ${r.file}`);
  for (const p of phantomFixes) console.log(`  ph ${p.count}x ${p.file}`);
}
