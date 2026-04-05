#!/usr/bin/env node

/**
 * sync-detect.js
 *
 * Detects subprojects automatically by reading git submodule status
 * (or scanning for folders with CLAUDE.md as fallback).
 *
 * For each subproject:
 *   - Detects role via file-based scoring (config files, deps, directories)
 *   - Maps role → agent
 *   - Scans .claude/commands/*.md for available commands
 *
 * Also reads .claude/prompts/*.md files to build the agents list.
 *
 * Outputs JSON to stdout.
 */

const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { execSync } = require("child_process");

// Root of the monorepo (parent of .claude/scripts/)
const ROOT = path.resolve(__dirname, "..", "..");

// Cache TTL for early-exit gate (5 minutes)
const CACHE_TTL_MS = 5 * 60 * 1000;

// Manifest file globs passed to `git status --porcelain` for dirty check.
// If none of these changed since the last cache write, the cache is safe to reuse.
const MANIFEST_GLOBS = [
  "package.json",
  "**/*.csproj",
  "**/pubspec.yaml",
  "**/schema.prisma",
  "**/drizzle.config.*",
  "**/go.mod",
  "**/Cargo.toml",
  "**/pyproject.toml",
];

// ---------------------------------------------------------------------------
// Stream-based file hashing
// ---------------------------------------------------------------------------

/**
 * Hash a file in 64KB chunks, updating `hash` in-place.
 * Avoids loading entire files into memory for large source trees.
 */
function hashFileStream(filePath, hash) {
  const buffer = Buffer.alloc(65536);
  const fd = fs.openSync(filePath, "r");
  try {
    let bytesRead;
    while ((bytesRead = fs.readSync(fd, buffer, 0, buffer.length, null)) > 0) {
      hash.update(buffer.subarray(0, bytesRead));
    }
  } finally {
    fs.closeSync(fd);
  }
}

// ---------------------------------------------------------------------------
// Memoized collectSourceFiles cache
// ---------------------------------------------------------------------------

const _collectCache = new Map();

function clearCollectCache() {
  _collectCache.clear();
}

// ---------------------------------------------------------------------------
// Role detection: file-based scoring
// ---------------------------------------------------------------------------

const ROLE_WEIGHTS = { HIGH: 10, MEDIUM: 5, LOW: 3 };

/**
 * Check if a file/glob pattern exists in a directory.
 * Supports simple glob: "*.csproj", "next.config.*", etc.
 */
function fileExists(dir, pattern) {
  if (!pattern.includes("*")) {
    return fs.existsSync(path.join(dir, pattern));
  }
  // Simple glob matching
  const parts = pattern.split("*");
  try {
    const entries = fs.readdirSync(dir);
    return entries.some((entry) => {
      if (parts.length === 2) {
        return entry.startsWith(parts[0]) && entry.endsWith(parts[1]);
      }
      return false;
    });
  } catch {
    return false;
  }
}

/**
 * Check if a directory exists inside the subproject (searches up to 2 levels deep).
 */
function dirExists(base, dirName) {
  // Check direct path first
  try {
    const fullPath = path.join(base, dirName);
    if (fs.existsSync(fullPath) && fs.statSync(fullPath).isDirectory()) {
      return true;
    }
  } catch {
    // continue searching
  }
  // Check one level deeper (e.g., MyApp.Backend/MyApp.Backend/Modules/)
  try {
    const entries = fs.readdirSync(base, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      if (entry.name.startsWith(".") || entry.name === "node_modules" || entry.name === "bin" || entry.name === "obj") continue;
      const nestedPath = path.join(base, entry.name, dirName);
      try {
        if (fs.existsSync(nestedPath) && fs.statSync(nestedPath).isDirectory()) {
          return true;
        }
      } catch {
        // continue
      }
    }
  } catch {
    // ignore
  }
  return false;
}

/**
 * Read a file if it exists, return content or empty string.
 */
function readFileSafe(filePath) {
  try {
    return fs.readFileSync(filePath, "utf-8");
  } catch {
    return "";
  }
}

/**
 * Find all .csproj files recursively up to maxDepth levels.
 */
function findCsprojFiles(dir, maxDepth = 2, currentDepth = 0) {
  const results = [];
  if (currentDepth > maxDepth) return results;
  try {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.isFile() && entry.name.endsWith(".csproj")) {
        results.push(path.join(dir, entry.name));
      } else if (entry.isDirectory() && !entry.name.startsWith(".") && entry.name !== "node_modules" && entry.name !== "bin" && entry.name !== "obj") {
        results.push(...findCsprojFiles(path.join(dir, entry.name), maxDepth, currentDepth + 1));
      }
    }
  } catch {
    // ignore
  }
  return results;
}

/**
 * Check if any .csproj file has Sdk="Microsoft.NET.Sdk.Web" (web project).
 * Searches recursively up to 2 levels deep.
 */
function isCsprojWeb(dir) {
  const csprojFiles = findCsprojFiles(dir);
  for (const filePath of csprojFiles) {
    const content = readFileSafe(filePath);
    if (/Sdk\s*=\s*"Microsoft\.NET\.Sdk\.Web"/i.test(content)) {
      return true;
    }
  }
  return false;
}

/**
 * Check if any .csproj file exists but is NOT a web project.
 * Searches recursively up to 2 levels deep.
 */
function isCsprojLibrary(dir) {
  const csprojFiles = findCsprojFiles(dir);
  for (const filePath of csprojFiles) {
    const content = readFileSafe(filePath);
    if (!/Sdk\s*=\s*"Microsoft\.NET\.Sdk\.Web"/i.test(content)) {
      return true;
    }
  }
  return false;
}

/**
 * Read package.json dependencies (both deps and devDeps).
 */
function getPackageJsonDeps(dir) {
  const pkgPath = path.join(dir, "package.json");
  const content = readFileSafe(pkgPath);
  if (!content) return [];
  try {
    const pkg = JSON.parse(content);
    return [
      ...Object.keys(pkg.dependencies || {}),
      ...Object.keys(pkg.devDependencies || {}),
    ];
  } catch {
    return [];
  }
}

/**
 * Check if any dependency matches a list of patterns.
 */
function hasAnyDep(deps, patterns) {
  return deps.some((dep) =>
    patterns.some((p) => dep === p || dep.startsWith(p + "/") || dep.startsWith("@" + p + "/") || dep === "@" + p)
  );
}

/**
 * Read go.mod content and check for specific imports.
 */
function goModHas(dir, patterns) {
  const content = readFileSafe(path.join(dir, "go.mod"));
  if (!content) return false;
  return patterns.some((p) => content.includes(p));
}

/**
 * Read pyproject.toml content and check for specific deps.
 */
function pyprojectHas(dir, patterns) {
  const content = readFileSafe(path.join(dir, "pyproject.toml"));
  if (!content) return false;
  return patterns.some((p) => content.includes(p));
}

/**
 * Read Cargo.toml content and check for specific deps.
 */
function cargoHas(dir, patterns) {
  const content = readFileSafe(path.join(dir, "Cargo.toml"));
  if (!content) return false;
  return patterns.some((p) => content.includes(p));
}

/**
 * Detect the role of a subproject using file-based scoring.
 * Returns { role, scores }.
 */
function detectRole(absPath) {
  const scores = { api: 0, ui: 0, database: 0, library: 0, mobile: 0 };

  // --- Config files (HIGH weight = 10) ---

  // .csproj with Sdk.Web → api
  if (isCsprojWeb(absPath)) scores.api += ROLE_WEIGHTS.HIGH;

  // next.config.* | vite.config.* → ui
  if (fileExists(absPath, "next.config.*") || fileExists(absPath, "vite.config.*")) {
    scores.ui += ROLE_WEIGHTS.HIGH;
  }

  // drizzle.config.* | prisma/ → database
  if (fileExists(absPath, "drizzle.config.*") || dirExists(absPath, "prisma")) {
    scores.database += ROLE_WEIGHTS.HIGH;
  }

  // .csproj WITHOUT Sdk.Web → library
  if (isCsprojLibrary(absPath) && !isCsprojWeb(absPath)) {
    scores.library += ROLE_WEIGHTS.HIGH;
  }

  // pubspec.yaml → mobile (Flutter/Dart)
  if (fileExists(absPath, "pubspec.yaml")) {
    scores.mobile += ROLE_WEIGHTS.HIGH;
  }

  // --- package.json deps (MEDIUM weight = 5) ---

  const deps = getPackageJsonDeps(absPath);
  if (deps.length > 0) {
    // API frameworks
    if (hasAnyDep(deps, ["express", "fastify", "hono", "koa", "nestjs"])) {
      scores.api += ROLE_WEIGHTS.MEDIUM;
    }
    // UI frameworks
    if (hasAnyDep(deps, ["react", "next", "vue", "nuxt", "svelte", "angular"])) {
      scores.ui += ROLE_WEIGHTS.MEDIUM;
    }
    // Database ORMs
    if (hasAnyDep(deps, ["drizzle-orm", "prisma", "typeorm", "knex", "sequelize"])) {
      scores.database += ROLE_WEIGHTS.MEDIUM;
    }
  }

  // --- pubspec.yaml deps (MEDIUM weight = 5) ---

  const pubspecContent = readFileSafe(path.join(absPath, "pubspec.yaml"));
  if (pubspecContent) {
    if (/flutter:/.test(pubspecContent)) scores.mobile += ROLE_WEIGHTS.MEDIUM;
  }

  // --- go.mod / pyproject.toml / Cargo.toml (HIGH weight = 10) ---

  if (fileExists(absPath, "go.mod")) {
    if (goModHas(absPath, ["net/http", "gin", "echo", "fiber"])) {
      scores.api += ROLE_WEIGHTS.HIGH;
    }
  }

  if (fileExists(absPath, "pyproject.toml")) {
    if (pyprojectHas(absPath, ["fastapi", "django", "flask", "starlette"])) {
      scores.api += ROLE_WEIGHTS.HIGH;
    }
  }

  if (fileExists(absPath, "Cargo.toml")) {
    if (cargoHas(absPath, ["actix", "axum", "rocket", "warp"])) {
      scores.api += ROLE_WEIGHTS.HIGH;
    }
  }

  // --- Directories (LOW weight = 3) ---

  // API directories
  if (
    dirExists(absPath, "Controllers") ||
    dirExists(absPath, "Modules") ||
    dirExists(absPath, "routes")
  ) {
    scores.api += ROLE_WEIGHTS.LOW;
  }

  // UI directories (app/ + components/)
  if (dirExists(absPath, "app") && dirExists(absPath, "components")) {
    scores.ui += ROLE_WEIGHTS.LOW;
  }
  // Also check src/app + src/components
  if (dirExists(absPath, path.join("src", "app")) && dirExists(absPath, path.join("src", "components"))) {
    scores.ui += ROLE_WEIGHTS.LOW;
  }

  // Database directories
  if (dirExists(absPath, "migrations") || dirExists(absPath, "schema")) {
    scores.database += ROLE_WEIGHTS.LOW;
  }
  if (dirExists(absPath, path.join("src", "migrations")) || dirExists(absPath, path.join("src", "schema"))) {
    scores.database += ROLE_WEIGHTS.LOW;
  }

  // Mobile directories (Flutter)
  if (dirExists(absPath, "lib") && (dirExists(absPath, "android") || dirExists(absPath, "ios"))) {
    scores.mobile += ROLE_WEIGHTS.LOW;
  }

  // Determine role from highest score
  let maxScore = 0;
  let role = "general";
  for (const [r, s] of Object.entries(scores)) {
    if (s > maxScore) {
      maxScore = s;
      role = r;
    }
  }

  return { role, scores };
}

// ---------------------------------------------------------------------------
// Stack summary: compact string with main deps + versions
// ---------------------------------------------------------------------------

/**
 * Read package manifest and return a compact stack summary string.
 * E.g. ".NET 10, Minimal APIs, FluentValidation 12.0" or "React 19, Next.js 16, Tailwind 4.1"
 */
function getStackSummary(absPath) {
  const parts = [];

  // Try .csproj files
  const csprojFiles = findCsprojFiles(absPath);
  if (csprojFiles.length > 0) {
    const csprojContent = csprojFiles.map((f) => readFileSafe(f)).join("\n");
    // Target framework
    const tfm = csprojContent.match(/<TargetFramework>(net\d+\.?\d*)<\/TargetFramework>/);
    if (tfm) {
      const ver = tfm[1].replace("net", ".NET ");
      parts.push(ver);
    }
    // Sdk type
    if (/Sdk\s*=\s*"Microsoft\.NET\.Sdk\.Web"/i.test(csprojContent)) {
      parts.push("Web API");
    }
    // Key PackageReferences (top 5 by relevance)
    const pkgRefs = [];
    const pkgRegex = /<PackageReference\s+Include="([^"]+)"(?:\s+Version="([^"]+)")?/g;
    let match;
    while ((match = pkgRegex.exec(csprojContent)) !== null) {
      const name = match[1].split(".").pop(); // short name
      const ver = match[2] ? ` ${match[2].split(".").slice(0, 2).join(".")}` : "";
      pkgRefs.push(`${name}${ver}`);
    }
    if (pkgRefs.length > 0) {
      parts.push(...pkgRefs.slice(0, 5));
    }
  }

  // Try package.json
  const pkgPath = path.join(absPath, "package.json");
  const pkgContent = readFileSafe(pkgPath);
  if (pkgContent) {
    try {
      const pkg = JSON.parse(pkgContent);
      const deps = { ...pkg.dependencies, ...pkg.devDependencies };
      const interesting = [
        "react", "next", "vue", "nuxt", "svelte", "angular",
        "express", "fastify", "hono", "tailwindcss",
        "drizzle-orm", "prisma", "typeorm",
        "typescript",
      ];
      for (const dep of interesting) {
        if (deps[dep]) {
          const name = dep === "tailwindcss" ? "Tailwind" : dep.charAt(0).toUpperCase() + dep.slice(1);
          const ver = deps[dep].replace(/[\^~>=<]*/g, "").split(".").slice(0, 2).join(".");
          parts.push(`${name} ${ver}`);
        }
      }
    } catch {
      // ignore parse errors
    }
  }

  // Try drizzle.config.*
  if (fileExists(absPath, "drizzle.config.*")) {
    if (!parts.some((p) => p.toLowerCase().includes("drizzle"))) {
      parts.push("Drizzle ORM");
    }
  }

  // Try pubspec.yaml (Flutter/Dart)
  const pubspec = readFileSafe(path.join(absPath, "pubspec.yaml"));
  if (pubspec) {
    // Flutter SDK version
    const flutterSdk = pubspec.match(/flutter:\s*["']?>=?([\d.]+)/);
    if (flutterSdk) parts.push(`Flutter ${flutterSdk[1]}`);
    else if (/flutter:/.test(pubspec)) parts.push("Flutter");

    // Dart SDK version
    const dartSdk = pubspec.match(/sdk:\s*["']?>=?([\d.]+)/);
    if (dartSdk) parts.push(`Dart ${dartSdk[1]}`);

    // Key dependencies
    const flutterDeps = ["riverpod", "bloc", "provider", "dio", "go_router", "freezed", "hive", "drift", "get_it"];
    for (const dep of flutterDeps) {
      if (pubspec.includes(dep)) {
        parts.push(dep.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase()));
      }
    }
  }

  return parts.length > 0 ? parts.join(", ") : "";
}

// ---------------------------------------------------------------------------
// Role → Agent mapping
// ---------------------------------------------------------------------------

const ROLE_AGENT_MAP = {
  api: "backend",
  ui: "frontend",
  database: "database",
  library: "backend", // libraries merge into backend agent
  mobile: "mobile",
  general: "general",
};

function roleToAgent(role) {
  return ROLE_AGENT_MAP[role] || "general";
}

// ---------------------------------------------------------------------------
// Subproject discovery
// ---------------------------------------------------------------------------

/**
 * Try to get subproject paths from `git submodule status`.
 * Returns an array of relative paths (e.g. ["my-api", "my-frontend", ...]) or null on failure.
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
// Agents discovery (from .claude/prompts/*.md files)
// ---------------------------------------------------------------------------

/**
 * Reads .claude/prompts/*.md files. Each file (except _index.md and _templates/)
 * defines an agent. Always includes "orchestrator".
 */
function getAgents() {
  const promptsDir = path.join(ROOT, ".claude", "prompts");
  const agents = new Set(["orchestrator"]);

  try {
    if (fs.existsSync(promptsDir)) {
      const files = fs.readdirSync(promptsDir);
      for (const f of files) {
        if (f.endsWith(".md") && f !== "_index.md" && !f.startsWith("_")) {
          agents.add(f.replace(".md", ""));
        }
      }
    }
  } catch {
    // ignore
  }

  return Array.from(agents).sort();
}

// ---------------------------------------------------------------------------
// Git dirty detection
// ---------------------------------------------------------------------------

/**
 * Check if a subproject has uncommitted changes (modified, untracked, staged)
 * using `git status --porcelain`. Returns { dirty: boolean, files: string[] }.
 * Only considers source files (matching SOURCE_EXTENSIONS).
 */
function getGitDirtyFiles(subprojectPath) {
  try {
    const output = execSync(`git status --porcelain -- "${subprojectPath}"`, {
      cwd: ROOT,
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    });
    const sourceExts = new Set([".cs", ".ts", ".tsx", ".js", ".jsx", ".dart"]);
    const ignoreNames = new Set(["node_modules", ".next", "bin", "obj", "dist", "_backup"]);
    const files = [];
    for (const line of output.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      // Format: "XY filename" or "XY filename -> newname"
      const filePath = trimmed.substring(3).split(" -> ").pop().trim();
      const fileName = path.basename(filePath);
      const ext = path.extname(filePath).toLowerCase();
      if (!sourceExts.has(ext) && !MANIFEST_FILES.has(fileName)) continue;
      // Skip ignored directories
      const parts = filePath.split("/");
      if (parts.some((p) => ignoreNames.has(p) || p === "migrations")) continue;
      files.push(filePath);
    }
    return { dirty: files.length > 0, files };
  } catch {
    return { dirty: false, files: [] };
  }
}

// ---------------------------------------------------------------------------
// Source hash computation for scan incremental
// ---------------------------------------------------------------------------

const SOURCE_IGNORE_PATTERNS = [
  "**/node_modules/**",
  "**/.next/**",
  "**/bin/**",
  "**/obj/**",
  "**/dist/**",
  "**/migrations/**",
  "**/_backup/**",
  "**/.git/**",
];

const SOURCE_EXTENSIONS = new Set([".cs", ".ts", ".tsx", ".js", ".jsx", ".dart"]);

/**
 * Manifest files that affect project behavior without changing source code.
 * Changes to these files (dependency upgrades, SDK bumps) should invalidate
 * the source hash even when no source file changed.
 */
const MANIFEST_FILES = new Set([
  // Flutter/Dart
  "pubspec.yaml", "pubspec.lock",
  // Node.js
  "package.json", "pnpm-lock.yaml", "package-lock.json", "yarn.lock",
  // .NET
  "Directory.Packages.props", "Directory.Build.props", "nuget.config",
  // Go
  "go.mod", "go.sum",
  // Rust
  "Cargo.toml", "Cargo.lock",
  // Python
  "pyproject.toml", "requirements.txt", "poetry.lock",
]);

/**
 * Recursively collect source files from a directory.
 * Respects ignore patterns and extension filters.
 */
function collectSourceFiles(dir, maxDepth = 10, currentDepth = 0) {
  if (currentDepth === 0) {
    const cached = _collectCache.get(dir);
    if (cached) return cached.slice();
  }
  const results = [];
  if (currentDepth > maxDepth) return results;
  try {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);
      const relFromRoot = path.relative(ROOT, fullPath).replace(/\\/g, "/");

      // Check ignore patterns
      if (
        entry.name === "node_modules" ||
        entry.name === ".next" ||
        entry.name === "bin" ||
        entry.name === "obj" ||
        entry.name === "dist" ||
        entry.name === "migrations" ||
        entry.name === "_backup" ||
        entry.name.startsWith(".")
      ) continue;

      if (entry.isDirectory()) {
        results.push(...collectSourceFiles(fullPath, maxDepth, currentDepth + 1));
      } else if (entry.isFile()) {
        const ext = path.extname(entry.name).toLowerCase();
        if (SOURCE_EXTENSIONS.has(ext) || MANIFEST_FILES.has(entry.name)) {
          results.push(relFromRoot);
        }
      }
    }
  } catch {
    // ignore unreadable dirs
  }
  if (currentDepth === 0) {
    _collectCache.set(dir, results.slice());
  }
  return results;
}

/**
 * Compute a SHA-256 hash of all source files in a subproject.
 * Files are sorted for deterministic output.
 */
function computeSourceHash(subprojectPath) {
  const absPath = path.join(ROOT, subprojectPath);
  const files = collectSourceFiles(absPath);
  files.sort();

  const hash = crypto.createHash("sha256");
  for (const file of files) {
    try {
      hash.update(file); // include path for sensitivity to renames
      hashFileStream(path.join(ROOT, file), hash);
    } catch {
      // skip unreadable
    }
  }
  return hash.digest("hex");
}

/**
 * Compute module-level hashes for backend modules (Modules/v1/{Module}/)
 * and frontend entity dirs (app/(dashboard)/{entity}/).
 */
function computeModuleHashes(subprojectPath, role) {
  const absPath = path.join(ROOT, subprojectPath);
  const modules = {};

  if (role === "api" || role === "library") {
    // Backend: find ALL Modules/ dirs across project layers (Application, Backend, etc.)
    // Then drill into versioned subdirs (v1/, v2/) to find actual domain modules
    const allModuleDirs = findAllModulesDirs(absPath);
    const moduleFiles = {}; // { ModuleName: [file1, file2, ...] }
    for (const modulesDir of allModuleDirs) {
      try {
        const versionEntries = fs.readdirSync(modulesDir, { withFileTypes: true });
        for (const vEntry of versionEntries) {
          if (!vEntry.isDirectory()) continue;
          // Check if this is a version dir (v1, v2) or a direct module dir
          const vPath = path.join(modulesDir, vEntry.name);
          const isVersionDir = /^v\d+$/i.test(vEntry.name);
          if (isVersionDir) {
            // Drill into version dir → list domain modules
            const domainEntries = fs.readdirSync(vPath, { withFileTypes: true });
            for (const dEntry of domainEntries) {
              if (!dEntry.isDirectory()) continue;
              const domainPath = path.join(vPath, dEntry.name);
              const files = collectSourceFiles(domainPath);
              if (files.length === 0) continue;
              if (!moduleFiles[dEntry.name]) moduleFiles[dEntry.name] = [];
              moduleFiles[dEntry.name].push(...files);
            }
          } else {
            // Direct module dir (no version prefix)
            const files = collectSourceFiles(vPath);
            if (files.length === 0) continue;
            if (!moduleFiles[vEntry.name]) moduleFiles[vEntry.name] = [];
            moduleFiles[vEntry.name].push(...files);
          }
        }
      } catch {}
    }
    // Also scan Infra/, Seeds/, and other cross-cutting dirs as "_infra" module
    const crossCuttingDirs = ["Infra", "Seeds", "Shared", "Common"];
    for (const ccDir of crossCuttingDirs) {
      const ccPaths = findDirsNamed(absPath, ccDir, 2);
      for (const ccPath of ccPaths) {
        const files = collectSourceFiles(ccPath);
        if (files.length === 0) continue;
        if (!moduleFiles["_infra"]) moduleFiles["_infra"] = [];
        moduleFiles["_infra"].push(...files);
      }
    }
    // Compute hash per module
    for (const [modName, files] of Object.entries(moduleFiles)) {
      files.sort();
      const hash = crypto.createHash("sha256");
      for (const f of files) {
        try {
          hash.update(f);
          hashFileStream(path.join(ROOT, f), hash);
        } catch {}
      }
      modules[modName] = {
        hash: hash.digest("hex"),
        files: files.length,
      };
    }
  } else if (role === "mobile") {
    // Flutter: scan lib/*/
    const libDir = path.join(absPath, "lib");
    if (fs.existsSync(libDir)) {
      try {
        const entries = fs.readdirSync(libDir, { withFileTypes: true });
        for (const entry of entries) {
          if (!entry.isDirectory()) continue;
          const featurePath = path.join(libDir, entry.name);
          const files = collectSourceFiles(featurePath);
          if (files.length === 0) continue;
          const hash = crypto.createHash("sha256");
          files.sort();
          for (const f of files) {
            try {
              hash.update(f);
              hashFileStream(path.join(ROOT, f), hash);
            } catch {}
          }
          modules[entry.name] = {
            hash: hash.digest("hex"),
            files: files.length,
          };
        }
      } catch {}
    }
  } else if (role === "ui") {
    // Frontend: scan app/(dashboard)/*/
    const dashboardDir = findDashboardDir(absPath);
    if (dashboardDir) {
      try {
        const entries = fs.readdirSync(dashboardDir, { withFileTypes: true });
        for (const entry of entries) {
          if (!entry.isDirectory()) continue;
          const entityPath = path.join(dashboardDir, entry.name);
          const files = collectSourceFiles(entityPath);
          if (files.length === 0) continue;
          const hash = crypto.createHash("sha256");
          files.sort();
          for (const f of files) {
            try {
              hash.update(f);
              hashFileStream(path.join(ROOT, f), hash);
            } catch {}
          }
          modules[entry.name] = {
            hash: hash.digest("hex"),
            files: files.length,
          };
        }
      } catch {}
    }
  }

  return modules;
}

/**
 * Find Modules/ directory (searches 2 levels deep). Returns first match.
 */
function findModulesDir(absPath) {
  const results = findAllModulesDirs(absPath);
  return results.length > 0 ? results[0] : null;
}

/**
 * Find ALL Modules/ directories across project layers (searches 2 levels deep).
 * E.g. Application/Modules/, Backend/Modules/
 */
function findAllModulesDirs(absPath) {
  const results = [];
  const direct = path.join(absPath, "Modules");
  if (fs.existsSync(direct) && fs.statSync(direct).isDirectory()) {
    results.push(direct);
  }
  try {
    const entries = fs.readdirSync(absPath, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory() || entry.name.startsWith(".")) continue;
      if (["node_modules", "bin", "obj", "dist"].includes(entry.name)) continue;
      const nested = path.join(absPath, entry.name, "Modules");
      if (fs.existsSync(nested) && fs.statSync(nested).isDirectory()) {
        results.push(nested);
      }
    }
  } catch {}
  return results;
}

/**
 * Find all directories with a given name up to maxDepth levels deep.
 */
function findDirsNamed(absPath, dirName, maxDepth = 2, currentDepth = 0) {
  const results = [];
  if (currentDepth > maxDepth) return results;
  try {
    const entries = fs.readdirSync(absPath, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory() || entry.name.startsWith(".")) continue;
      if (["node_modules", "bin", "obj", "dist"].includes(entry.name)) continue;
      if (entry.name === dirName) {
        results.push(path.join(absPath, entry.name));
      } else if (currentDepth < maxDepth) {
        results.push(...findDirsNamed(path.join(absPath, entry.name), dirName, maxDepth, currentDepth + 1));
      }
    }
  } catch {}
  return results;
}

/**
 * Find app/(dashboard)/ directory.
 */
function findDashboardDir(absPath) {
  const candidates = [
    path.join(absPath, "app", "(dashboard)"),
    path.join(absPath, "src", "app", "(dashboard)"),
  ];
  for (const dir of candidates) {
    if (fs.existsSync(dir) && fs.statSync(dir).isDirectory()) return dir;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const skipCache = process.argv.includes("--no-cache") || process.argv.includes("--force");

  // ---------------------------------------------------------------------------
  // Early-exit cache gate
  //
  // Skip the full discovery/hash loop when all three conditions hold:
  //   1. --no-cache / --force was NOT passed
  //   2. .detect-cache.json exists and its mtime is within CACHE_TTL_MS (5 min)
  //   3. No manifest files (package.json, *.csproj, go.mod, …) are dirty
  //      according to `git status --porcelain`
  //
  // On ANY error the gate falls through silently — fail-open, never crashes.
  // ---------------------------------------------------------------------------
  if (!skipCache) {
    try {
      const cachePath = path.join(ROOT, ".claude", ".detect-cache.json");
      if (fs.existsSync(cachePath)) {
        const cacheAge = Date.now() - fs.statSync(cachePath).mtimeMs;
        if (cacheAge < CACHE_TTL_MS) {
          // Check whether any manifest file changed since the cache was written
          const gitOut = execSync(
            `git status --porcelain -- ${MANIFEST_GLOBS.join(" ")}`,
            { cwd: ROOT, encoding: "utf-8", stdio: ["pipe", "pipe", "pipe"] }
          );
          const manifestDirty = gitOut.trim().length > 0;
          if (!manifestDirty) {
            // Cache is fresh and manifests are clean — reuse previous output
            const cached = JSON.parse(fs.readFileSync(cachePath, "utf-8"));
            // Reconstruct the same shape that main() emits on stdout
            const result = {
              subprojects: cached.subprojects || [],
              agents: getAgents(),
              detectedAgents: Array.from(
                new Set((cached.subprojects || []).map((s) => s.agent).filter((a) => a && a !== "general"))
              ).sort(),
              promptsDir: ".claude/prompts",
              promptsCompiledDir: ".claude/prompts_compiled",
              sourceHashes: cached.sourceHashes || {},
              moduleHashes: cached.moduleHashes || {},
            };
            process.stdout.write(JSON.stringify(result, null, 2) + "\n");
            return;
          }
        }
      }
    } catch {
      // Fall through to full discovery on any error
    }
  }

  // 1. Discover subproject paths (merge submodules + CLAUDE.md scan)
  const submodulePaths = getSubmodulePaths() || [];
  const submoduleSet = new Set(submodulePaths);
  const scannedPaths = scanForSubprojects();
  const notInGit = [];
  const seen = new Set(submodulePaths);
  for (const p of scannedPaths) {
    if (!seen.has(p)) {
      seen.add(p);
      submodulePaths.push(p);
      notInGit.push(p);
    }
  }
  const subprojectPaths = submodulePaths;

  // Load previous cache for hash comparison (anti-stale detection)
  let previousCache = null;
  try {
    const cachePath = path.join(ROOT, ".claude", ".detect-cache.json");
    if (fs.existsSync(cachePath)) {
      previousCache = JSON.parse(fs.readFileSync(cachePath, "utf-8"));
    }
  } catch {
    // no previous cache — treat all as changed
  }

  // 2. Filter to only those with a CLAUDE.md, then build subproject entries
  const subprojects = [];
  const detectedAgentsSet = new Set();
  const sourceHashes = {};
  const moduleHashes = {};

  for (const relPath of subprojectPaths) {
    const absPath = path.join(ROOT, relPath);
    const claudeFile = path.join(absPath, "CLAUDE.md");

    if (!fs.existsSync(claudeFile)) continue;

    const name = path.basename(relPath);
    const { role, scores } = detectRole(absPath);
    const agent = roleToAgent(role);
    const commands = getCommands(absPath);
    const stackSummary = getStackSummary(absPath);

    if (agent !== "general") {
      detectedAgentsSet.add(agent);
    }

    // Compute source hash for incremental scan
    const normalizedPath = relPath.split(path.sep).join("/");
    sourceHashes[name] = computeSourceHash(normalizedPath);

    // Compute module-level hashes for fine-grained incremental
    const modHashes = computeModuleHashes(normalizedPath, role);
    if (Object.keys(modHashes).length > 0) {
      moduleHashes[name] = modHashes;
    }

    // Detect git dirty state (uncommitted source file changes)
    const gitDirty = getGitDirtyFiles(normalizedPath);

    // Compare current hash against previous cache to detect stale state
    const prevHash = previousCache?.sourceHashes?.[name];
    const hashChanged = !prevHash || prevHash !== sourceHashes[name];

    subprojects.push({
      name,
      path: normalizedPath,
      role,
      agent,
      commands,
      stackSummary,
      hashChanged,
      ...(gitDirty.dirty ? { gitDirty: true, gitDirtyCount: gitDirty.files.length } : {}),
    });
  }

  // 3. Discover agents (from prompts/*.md files)
  const agents = getAgents();

  // 4. Build warnings for subprojects not registered as git submodules
  const warnings = [];
  for (const p of notInGit) {
    warnings.push(
      `"${p}" has CLAUDE.md but is NOT a git submodule. Consider: git submodule add <url> ${p}`
    );
  }

  // 5. Output
  const result = {
    subprojects,
    agents,
    detectedAgents: Array.from(detectedAgentsSet).sort(),
    promptsDir: ".claude/prompts",
    promptsCompiledDir: ".claude/prompts_compiled",
    sourceHashes,
    moduleHashes,
    ...(warnings.length > 0 ? { warnings } : {}),
  };

  process.stdout.write(JSON.stringify(result, null, 2) + "\n");

  // Release memoized collectSourceFiles cache
  clearCollectCache();

  // 6. Write detect cache for guard-verify.js and scan incremental
  //    Skip when called with --no-cache (scan uses this to avoid premature cache update)
  if (!skipCache) {
    const cachePath = path.join(ROOT, ".claude", ".detect-cache.json");
    const cacheData = {
      lastScan: new Date().toISOString(),
      subprojects,
      sourceHashes,
      moduleHashes,
    };
    try {
      fs.writeFileSync(cachePath, JSON.stringify(cacheData, null, 2), "utf-8");
    } catch {
      // ignore
    }
  }
}

main();
