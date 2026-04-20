#!/usr/bin/env node
'use strict';

/**
 * skill-generator.js
 *
 * Reads entity-registry.json v4.0 and generates skills in
 * ROOT .claude/skills/ based on detected _patterns.
 *
 * Agnostic — no hardcoded code synthesis per stack.
 * Convention sections read fields dynamically from the registry pattern JSON.
 * Real-code examples are extracted from actual source files listed in the registry.
 *
 * Usage:
 *   node .claude/scripts/skill-generator.js                     # Generate from registry
 *   node .claude/scripts/skill-generator.js --dry-run            # Show what would be generated
 *   node .claude/scripts/skill-generator.js --subproject api     # Filter to one subproject
 *   node .claude/scripts/skill-generator.js --force              # Overwrite existing skills
 */

const fs = require('fs');
const path = require('path');

// ---------------------------------------------------------------------------
// Skill meta: role mappers loaded from JSON (no hardcoded stack list)
// ---------------------------------------------------------------------------

const SKILL_META = JSON.parse(fs.readFileSync(path.join(__dirname, '_skill-meta.json'), 'utf-8'));

// ---------------------------------------------------------------------------
// Paths (mirror sync-registry.js convention)
// ---------------------------------------------------------------------------

const ROOT = path.resolve(__dirname, '..', '..');
const REGISTRY_PATH = path.join(ROOT, '.claude', 'entity-registry.json');
const DETECT_CACHE_PATH = path.join(ROOT, '.claude', '.detect-cache.json');
const TPL_DIR = path.join(__dirname, '..', 'skill-templates');

// ---------------------------------------------------------------------------
// File-extension → fenced-code-block language tag
// Loaded from _fence-languages.json — add new extensions there, not here.
// Unknown extension → empty string (no language tag, still valid markdown).
// ---------------------------------------------------------------------------

const EXT_LANG = JSON.parse(fs.readFileSync(path.join(__dirname, '_fence-languages.json'), 'utf-8'));

/**
 * Map a file extension to a fenced-code-block language tag.
 * Unknown extension returns empty string (no language hint).
 * @param {string} filePath
 * @returns {string}
 */
function extToLang(filePath) {
  return EXT_LANG[path.extname(filePath).toLowerCase()] || '';
}

// ---------------------------------------------------------------------------
// CLI flags
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
const DRY_RUN = args.includes('--dry-run');
const FORCE = args.includes('--force');
const NO_CLEANUP = args.includes('--no-cleanup');
const SUB_FILTER = (() => {
  const idx = args.indexOf('--subproject');
  return idx !== -1 && args[idx + 1] ? args[idx + 1] : null;
})();

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Read a JSON file safely. Returns null on error.
 * @param {string} filePath
 * @returns {Object|null}
 */
function readJsonSafe(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf-8'));
  } catch {
    return null;
  }
}

/**
 * Read a file's content to check for mustard:generated header.
 * @param {string} filePath
 * @returns {boolean}
 */
function isMustardGenerated(filePath) {
  try {
    const content = fs.readFileSync(filePath, 'utf-8');
    return content.includes('<!-- mustard:generated');
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Template engine (zero deps, {{var}} + {{#if key}}...{{/if}})
// Retained for the cluster-pattern template which is pure prose.
// ---------------------------------------------------------------------------

/**
 * Render a template with {{var}}, {{a.b.c}} and {{#if key}}...{{/if}}.
 * @param {string} tmpl
 * @param {Object} vars
 * @returns {string}
 */
function render(tmpl, vars) {
  let out = tmpl.replace(/\{\{#if\s+(\w+)\}\}([\s\S]*?)\{\{\/if\}\}/g, (_, key, body) => {
    const val = vars[key];
    return (val && (!Array.isArray(val) || val.length)) ? body : '';
  });
  out = out.replace(/\{\{([\w.]+)\}\}/g, (_, dotPath) => {
    const val = dotPath.split('.').reduce((o, k) => (o == null ? null : o[k]), vars);
    return val == null ? '' : String(val);
  });
  return out;
}

/**
 * Load a .md.tmpl file from TPL_DIR. Returns null if missing (fail-open).
 * @param {string} name
 * @returns {string|null}
 */
function loadTpl(name) {
  try {
    return fs.readFileSync(path.join(TPL_DIR, name), 'utf-8');
  } catch {
    process.stderr.write(`[skill-generator] Template not found: ${name}\n`);
    return null;
  }
}

// ---------------------------------------------------------------------------
// Skill validator
// ---------------------------------------------------------------------------

/**
 * Validate a generated SKILL.md body. Returns { ok, errors[] }.
 * @param {string} content
 * @returns {{ ok: boolean, errors: string[] }}
 */
function validateSkill(content) {
  const errors = [];
  const normalized = content.replace(/\r\n/g, '\n');
  const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
  if (!fm) { errors.push('missing YAML frontmatter'); return { ok: false, errors }; }

  const body = fm[1];
  const nameMatch = body.match(/^name:\s*(.+)$/m);
  const descMatch = body.match(/^description:\s*(?:"([\s\S]+?)"|([^\n]+(?:\n\s+[^\n]+)*))$/m);
  const sourceMatch = body.match(/^source:\s*(scan|manual)$/m);

  if (!nameMatch) {
    errors.push('frontmatter: missing "name"');
  } else if (!/^[a-z][a-z0-9-]+$/.test(nameMatch[1].trim())) {
    errors.push(`name not kebab-case: ${nameMatch[1]}`);
  }

  if (!descMatch) {
    errors.push('frontmatter: missing "description"');
  } else {
    const raw = (descMatch[1] || descMatch[2] || '').replace(/\s+/g, ' ').trim();
    if (raw.length < 50) errors.push(`description too short (${raw.length} chars, min 50)`);
    if (raw.length > 600) errors.push(`description too long (${raw.length} chars, max 600)`);
    if (!/\b(use when|when the user|add|create|new|detect|check|write|even if)\b/i.test(raw)) {
      errors.push('description lacks trigger words (use when / when / add / create / ...)');
    }
  }

  if (!sourceMatch) errors.push('frontmatter: missing "source" (expected scan|manual)');

  return { ok: errors.length === 0, errors };
}

// Dedup tracker: names already written in this run
const _writtenNames = new Set();
// Track skill folders already purged in this FORCE run so we only rm -rf once
// per skill dir even when multiple files (SKILL.md + references/examples.md)
// belong to the same folder.
const _purgedFolders = new Set();
// Agent prefixes this run owns (e.g. "frontend", "backend", "api", "app",
// "general"). Used as territorial guard for cleanup — never touch folders
// whose prefix is not in this set.
const _processedAgentPrefixes = new Set();

/**
 * If the target file lives inside a `.../skills/<folder>/...` path AND we are
 * in FORCE mode, rm -rf that entire `<folder>` on first touch of this run.
 *
 * Safety: only purge folders whose SKILL.md already carries the
 * `mustard:generated` marker. User-authored folders (no marker) are preserved.
 *
 * @param {string} filePath - absolute file path about to be written
 * @param {string[]} log
 */
function purgeStaleSkillFolderOnForce(filePath, log) {
  if (!FORCE || DRY_RUN) return;
  const norm = filePath.replace(/\\/g, '/');
  const m = norm.match(/^(.*\/\.claude\/skills\/[^/]+)\//);
  if (!m) return;
  const skillFolder = m[1].replace(/\//g, path.sep);
  if (_purgedFolders.has(skillFolder)) return;
  _purgedFolders.add(skillFolder);

  if (!fs.existsSync(skillFolder)) return;

  const existingSkillMd = path.join(skillFolder, 'SKILL.md');
  if (fs.existsSync(existingSkillMd) && !isMustardGenerated(existingSkillMd)) {
    log.push(`  [force-keep] manually edited skill folder preserved: ${path.relative(ROOT, skillFolder).replace(/\\/g, '/')}`);
    return;
  }

  try {
    fs.rmSync(skillFolder, { recursive: true, force: true });
    log.push(`  [force-purge] ${path.relative(ROOT, skillFolder).replace(/\\/g, '/')}`);
  } catch (err) {
    log.push(`  [force-purge-fail] ${path.relative(ROOT, skillFolder).replace(/\\/g, '/')}: ${err.message}`);
  }
}

/**
 * Write a file, creating parent directories as needed.
 * In dry-run mode, just prints what would be written.
 * Skips files without mustard:generated header unless --force.
 * Validates SKILL.md content before writing (unless --force).
 * @param {string} filePath
 * @param {string} content
 * @param {string[]} log - collector for summary lines
 */
function writeFile(filePath, content, log) {
  const relPath = path.relative(ROOT, filePath).replace(/\\/g, '/');

  if (filePath.endsWith('SKILL.md') && !FORCE) {
    const validation = validateSkill(content);
    if (!validation.ok) {
      log.push(`  [invalid] ${relPath}: ${validation.errors.join(', ')}`);
      return;
    }
    const nameMatch = content.match(/^name:\s*(.+)$/m);
    if (nameMatch) {
      const skillName = nameMatch[1].trim();
      if (_writtenNames.has(skillName)) {
        log.push(`  [skip-dup] ${relPath}: name already written (${skillName})`);
        return;
      }
      _writtenNames.add(skillName);
    }
  }

  if (DRY_RUN) {
    log.push(`  [dry-run] would write: ${relPath}`);
    return;
  }

  if (!FORCE && fs.existsSync(filePath) && !isMustardGenerated(filePath)) {
    log.push(`  [skip] manually edited: ${relPath}`);
    return;
  }

  purgeStaleSkillFolderOnForce(filePath, log);

  try {
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, content, 'utf-8');
    log.push(`  [write] ${relPath}`);
  } catch (err) {
    process.stderr.write(`[skill-generator] Failed to write ${relPath}: ${err.message}\n`);
  }
}

/**
 * Format a date as ISO string (YYYY-MM-DDTHH:MM:SSZ).
 * @returns {string}
 */
function isoNow() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/**
 * Capitalise the first letter of a string.
 * @param {string} s
 * @returns {string}
 */
function cap(s) {
  if (!s) return s;
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/**
 * Return the first non-null/non-undefined value from an array.
 * @param {...any} vals
 * @returns {any}
 */
function first(...vals) {
  for (const v of vals) {
    if (v !== null && v !== undefined && v !== '') return v;
  }
  return null;
}

/**
 * Map a role string to an agent name.
 * @param {string} role
 * @returns {string}
 */
function roleToAgent(role) {
  return SKILL_META.roles[role] || 'general';
}

// ---------------------------------------------------------------------------
// Manifest file → human-readable stack label
// Ordered: more-specific manifests first. Any new ecosystem: just add a row.
// ---------------------------------------------------------------------------

const MANIFEST_LABEL_MAP = [
  // .NET
  { match: f => f.endsWith('.csproj') || f.endsWith('.sln'), label: (f) => 'C# / .NET' },
  // Rust
  { match: f => f === 'Cargo.toml', label: () => 'Rust/Cargo' },
  // Elixir
  { match: f => f === 'mix.exs', label: () => 'Elixir/Mix' },
  // Dart / Flutter
  { match: f => f === 'pubspec.yaml', label: () => 'Dart/Flutter' },
  // Go
  { match: f => f === 'go.mod', label: () => 'Go' },
  // Python (pyproject preferred over requirements.txt)
  { match: f => f === 'pyproject.toml', label: () => 'Python' },
  { match: f => f === 'setup.py' || f === 'requirements.txt', label: () => 'Python' },
  // Ruby
  { match: f => f === 'Gemfile' || f.endsWith('.gemspec'), label: () => 'Ruby' },
  // Haskell
  { match: f => f.endsWith('.cabal'), label: () => 'Haskell/Cabal' },
  // Erlang
  { match: f => f === 'rebar.config', label: () => 'Erlang/Rebar' },
  // Crystal
  { match: f => f === 'shard.yml', label: () => 'Crystal/Shards' },
  // Java / JVM
  { match: f => f === 'pom.xml', label: () => 'Java/Maven' },
  { match: f => f === 'build.gradle' || f === 'build.gradle.kts', label: (f) => f.endsWith('.kts') ? 'Kotlin/Gradle' : 'Java/Gradle' },
  // PHP
  { match: f => f === 'composer.json', label: () => 'PHP/Composer' },
  // Deno / Bun
  { match: f => f === 'deno.json' || f === 'deno.jsonc', label: () => 'Deno' },
  { match: f => f === 'bun.lockb', label: () => 'Bun' },
  // Swift
  { match: f => f === 'Package.swift', label: () => 'Swift/SPM' },
  // Nim
  { match: f => f.endsWith('.nimble'), label: () => 'Nim' },
  // Zig
  { match: f => f === 'build.zig' || f === 'build.zig.zon', label: () => 'Zig' },
  // Gleam
  { match: f => f === 'gleam.toml', label: () => 'Gleam' },
  // Node.js/TypeScript (last — tsconfig is JS-ecosystem, less specific than others above)
  { match: f => f === 'tsconfig.json', label: () => 'TypeScript/Node.js' },
  { match: f => f === 'package.json', label: () => 'Node.js' },
];

/**
 * Derive a human-readable stack label from any directory by scanning its
 * manifest files. Falls back to dominant source extension, then the stackId.
 *
 * @param {string} stackId - e.g. "zig", "cs", "elm"
 * @param {string} [absPath] - absolute path to subproject (optional)
 * @returns {string}
 */
function stackLabel(stackId, absPath) {
  // If we have a path, scan manifest files for a label
  if (absPath) {
    try {
      const entries = fs.readdirSync(absPath);
      for (const row of MANIFEST_LABEL_MAP) {
        const hit = entries.find(e => row.match(e));
        if (hit) return row.label(hit);
      }
    } catch { /* fail-open */ }
  }
  // Fall back: capitalise stackId as best-effort label
  return cap(stackId);
}

// ---------------------------------------------------------------------------
// Subproject resolution
// ---------------------------------------------------------------------------

/**
 * Build a map of stackId → [{subprojectName, path, role, agent, absPath}] from the detect cache.
 * Falls back to empty maps if cache is missing.
 *
 * @returns {Map<string, Array<{name: string, path: string, role: string, agent: string, absPath: string}>>}
 */
function buildStackSubprojectMap() {
  const cache = readJsonSafe(DETECT_CACHE_PATH);
  const subprojects = cache?.subprojects || [];
  const map = new Map();

  for (const sub of subprojects) {
    const subAbsPath = path.join(ROOT, sub.path);
    // Prefer stack from detect cache (sync-registry.js sets sub.stack).
    // Fall back to dynamic detection from manifest files + dominant extension.
    const stackId = sub.stack || detectStackFromPath(subAbsPath);
    if (!stackId) continue;

    if (!map.has(stackId)) map.set(stackId, []);
    map.get(stackId).push({
      name: sub.name,
      path: sub.path,
      role: sub.role || 'general',
      agent: sub.agent || roleToAgent(sub.role || 'general'),
      absPath: subAbsPath,
    });
  }

  return map;
}

// ---------------------------------------------------------------------------
// Dynamic manifest → stack-ID table (ordered most-specific first).
// Stack ID = a stable lowercase identifier for the ecosystem.
// Any ecosystem — add a row here, zero other changes needed.
// ---------------------------------------------------------------------------

const MANIFEST_STACK_MAP = [
  // .NET (check before generic JSON-based ones)
  { match: f => f.endsWith('.csproj') || f.endsWith('.sln'), id: 'dotnet' },
  // Rust
  { match: f => f === 'Cargo.toml', id: 'rust' },
  // Elixir
  { match: f => f === 'mix.exs', id: 'elixir' },
  // Dart / Flutter
  { match: f => f === 'pubspec.yaml', id: 'dart' },
  // Go
  { match: f => f === 'go.mod', id: 'go' },
  // Zig
  { match: f => f === 'build.zig' || f === 'build.zig.zon', id: 'zig' },
  // Gleam
  { match: f => f === 'gleam.toml', id: 'gleam' },
  // Crystal
  { match: f => f === 'shard.yml', id: 'crystal' },
  // Erlang
  { match: f => f === 'rebar.config', id: 'erlang' },
  // Haskell
  { match: f => f.endsWith('.cabal'), id: 'haskell' },
  // Ruby
  { match: f => f === 'Gemfile' || f.endsWith('.gemspec'), id: 'ruby' },
  // Python
  { match: f => f === 'pyproject.toml' || f === 'setup.py' || f === 'requirements.txt' || f === 'manage.py', id: 'python' },
  // Java / Maven
  { match: f => f === 'pom.xml', id: 'java' },
  // Kotlin / Gradle
  { match: f => f === 'build.gradle.kts', id: 'kotlin' },
  // Java / Gradle
  { match: f => f === 'build.gradle', id: 'java' },
  // PHP / Composer
  { match: f => f === 'composer.json', id: 'php' },
  // Deno
  { match: f => f === 'deno.json' || f === 'deno.jsonc', id: 'deno' },
  // Swift
  { match: f => f === 'Package.swift', id: 'swift' },
  // Nim
  { match: f => f.endsWith('.nimble'), id: 'nim' },
  // TypeScript (tsconfig is more specific than package.json)
  { match: f => f === 'tsconfig.json', id: 'typescript' },
  // Node.js fallback
  { match: f => f === 'package.json', id: 'typescript' },
];

/**
 * Detect stack ID from a subproject path — fully dynamic, no hardcoded list.
 *
 * Strategy:
 *   1. Scan root directory for known manifest files (MANIFEST_STACK_MAP).
 *   2. If no manifest hit, count source file extensions and return the dominant one.
 *   3. Returns null only if the directory is empty or unreadable.
 *
 * Adding support for a new ecosystem requires ONLY a new row in MANIFEST_STACK_MAP.
 *
 * @param {string} absPath
 * @returns {string|null}
 */
function detectStackFromPath(absPath) {
  let entries;
  try {
    entries = fs.readdirSync(absPath);
  } catch {
    return null; // unreadable directory
  }

  // 1. Manifest-file match (ordered most-specific first)
  for (const row of MANIFEST_STACK_MAP) {
    if (entries.some(e => row.match(e))) return row.id;
  }

  // 2. Dominant source extension fallback — walk the subtree up to depth 3
  const extCount = new Map();
  countExtensions(absPath, extCount, 0, 3);
  if (extCount.size === 0) return null;

  // Pick extension with highest count; exclude config/markup-only files
  const SKIP_EXTS = new Set(['.json', '.yaml', '.yml', '.toml', '.xml', '.md', '.txt', '.lock', '.cfg', '.ini', '.env']);
  let bestExt = null;
  let bestCount = 0;
  for (const [ext, count] of extCount) {
    if (SKIP_EXTS.has(ext)) continue;
    if (count > bestCount) { bestCount = count; bestExt = ext; }
  }

  if (!bestExt) return null;
  // Derive stack ID from extension (strip leading dot)
  return bestExt.slice(1).toLowerCase();
}

/**
 * Recursively count file extensions up to maxDepth.
 * @param {string} dir
 * @param {Map<string, number>} acc
 * @param {number} depth
 * @param {number} maxDepth
 */
function countExtensions(dir, acc, depth, maxDepth) {
  if (depth > maxDepth) return;
  const SKIP_DIRS = new Set(['node_modules', '.git', 'bin', 'obj', 'dist', '.next', 'vendor', 'target', '_build']);
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const e of entries) {
    if (e.name.startsWith('.')) continue;
    if (e.isDirectory()) {
      if (SKIP_DIRS.has(e.name)) continue;
      countExtensions(path.join(dir, e.name), acc, depth + 1, maxDepth);
    } else if (e.isFile()) {
      const ext = path.extname(e.name).toLowerCase();
      if (ext) acc.set(ext, (acc.get(ext) || 0) + 1);
    }
  }
}

// ---------------------------------------------------------------------------
// Pick a representative entity from registry.e for examples
// ---------------------------------------------------------------------------

/**
 * Find an entity entry that has the most interesting data (refs, enums, services).
 * @param {Object} entities - registry.e
 * @param {string[]} [preferredKeys] - entity names to prefer
 * @returns {{ name: string, info: Object }|null}
 */
function pickRepresentativeEntity(entities, preferredKeys = []) {
  const entries = Object.entries(entities || {});
  if (!entries.length) return null;

  const scored = entries.map(([name, info]) => {
    let score = 0;
    if (info.refs?.length) score += info.refs.length * 2;
    if (info.enums?.length) score += info.enums.length * 2;
    if (info.sub?.length) score += info.sub.length;
    if (info.services?.length) score += 1;
    if (info.repositories?.length) score += 1;
    if (info.dtos?.length) score += 1;
    if (info.endpoints?.length) score += info.endpoints.length;
    if (preferredKeys.includes(name)) score += 100;
    return { name, info, score };
  });

  scored.sort((a, b) => b.score - a.score);
  return { name: scored[0].name, info: scored[0].info };
}

// ---------------------------------------------------------------------------
// Real-file excerpt extractor (replaces all build*Example functions)
// ---------------------------------------------------------------------------

/**
 * Read a real source file and return an excerpt suitable for a code fence.
 * Strategy:
 *   - If file is ≤80 lines: include full content.
 *   - Otherwise: find the first class/interface/def/func/struct/type declaration
 *     and return 20 lines of surrounding context.
 * Fails silently: returns null when file is missing or unreadable.
 *
 * @param {string} filePath - absolute or project-relative path
 * @returns {string|null}
 */
function readFileExcerpt(filePath) {
  const abs = path.isAbsolute(filePath) ? filePath : path.join(ROOT, filePath);
  try {
    const raw = fs.readFileSync(abs, 'utf-8');
    const lines = raw.split('\n');
    if (lines.length <= 80) return raw.trimEnd();

    // Find first significant declaration line
    const declPattern = /^\s*(public\s+|private\s+|protected\s+|export\s+|async\s+)?(class|interface|struct|enum|def |func |fn |type )\s/;
    let declIdx = lines.findIndex(l => declPattern.test(l));
    if (declIdx === -1) declIdx = 0;

    const start = Math.max(0, declIdx - 2);
    const end = Math.min(lines.length, declIdx + 22);
    return lines.slice(start, end).join('\n').trimEnd();
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Agnostic SKILL.md content builder
// ---------------------------------------------------------------------------

/**
 * Enumerate pattern fields dynamically — produces bullet lines for any
 * fields present in the pattern JSON object, skipping internal/known-noise keys.
 *
 * @param {Object} pattern - the pattern object from registry._patterns
 * @param {string[]} [skip] - field names to omit
 * @returns {string} markdown bullet list (empty string if nothing to show)
 */
const PATTERN_SKIP_KEYS = new Set([
  // Structural fields rendered separately
  'folder', 'namingConvention', 'baseClass', 'baseInterface', 'interfaces',
  'namespacePattern',
  // Meta/internal
  'discovered', 'folderFrequency', 'separateFiles',
]);

function enumeratePatternFields(pattern, skip = []) {
  if (!pattern || typeof pattern !== 'object') return '';
  const skipSet = new Set([...PATTERN_SKIP_KEYS, ...skip]);
  const lines = [];
  for (const [key, val] of Object.entries(pattern)) {
    if (skipSet.has(key)) continue;
    if (val === null || val === undefined || val === '') continue;
    if (Array.isArray(val)) {
      if (val.length === 0) continue;
      lines.push(`- ${key}: ${val.map(v => `\`${v}\``).join(', ')}`);
    } else if (typeof val === 'object') {
      // Nested object: show as key: {field: val, ...} abbreviated
      lines.push(`- ${key}: ${JSON.stringify(val)}`);
    } else {
      lines.push(`- ${key}: \`${val}\``);
    }
  }
  return lines.join('\n');
}

/**
 * Generate a SKILL.md and references/examples.md for a registry-detected pattern.
 * 100% agnostic — reads fields dynamically from the pattern JSON.
 *
 * @param {string} sub - skill name prefix (agent/role name, e.g. "frontend")
 * @param {string} patternSlug - pattern type slug (e.g. "entity-creation")
 * @param {string} patternTitle - human-readable title
 * @param {string} description - YAML frontmatter description (must include trigger words)
 * @param {Object} pattern - pattern object from registry._patterns[stackId]
 * @param {Object} registryEntities - registry.e
 * @param {Object} registryEnums - registry._enums
 * @param {string} role
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genPatternSkill(sub, patternSlug, patternTitle, description, pattern, registryEntities, registryEnums, role) {
  const iso = isoNow();
  const folder = pattern.folder || null;
  const baseClass = pattern.baseClass || null;
  const baseInterface = pattern.baseInterface || null;
  const ifaces = Array.isArray(pattern.interfaces) ? pattern.interfaces.filter(Boolean) : [];
  const nsPattern = pattern.namespacePattern || null;
  const naming = pattern.namingConvention || null;

  // Primary convention bullets
  const convLines = [];
  if (folder) convLines.push(`- Folder: \`${folder}\``);
  if (baseClass) convLines.push(`- Base class: \`${baseClass}\``);
  if (baseInterface) convLines.push(`- Base interface: \`${baseInterface}\``);
  if (ifaces.length) convLines.push(`- Interfaces: \`${ifaces.join(', ')}\``);
  if (nsPattern) convLines.push(`- Namespace: \`${nsPattern}.{Entity}\``);
  if (naming) convLines.push(`- Naming: \`${naming}\``);

  // Extra fields from pattern (dynamic — no assumptions about which fields exist)
  const extraFields = enumeratePatternFields(pattern);
  if (extraFields) convLines.push(extraFields);

  const convSection = convLines.join('\n');

  // Pick up to 3 real entity entries for examples
  const allEntityKeys = Object.keys(registryEntities || {});
  const repEntries = allEntityKeys.slice(0, 3).map(k => ({ key: k, info: registryEntities[k] }));

  // Real-examples bullet list
  const repBullets = repEntries
    .filter(e => e.info?.file)
    .map(e => `- \`${e.key}\` — \`${e.info.file}\``)
    .join('\n');

  const skillMd = `---
name: ${sub}-${patternSlug}
description: "${description}"
source: scan
---
<!-- mustard:generated at:${iso} role:${role} -->

# ${patternTitle}

> Pattern detected in this project.

## Convention
${convSection}

## Real examples in this codebase
${repBullets || '- (no entities with file references in registry)'}

## References
See \`references/examples.md\` for extracted code.
`;

  // Build examples.md from real source files
  const examplesMd = buildExamplesMd(patternTitle, repEntries);

  return { skillMd, examplesMd };
}

/**
 * Build references/examples.md by reading actual source files from registry entries.
 * Skips entries with no file path or where the file doesn't exist (stale registry).
 *
 * @param {string} patternTitle
 * @param {Array<{key: string, info: Object}>} entries
 * @returns {string}
 */
function buildExamplesMd(patternTitle, entries) {
  const iso = isoNow();
  let md = `<!-- mustard:generated at:${iso} -->\n\n# ${patternTitle} — real examples from this codebase\n\n`;

  let hasAny = false;
  for (const { key, info } of entries) {
    const filePath = info?.file;
    if (!filePath) continue;

    const excerpt = readFileExcerpt(filePath);
    if (excerpt === null) continue; // stale registry entry — skip silently

    hasAny = true;
    const lang = extToLang(filePath);
    md += `## ${key}\nSource: \`${filePath}\`\n\`\`\`${lang}\n${excerpt}\n\`\`\`\n\n`;
  }

  if (!hasAny) {
    md += '_No source files found — registry may be stale. Run `sync-registry.js --force` to refresh._\n';
  }

  return md;
}

// ---------------------------------------------------------------------------
// Per-pattern descriptors — fully derived, no switch/case per slug.
// ---------------------------------------------------------------------------

/**
 * Derive a human title and description for any pattern slug, in any stack.
 * No hardcoded titles. No stack-specific prose. Works for unknown slugs too.
 *
 * Description is guaranteed to contain at least one validator trigger word
 * ("Use when", "add", "create", "new", "even if") and be ≥50 chars.
 *
 * @param {string} slug   - e.g. "entity-creation", "saga-orchestration"
 * @param {string} label  - human stack label (e.g. "Zig", "C# / .NET")
 * @param {Object|null} [patternData] - the pattern JSON from registry (optional)
 * @returns {{ title: string, description: string }}
 */
function deriveDescriptor(slug, label, patternData) {
  // Title: "entity-creation" → "Entity Creation"
  const title = slug.split('-').map(cap).join(' ');

  // Human-readable slug phrase: "entity creation"
  const humanSlug = slug.replace(/-/g, ' ');

  // Optional folder hint from registry data
  const folder = patternData?.folder ? ` (detected in \`${patternData.folder}\`)` : '';

  // Compose a description that is unambiguous about trigger conditions.
  // The template guarantees "Use when" + "add" trigger words for validateSkill().
  const description = [
    `Pattern for ${humanSlug}${folder} in this ${label} project.`,
    `Use when adding new ${humanSlug} artifacts to the codebase,`,
    `or when the user says 'add ${humanSlug}', 'create ${humanSlug}', 'new ${humanSlug}', 'implement ${humanSlug}'.`,
    `Even if the user just says '${humanSlug}'.`,
  ].join(' ');

  return { title, description };
}

// ---------------------------------------------------------------------------
// Cluster skill generator (generic/agnostic discovery) — unchanged
// ---------------------------------------------------------------------------

/**
 * Derive structural stopwords from a project-wide folder frequency index.
 * A segment is "structural" for THIS project if it appears in ≥60% of all folders.
 *
 * @param {{totalFolders: number, segments: Object<string, number>}|null} folderFrequency
 * @returns {Set<string>} lowercase stopwords
 */
function deriveStopwords(folderFrequency) {
  const stopwords = new Set();
  if (!folderFrequency || !folderFrequency.totalFolders || folderFrequency.totalFolders < 5) {
    return stopwords;
  }
  const total = folderFrequency.totalFolders;
  const segments = folderFrequency.segments || {};
  for (const [seg, count] of Object.entries(segments)) {
    if (count / total >= 0.6) stopwords.add(seg.toLowerCase());
  }
  return stopwords;
}

/**
 * Extract distinctive folder-name keywords from a cluster's folders.
 *
 * @param {string[]} folders
 * @param {{totalFolders: number, segments: Object<string, number>}|null} [folderFrequency]
 * @returns {string} comma-separated top-3 distinctive keywords (empty if none)
 */
function extractFolderKeywords(folders, folderFrequency = null) {
  if (!folders || !folders.length) return '';
  const STOPWORDS = deriveStopwords(folderFrequency);
  const freq = new Map();
  for (const folder of folders) {
    if (typeof folder !== 'string') continue;
    const parts = folder.split(/[\\/]/).filter(Boolean);
    for (const part of parts) {
      const lower = part.toLowerCase();
      if (STOPWORDS.has(lower)) continue;
      if (part.length < 3) continue;
      if (part.includes('.')) continue;
      freq.set(part, (freq.get(part) || 0) + 1);
    }
  }
  return Array.from(freq.entries())
    .filter(([, count]) => folders.length === 1 || count >= 2)
    .sort((a, b) => b[1] - a[1] || b[0].length - a[0].length)
    .slice(0, 3)
    .map(([k]) => k)
    .join(', ');
}

/**
 * Generate a SKILL.md for a discovered structural cluster.
 *
 * @param {string} sub - subproject agent name
 * @param {string} stackId
 * @param {Object} cluster - cluster descriptor from discoverClusters()
 * @param {string} role
 * @param {{totalFolders: number, segments: Object<string, number>}|null} [folderFrequency]
 * @returns {{ skillMd: string, slug: string }|null}
 */
function genClusterSkill(sub, stackId, cluster, role, folderFrequency = null) {
  const tpl = loadTpl('cluster-pattern.skill.md.tmpl');
  if (!tpl) return null;

  const iso = isoNow();
  const label = stackLabel(stackId);

  const suffix = cluster.suffix || cluster.commonBaseClass || 'Pattern';

  const slug = suffix
    .toLowerCase()
    .replace(/[^a-z0-9]/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '') || 'cluster';

  const humanSuffix = suffix;
  const folderPattern = cluster.folderPattern || cluster.folder || '(multiple)';
  const folderCount = cluster.folders ? cluster.folders.length : 1;
  const samples = cluster.samples || [];
  const samplesList = samples.map(s => `- \`${s}\``).join('\n');

  const folderKeywords = extractFolderKeywords(
    cluster.folders || (cluster.folder ? [cluster.folder] : []),
    folderFrequency
  );
  const folderKeywordsHint = folderKeywords ? `, or when the task involves ${folderKeywords}` : '';
  const folderKeywordsSection = folderKeywords
    ? `- Context keywords (from folder paths): \`${folderKeywords}\`\n`
    : '';

  const vars = {
    sub,
    label,
    slug,
    suffix,
    humanSuffix,
    ext: cluster.ext || '',
    fileCount: cluster.fileCount,
    folderPattern,
    folderCount,
    commonBaseClass: cluster.commonBaseClass || '',
    commonInterfaces: (cluster.commonInterfaces || []).join(', '),
    samples: samples.length ? samples : null,
    samplesList,
    folderKeywords,
    folderKeywordsHint,
    folderKeywordsSection,
    iso,
    role,
  };

  const skillMd = render(tpl, vars);
  return { skillMd, slug };
}

// ---------------------------------------------------------------------------
// Main generation logic
// ---------------------------------------------------------------------------

/**
 * Meta keys inside a stack's _patterns[stackId] object that are NOT themselves
 * patterns (they carry discovery metadata, not skill-worthy conventions).
 */
const PATTERN_META_KEYS = new Set(['discovered', 'folderFrequency']);

/**
 * Determine if a pattern object has at least one useful (non-empty) value
 * after filtering out structural/meta fields that are rendered separately
 * or carry no skill-generation signal.
 *
 * @param {Object|null|undefined} pattern
 * @returns {boolean}
 */
function hasUsefulFields(pattern) {
  if (!pattern || typeof pattern !== 'object') return false;
  for (const [key, val] of Object.entries(pattern)) {
    if (key.startsWith('_')) continue;
    if (val === null || val === undefined || val === '') continue;
    if (Array.isArray(val)) {
      if (val.length > 0) return true;
      continue;
    }
    if (typeof val === 'object') {
      if (Object.keys(val).length > 0) return true;
      continue;
    }
    // Any non-empty primitive counts.
    return true;
  }
  return false;
}

/**
 * Convert a camelCase/PascalCase key into a kebab-case slug.
 * @param {string} key
 * @returns {string}
 */
function keyToSlug(key) {
  return String(key)
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

/**
 * Generate all skills for a given stack and its subproject(s).
 *
 * @param {string} stackId
 * @param {Object} stackPatterns - _patterns[stackId]
 * @param {Array<{name: string, path: string, role: string, agent: string}>} subprojects
 * @param {Object} registry - full registry
 * @returns {Array<{filePath: string, content: string}>}
 */
function generateSkillsForStack(stackId, stackPatterns, subprojects, registry) {
  if (!subprojects.length) return [];

  const files = [];
  const registryEntities = registry.e || {};
  const registryEnums = registry._enums || {};
  // Derive label from any subproject's abs path (all share the same stack)
  const firstAbsPath = subprojects[0]?.absPath || null;
  const label = stackLabel(stackId, firstAbsPath);

  // Role-based registry skills live in ROOT .claude/skills/ (single copy per role).
  const rootSkillsDir = path.join(ROOT, '.claude', 'skills');
  const emittedFolders = new Set();

  for (const sub of subprojects) {
    const skillsDir = rootSkillsDir;
    const role = sub.role || 'general';
    const subName = sub.agent || role;

    const emitSkill = (slug, skillMd, examplesMd) => {
      const folder = `${subName}-${slug}`;
      if (emittedFolders.has(folder)) return;
      emittedFolders.add(folder);
      files.push({ filePath: path.join(skillsDir, folder, 'SKILL.md'), content: skillMd });
      if (examplesMd) {
        files.push({ filePath: path.join(skillsDir, folder, 'references', 'examples.md'), content: examplesMd });
      }
    };

    // Dynamic pattern generation — iterate over every non-meta key in the
    // stack's pattern object. No hardcoded slug list: any key the registry
    // produces becomes a candidate skill, provided it carries useful data.
    for (const [key, pattern] of Object.entries(stackPatterns)) {
      if (key.startsWith('_')) continue;
      if (PATTERN_META_KEYS.has(key)) continue;
      if (!pattern || typeof pattern !== 'object') continue;
      if (!hasUsefulFields(pattern)) continue;

      const slug = keyToSlug(key) || 'pattern';
      const { title, description } = deriveDescriptor(slug, label, pattern);
      const { skillMd, examplesMd } = genPatternSkill(
        subName, slug, title, description,
        pattern, registryEntities, registryEnums, role
      );
      emitSkill(slug, skillMd, examplesMd);
    }

    // Generic/agnostic cluster skills (discovered by structure, not by tech name)
    const alreadyGeneratedSlugs = new Set(
      files
        .map(f => {
          const folderName = path.basename(path.dirname(f.filePath));
          const m = folderName.match(/^.+?-(.+)-pattern$/);
          return m ? m[1] : null;
        })
        .filter(Boolean)
    );
    const clusterSlug = (suffix) => (suffix || '')
      .toLowerCase()
      .replace(/[^a-z0-9]/g, '-')
      .replace(/-+/g, '-')
      .replace(/^-|-$/g, '') || 'cluster';

    const discoveredClusters = stackPatterns.discovered || [];
    const folderFrequency = stackPatterns.folderFrequency || null;
    const topClusters = discoveredClusters
      .filter(c => {
        const s = clusterSlug(c.suffix || c.commonBaseClass || '');
        return s && !alreadyGeneratedSlugs.has(s);
      })
      .slice()
      .sort((a, b) => (b.fileCount || 0) - (a.fileCount || 0))
      .slice(0, 10);

    for (const cluster of topClusters) {
      const result = genClusterSkill(subName, stackId, cluster, role, folderFrequency);
      if (!result) continue;
      const { skillMd, slug } = result;
      const folder = `${subName}-${slug}-pattern`;
      if (emittedFolders.has(folder)) continue;
      emittedFolders.add(folder);
      const skillPath = path.join(skillsDir, folder, 'SKILL.md');
      files.push({ filePath: skillPath, content: skillMd });
    }
  }

  return files;
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

function main() {
  const registry = readJsonSafe(REGISTRY_PATH);
  if (!registry) {
    console.error('Error: entity-registry.json not found at', REGISTRY_PATH);
    console.error('Run: node .claude/scripts/sync-registry.js');
    process.exit(1);
  }

  const version = registry._meta?.version || '?';
  if (version < '4.0') {
    console.warn(`Warning: registry at v${version} — some patterns may be missing. Run sync-registry.js --force to upgrade.`);
  }

  const patterns = registry._patterns || {};
  const patternStacks = Object.keys(patterns);

  if (patternStacks.length === 0) {
    console.log('No patterns in registry. Run sync-registry.js first.');
    process.exit(0);
  }

  console.log(`Registry v${version} — patterns: [${patternStacks.join(', ')}]`);
  console.log(`Entities: ${Object.keys(registry.e || {}).length}, Enums: ${Object.keys(registry._enums || {}).length}`);

  const stackSubMap = buildStackSubprojectMap();

  const log = [];
  let totalSkills = 0;
  const expectedSkillFolders = new Set();
  const subNamesProcessed = new Set();
  _processedAgentPrefixes.clear();

  for (const stackId of patternStacks) {
    const stackPatterns = patterns[stackId];
    if (!stackPatterns || Object.keys(stackPatterns).length === 0) continue;

    let subs = stackSubMap.get(stackId) || [];

    if (subs.length === 0) {
      console.log(`  No subproject found for stack "${stackId}" — skipping`);
      continue;
    }

    if (SUB_FILTER) {
      subs = subs.filter(s => s.name === SUB_FILTER);
      if (subs.length === 0) continue;
    }

    console.log(`\nStack: ${stackId} → subproject(s): ${subs.map(s => s.name).join(', ')}`);

    for (const s of subs) {
      subNamesProcessed.add(s.name);
      const prefix = s.agent || s.role || 'general';
      if (prefix) _processedAgentPrefixes.add(prefix);
    }

    const files = generateSkillsForStack(stackId, stackPatterns, subs, registry);

    for (const { filePath, content } of files) {
      const rel = path.relative(path.join(ROOT, '.claude', 'skills'), filePath);
      const folderName = rel.split(/[\\/]/)[0];
      if (folderName && !folderName.startsWith('..')) expectedSkillFolders.add(folderName);

      writeFile(filePath, content, log);
      totalSkills++;
    }
  }

  // Cleanup orphan skills
  if (!NO_CLEANUP && subNamesProcessed.size > 0) {
    const skillsRoot = path.join(ROOT, '.claude', 'skills');
    const agentPrefixes = Array.from(_processedAgentPrefixes);
    const removed = cleanupOrphanSkills(skillsRoot, expectedSkillFolders, agentPrefixes, log);
    if (removed > 0) {
      console.log(`\nCleanup: ${removed} orphan skill folder(s) ${DRY_RUN ? 'would be ' : ''}removed.`);
    }
  }

  console.log('\n' + log.join('\n'));
  console.log(`\nDone: ${totalSkills} file(s) processed.`);

  if (DRY_RUN) {
    console.log('\n(dry-run — no files written)');
  }

  // Final validation sweep — non-blocking
  try {
    const { spawnSync } = require('child_process');
    const validateScript = path.join(__dirname, 'skill-validate.js');
    if (fs.existsSync(validateScript)) {
      const res = spawnSync(process.execPath, [validateScript, '--quiet'], {
        stdio: ['ignore', 'pipe', 'pipe'],
        encoding: 'utf-8',
      });
      const output = (res.stdout || '') + (res.stderr || '');
      if (output.trim()) {
        console.log('\n---- skill-validate ----');
        process.stdout.write(output);
      }
      if (res.status === 2) {
        process.exitCode = 2;
      }
    }
  } catch (err) {
    process.stderr.write(`[skill-generator] Validation pass skipped: ${err.message}\n`);
  }
}

/**
 * Remove orphan skills: folders under `.claude/skills/` that were generated by
 * a previous run (have `source: scan` in frontmatter, match a processed-agent prefix)
 * but are NOT in the current run's expected set.
 *
 * Safety rules (territorial enforcement):
 *   - Only touch folders whose SKILL.md has `source: scan` frontmatter.
 *   - Only touch folders whose name starts with one of the processed agent prefixes.
 *   - Never touch `source: manual` skills or folders without the frontmatter marker.
 *
 * @param {string} skillsRoot - absolute path to `.claude/skills/`
 * @param {Set<string>} expectedFolders - set of folder names the current run will write
 * @param {string[]} agentPrefixes - agent prefixes included in this run
 * @param {string[]} log - mutable log array
 * @returns {number} count of folders removed
 */
function cleanupOrphanSkills(skillsRoot, expectedFolders, agentPrefixes, log) {
  if (!fs.existsSync(skillsRoot)) return 0;
  let entries;
  try { entries = fs.readdirSync(skillsRoot); } catch { return 0; }

  let removed = 0;
  for (const entry of entries) {
    const folderPath = path.join(skillsRoot, entry);
    const skillPath = path.join(folderPath, 'SKILL.md');

    const belongsToProcessedAgent = agentPrefixes.some(pref => entry.startsWith(pref + '-'));
    if (!belongsToProcessedAgent) continue;

    if (!fs.existsSync(skillPath)) continue;

    let content;
    try { content = fs.readFileSync(skillPath, 'utf-8'); } catch { continue; }
    const fm = content.match(/^---\n([\s\S]*?)\n---/);
    if (!fm) continue;
    if (!/^source:\s*scan\s*$/m.test(fm[1])) continue;

    if (expectedFolders.has(entry)) continue;

    if (DRY_RUN) {
      log.push(`  [cleanup-dry] would remove orphan: ${entry}`);
      removed++;
    } else {
      try {
        fs.rmSync(folderPath, { recursive: true, force: true });
        log.push(`  [cleanup] removed orphan: ${entry}`);
        removed++;
      } catch (err) {
        log.push(`  [cleanup] FAILED to remove ${entry}: ${err.message}`);
      }
    }
  }
  return removed;
}

// Fail-open: never crash the calling process
if (require.main === module) {
  try {
    main();
  } catch (err) {
    process.stderr.write(`[skill-generator] Fatal error: ${err.message}\n${err.stack}\n`);
    process.exit(0); // fail-open
  }
}

// Export for testing
module.exports = { validateSkill, genClusterSkill, stackLabel, cleanupOrphanSkills, genPatternSkill };
