#!/usr/bin/env node
'use strict';

/**
 * scan/orchestrate.js
 *
 * Pre-dispatch orchestration for /scan. Replaces the prose protocol the LLM
 * orchestrator used to follow step-by-step. All mechanical work happens here;
 * the LLM only consumes the JSON output to dispatch Task agents.
 *
 * Contract:
 *   stdout: JSON { dispatch, skipped, generated, errors, warnings, force, fastPath }
 *   exit:   always 0 (fail-open). Per-step errors are reported in the JSON.
 *
 * Usage:
 *   node .claude/scripts/scan/orchestrate.js                   # incremental
 *   node .claude/scripts/scan/orchestrate.js --force           # full re-scan
 *   node .claude/scripts/scan/orchestrate.js <subproject>      # single subproject
 *   node .claude/scripts/scan/orchestrate.js <name> --force
 *
 * Design notes:
 *   - Each step wrapped in tryStep() — failure adds to errors[] but does not abort.
 *   - No state mutation outside the steps; all writes go through writeFileSafe().
 *   - Agent prompt template is loaded from scripts/scan/agent-prompt.template.md
 *     and rendered per dispatch, so the Task agent receives instructions inline
 *     and never needs to Read refs/scan/* itself.
 */

const fs = require('fs');
const path = require('path');
const { execFileSync, spawnSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const CLAUDE_DIR = path.join(ROOT, '.claude');
const SCRIPTS_DIR = path.join(CLAUDE_DIR, 'scripts');
const SYNC_DETECT = path.join(SCRIPTS_DIR, 'sync-detect.js');
const REGISTRY_PATH = path.join(CLAUDE_DIR, 'entity-registry.json');
const ROOT_CLAUDE_MD = path.join(ROOT, 'CLAUDE.md');
const ORCH_CLAUDE_MD = path.join(CLAUDE_DIR, 'CLAUDE.md');
const DETECT_CACHE = path.join(CLAUDE_DIR, '.detect-cache.json');
const PROMPT_TEMPLATE = path.join(__dirname, 'agent-prompt.template.md');

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

const argv = process.argv.slice(2);
const FORCE = argv.includes('--force');
const TARGET = argv.find(a => !a.startsWith('--')) || null;

// ---------------------------------------------------------------------------
// Result accumulators
// ---------------------------------------------------------------------------

const result = {
  force: FORCE,
  target: TARGET,
  fastPath: false,
  dispatch: [],
  skipped: [],
  generated: [],
  cleanup: [],
  errors: [],
  warnings: [],
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function tryStep(name, fn) {
  try {
    return fn();
  } catch (err) {
    result.errors.push(`${name}: ${err && err.message ? err.message : String(err)}`);
    return null;
  }
}

function readFileSafe(p) {
  try { return fs.readFileSync(p, 'utf-8'); } catch { return null; }
}

function readJsonSafe(p) {
  const raw = readFileSafe(p);
  if (!raw) return null;
  try { return JSON.parse(raw); } catch { return null; }
}

function writeFileSafe(p, content) {
  try {
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, content, 'utf-8');
    return true;
  } catch (err) {
    result.errors.push(`write ${relPathPosix(p)}: ${err.message}`);
    return false;
  }
}

function existsSafe(p) {
  try { return fs.existsSync(p); } catch { return false; }
}

function relPathPosix(absPath) {
  return path.relative(ROOT, absPath).split(path.sep).join('/');
}

function detectEol(content) {
  if (typeof content !== 'string') return '\n';
  const crlf = (content.match(/\r\n/g) || []).length;
  const lf = (content.match(/(?<!\r)\n/g) || []).length;
  return crlf > lf ? '\r\n' : '\n';
}

function applyEol(content, eol) {
  if (eol === '\n') return content.replace(/\r\n/g, '\n');
  return content.replace(/\r?\n/g, '\r\n');
}

// ---------------------------------------------------------------------------
// Step 1 — Discover subprojects + incremental detection
// ---------------------------------------------------------------------------

function runDetect() {
  if (!existsSafe(SYNC_DETECT)) {
    throw new Error(`sync-detect.js not found at ${SYNC_DETECT}`);
  }
  const out = execFileSync(process.execPath, [SYNC_DETECT, '--no-cache'], {
    encoding: 'utf-8',
    cwd: ROOT,
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  return JSON.parse(out);
}

function classifyForDispatch(detect, oldCache) {
  const oldHashes = (oldCache && oldCache.sourceHashes) || {};
  const dispatch = [];
  const skipped = [];

  for (const sub of detect.subprojects || []) {
    if (TARGET && sub.name !== TARGET) continue;

    const oldHash = oldHashes[sub.name];
    const newHash = (detect.sourceHashes || {})[sub.name];
    const hashChanged = !oldHash || oldHash !== newHash;
    const dirty = !!sub.gitDirty;

    if (FORCE || hashChanged || dirty) {
      dispatch.push(sub);
    } else {
      skipped.push({ name: sub.name, reason: 'hash unchanged, no git dirty' });
    }
  }

  return { dispatch, skipped };
}

// ---------------------------------------------------------------------------
// Step 2.5 — Cleanup stale subprojects
// ---------------------------------------------------------------------------

function cleanupStale(detect, oldCache) {
  const removed = [];
  if (!oldCache || !Array.isArray(oldCache.subprojects)) return removed;

  const currentNames = new Set((detect.subprojects || []).map(s => s.name));
  const oldNames = oldCache.subprojects.map(s => s.name);

  for (const name of oldNames) {
    if (currentNames.has(name)) continue;
    if (TARGET && name !== TARGET) continue;

    // Remove orchestrator-side artifacts only (never touch subproject directory itself
    // — that belongs to the user; deletion was always a footgun).
    const implAgent = path.join(CLAUDE_DIR, 'agents', `${name}-impl.md`);
    const explorerAgent = path.join(CLAUDE_DIR, 'agents', `${name}-explorer.md`);
    for (const p of [implAgent, explorerAgent]) {
      try {
        if (fs.existsSync(p)) {
          const head = readFileSafe(p) || '';
          if (head.includes('<!-- mustard:generated')) {
            fs.unlinkSync(p);
            removed.push(relPathPosix(p));
          }
        }
      } catch (err) {
        result.warnings.push(`cleanup ${name}: ${err.message}`);
      }
    }
  }

  return removed;
}

// ---------------------------------------------------------------------------
// Step 2.6 — Bootstrap foundational files
// ---------------------------------------------------------------------------

const ORCH_CLAUDE_TEMPLATE = `<!-- mustard:generated -->
# Orchestrator Rules

## Role
You do NOT implement code — you delegate via Task tool.

## Intent Routing

| Intent | Signals | Action |
|--------|---------|--------|
| Feature | create, add, new entity, new CRUD, implement | Pipeline Feature |
| Enhancement | improve, adjust, change, add field/column, optimize, update | Pipeline Feature |
| Bugfix | error, bug, not working, broken, fix, correct | Pipeline Bugfix |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | Delegate via /task |
| Simple | config, docs, small refactor, rename, move | Delegate via Task |

Any change that touches production code (schema, API, UI) → Pipeline Feature.
Read \`.claude/pipeline-config.md\` for agent dispatch rules.

## Full Reference
Rules, pipeline, naming: \`.claude/pipeline-config.md\`
`;

function buildRootClaudeMd(projectName, subprojects) {
  const rows = subprojects
    .map(s => `| ${s.name} | ${s.stackSummary || '-'} | - | [${s.name}](./${s.path}/CLAUDE.md) |`)
    .join('\n');

  return `# ${projectName} - Project Context

> Framework rules: See [.claude/CLAUDE.md](./.claude/CLAUDE.md)

## Project Structure

| Subproject | Technology | Port | CLAUDE.md |
|------------|------------|------|-----------|
${rows || '| (none detected) | - | - | - |'}

## Entity Registry

**CRITICAL:** Before searching for ANY entity, read \`.claude/entity-registry.json\` first.

## Ignore Paths

Never search in:
- \`node_modules/\`, \`.next/\`, \`bin/\`, \`obj/\`, \`dist/\`, \`migrations/\`
`;
}

function buildSubprojectClaudeMd(sub) {
  const titleCase = sub.name.charAt(0).toUpperCase() + sub.name.slice(1);
  return `# ${titleCase}

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)
> Skills: \`${sub.name}/.claude/skills/\` | Guards: \`${sub.name}/CLAUDE.md\`

## Stack

${sub.stackSummary || '(detected on next /scan)'}

## Commands

(populated by /scan)

## Key Paths

(populated by /scan)

## Guards

(populated by /scan)
`;
}

const EMPTY_REGISTRY = {
  _meta: { version: '4.0' },
  _patterns: {},
  _enums: {},
  e: {},
};

function bootstrap(detect) {
  const generated = [];

  // Fast-path: skip when foundational files exist and --force is not set
  const haveRootClaude = existsSafe(ROOT_CLAUDE_MD);
  const haveRegistry = existsSafe(REGISTRY_PATH);
  if (!FORCE && haveRootClaude && haveRegistry) {
    result.fastPath = true;
    // Still ensure orchestrator CLAUDE.md exists (always regenerated downstream)
    return generated;
  }

  // .claude/CLAUDE.md — always (re)generate (it's mustard:generated)
  if (writeFileSafe(ORCH_CLAUDE_MD, ORCH_CLAUDE_TEMPLATE)) {
    generated.push('.claude/CLAUDE.md');
  }

  // root CLAUDE.md — only if missing (preserves user customizations)
  if (!haveRootClaude) {
    const projectName = path.basename(ROOT);
    if (writeFileSafe(ROOT_CLAUDE_MD, buildRootClaudeMd(projectName, detect.subprojects || []))) {
      generated.push('CLAUDE.md');
    }
  }

  // entity-registry.json — only if missing (sync-registry refreshes it later)
  if (!haveRegistry) {
    if (writeFileSafe(REGISTRY_PATH, JSON.stringify(EMPTY_REGISTRY, null, 2))) {
      generated.push('.claude/entity-registry.json');
    }
  }

  // per-subproject CLAUDE.md — only if missing
  for (const sub of detect.subprojects || []) {
    const subClaude = path.join(ROOT, sub.path, 'CLAUDE.md');
    if (!existsSafe(subClaude)) {
      if (writeFileSafe(subClaude, buildSubprojectClaudeMd(sub))) {
        generated.push(`${sub.path}/CLAUDE.md`);
      }
    }
  }

  return generated;
}

// ---------------------------------------------------------------------------
// Step 4 — Update root CLAUDE.md project structure table
// ---------------------------------------------------------------------------

function updateRootClaudeMd(detect) {
  if (!existsSafe(ROOT_CLAUDE_MD)) return null; // bootstrap will have created it
  const current = readFileSafe(ROOT_CLAUDE_MD);
  if (!current) return null;

  const subprojects = detect.subprojects || [];
  if (subprojects.length === 0) return null;

  // Preserve existing Technology cell when sync-detect returns an empty
  // stackSummary — avoids overwriting user-curated descriptions with "-".
  const existingTech = new Map();
  const rowRe = /^\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*[^|]+?\s*\|\s*\[[^\]]+\]\([^)]+\)\s*\|\s*$/gm;
  let m;
  while ((m = rowRe.exec(current)) !== null) {
    const name = m[1].trim();
    const tech = m[2].trim();
    if (name && tech && tech !== '-') existingTech.set(name, tech);
  }

  const eol = detectEol(current);
  const normalized = current.replace(/\r\n/g, '\n');
  const newRows = subprojects
    .map(s => {
      const tech = s.stackSummary || existingTech.get(s.name) || '-';
      return `| ${s.name} | ${tech} | - | [${s.name}](./${s.path}/CLAUDE.md) |`;
    })
    .join('\n');

  // Replace the body of the Project Structure table if present
  const tableRe = /(## Project Structure\s*\n\s*\| Subproject \| Technology \| Port \| CLAUDE\.md \|\s*\n\s*\|[^\n]+\|\s*\n)([\s\S]*?)(?=\n## |\n# |$)/;
  if (!tableRe.test(normalized)) return null;

  const updatedLf = normalized.replace(tableRe, `$1${newRows}\n`);
  if (updatedLf === normalized) return null;

  const updated = applyEol(updatedLf, eol);
  if (updated === current) return null;

  if (writeFileSafe(ROOT_CLAUDE_MD, updated)) {
    return 'CLAUDE.md (Project Structure)';
  }
  return null;
}

// ---------------------------------------------------------------------------
// Step 4.5 — Generate per-subproject impl + explorer agent files
// ---------------------------------------------------------------------------

const ROLE_TOOLS = {
  api: 'Read, Write, Edit, Bash, Grep, Glob',
  ui: 'Read, Write, Edit, Bash, Grep, Glob',
  library: 'Read, Write, Edit, Bash, Grep, Glob',
  database: 'Read, Write, Edit, Bash, Grep, Glob',
  mobile: 'Read, Write, Edit, Bash, Grep, Glob',
  general: 'Read, Write, Edit, Bash, Grep, Glob',
};

function titleCase(s) {
  return s ? s.charAt(0).toUpperCase() + s.slice(1) : s;
}

function buildImplAgent(sub) {
  const tools = ROLE_TOOLS[sub.role] || ROLE_TOOLS.general;
  const Title = titleCase(sub.name);
  return `---
name: ${sub.name}-impl
description: ${sub.role} implementation for ${sub.name}. Reads ${sub.name}/CLAUDE.md for guards.
model: sonnet
tools: [${tools}]
memory: project
---
<!-- mustard:generated -->

# ${Title} Implementation Agent

## Mandatory Reads
1. \`${sub.path}/CLAUDE.md\` — guards, stack, key paths
2. \`${sub.path}/.claude/commands/guards.md\` — DO/DON'T rules
3. \`${sub.path}/.claude/commands/notes.md\` — project-specific notes

## Boundary
Role: ${sub.role}. Stack: ${sub.stackSummary || 'auto-detected'}.

## Validation
Run the build/type-check command listed in \`${sub.path}/CLAUDE.md\` → Commands.

## Return Format
### Files Modified/Created
| File | Action |
|------|--------|

### Build / Type-check
{output}

### Guards Verified
Total: {n}/{total} | Violations: {v}
`;
}

function buildExplorerAgent(sub) {
  const Title = titleCase(sub.name);
  return `---
name: ${sub.name}-explorer
description: Read-only exploration agent for ${sub.name} codebase analysis and investigation.
model: haiku
tools: [Read, Grep, Glob]
memory: project
---
<!-- mustard:generated at:${new Date().toISOString()} role:${sub.role} -->

# ${Title} Explorer Agent

> Read-only analysis of ${sub.name} codebase. Patterns, dependencies, architecture, quality evaluation.

## Mandatory Reads
1. \`${sub.path}/CLAUDE.md\` — project rules, guards, stack
2. \`${sub.path}/.claude/commands/guards.md\` — DO/DON'T rules

## Boundary
- **Read-only** — NEVER write, edit, or execute commands
- Scope: \`${sub.path}/\` directory only
- Ignore: \`bin/\`, \`obj/\`, \`node_modules/\`, \`.next/\`, \`migrations/\`
- **Budget: ≤20 tool uses total, ≤3 full file reads** — prefer Grep over Read
- Return findings as soon as pattern/root-cause is clear

## Return Format
### Findings
| Severity | File:Line | Detail |
|----------|-----------|--------|
| CRITICAL / WARNING / NOTE | path:line | description |

### Suggested Actions
- Concrete \`/task\` or pipeline commands to address findings
`;
}

function generateAgentFiles(detect) {
  const generated = [];
  const agentsDir = path.join(CLAUDE_DIR, 'agents');

  for (const sub of detect.subprojects || []) {
    if (TARGET && sub.name !== TARGET) continue;

    const implPath = path.join(agentsDir, `${sub.name}-impl.md`);
    const explorerPath = path.join(agentsDir, `${sub.name}-explorer.md`);

    // Only write the base template when missing OR --force. Preserves
    // refinements applied by previous Task agents (e.g. specific Boundary
    // or Validation commands enriched after a real scan).
    if ((FORCE || !existsSafe(implPath)) && writeFileSafe(implPath, buildImplAgent(sub))) {
      generated.push(`.claude/agents/${sub.name}-impl.md`);
    }
    if ((FORCE || !existsSafe(explorerPath)) && writeFileSafe(explorerPath, buildExplorerAgent(sub))) {
      generated.push(`.claude/agents/${sub.name}-explorer.md`);
    }
  }

  return generated;
}

// ---------------------------------------------------------------------------
// Step 2.7 — Inject frontmatter into .claude/docs/*.md
// ---------------------------------------------------------------------------

function scanProductDocs() {
  const docsDir = path.join(CLAUDE_DIR, 'docs');
  if (!existsSafe(docsDir)) return [];

  const updated = [];
  let entries;
  try {
    entries = fs.readdirSync(docsDir);
  } catch {
    return [];
  }

  for (const file of entries) {
    if (!file.endsWith('.md')) continue;
    const filePath = path.join(docsDir, file);
    const content = readFileSafe(filePath);
    if (!content) continue;

    const hasFrontmatter = /^---\s*\n/.test(content);
    const hasScannedAt = /^scanned-at:/m.test(content);

    // Skip user-authored frontmatter (without scanned-at marker)
    if (hasFrontmatter && !hasScannedAt) continue;

    // Extract H1 (name), first paragraph (description), H2s (topics)
    const h1Match = content.match(/^#\s+(.+)$/m);
    const name = h1Match ? h1Match[1].trim() : file.replace(/\.md$/, '');

    const bodyAfterH1 = h1Match ? content.slice(h1Match.index + h1Match[0].length) : content;
    const firstParaMatch = bodyAfterH1.match(/\n\n(?:>?\s*)([^\n#]+)/);
    const description = firstParaMatch ? firstParaMatch[1].trim().slice(0, 200) : '';

    const h2Matches = [...content.matchAll(/^##\s+(.+)$/gm)];
    const topics = h2Matches.slice(0, 8).map(m => m[1].trim().toLowerCase().replace(/[^a-z0-9]+/g, '-'));

    const frontmatter = `---
name: ${name}
description: ${description}
topics: [${topics.join(', ')}]
scanned-at: ${new Date().toISOString()}
---
`;

    let newContent;
    if (hasFrontmatter) {
      // Replace existing generated frontmatter
      newContent = content.replace(/^---\s*\n[\s\S]*?\n---\s*\n/, frontmatter);
    } else {
      newContent = frontmatter + content;
    }

    if (newContent !== content && writeFileSafe(filePath, newContent)) {
      updated.push(`.claude/docs/${file}`);
    }
  }

  return updated;
}

// ---------------------------------------------------------------------------
// Per-subproject registry slice — clusters + sample code injection
// ---------------------------------------------------------------------------

/**
 * Whether a cluster belongs to the given subproject. Prefers the explicit
 * `subprojectName` tag set by `cluster-discovery` (when sync-registry passed
 * the name). Falls back to folder-prefix matching for older registries.
 */
function clusterBelongsToSubproject(cluster, sub) {
  if (cluster.subprojectName) return cluster.subprojectName === sub.name;

  const folders = Array.isArray(cluster.folders) ? cluster.folders
                : (cluster.folder ? [cluster.folder] : []);
  if (folders.length === 0) return false;
  const subPathNorm = (sub.path + '/').replace(/\\/g, '/');
  return folders.some(f => {
    const norm = (f + '/').replace(/\\/g, '/');
    return norm.startsWith(subPathNorm);
  });
}

/**
 * Build a markdown block describing every cluster relevant to the subproject.
 * Includes both the original 5 structural fields and the 5 enrichment fields
 * when present (nulls are omitted, never inferred).
 *
 * @param {Object} sub
 * @param {Object|null} registry
 * @returns {string} - markdown block, or '' if no clusters apply
 */
/** Max clusters injected per agent prompt (ranked by fileCount desc).
 *  Beyond this, the agent falls back to reading entity-registry.json directly. */
const MAX_CLUSTERS_IN_PROMPT = Math.max(1, parseInt(process.env.MUSTARD_PROMPT_CLUSTER_MAX, 10) || 12);

function buildClustersBlock(sub, registry) {
  if (!registry || !registry._patterns) return '';

  const matched = [];
  for (const stackId of Object.keys(registry._patterns)) {
    const stackEntry = registry._patterns[stackId] || {};
    const discovered = Array.isArray(stackEntry.discovered) ? stackEntry.discovered : [];
    for (const c of discovered) {
      if (clusterBelongsToSubproject(c, sub)) matched.push(c);
    }
  }

  if (matched.length === 0) return '';

  // Rank by fileCount desc; cap at MAX_CLUSTERS_IN_PROMPT
  matched.sort((a, b) => (b.fileCount || 0) - (a.fileCount || 0));
  const dropped = matched.length - MAX_CLUSTERS_IN_PROMPT;
  const top = matched.slice(0, MAX_CLUSTERS_IN_PROMPT);

  const out = ['## Clusters detected for this subproject', ''];
  if (dropped > 0) {
    out.push(`> Showing top ${MAX_CLUSTERS_IN_PROMPT} of ${matched.length} clusters by file count. Remaining ${dropped} are in \`.claude/entity-registry.json\` (\`_patterns[stack].discovered[]\`) if you need them.`);
    out.push('');
  }
  for (const c of top) {
    const label = c.label || c.suffix || c.commonBaseClass || c.kind || '(unnamed)';
    out.push(`### ${label} — ${c.fileCount || 0} files (${c.kind || 'cluster'})`);
    if (Array.isArray(c.folders) && c.folders.length) {
      out.push(`- folders: ${c.folders.slice(0, 5).join(', ')}`);
    }
    if (Array.isArray(c.samples) && c.samples.length) {
      out.push(`- samples: ${c.samples.slice(0, 5).join(', ')}`);
    }
    if (c.commonBaseClass) out.push(`- commonBaseClass: ${c.commonBaseClass}`);
    if (Array.isArray(c.commonInterfaces) && c.commonInterfaces.length) {
      out.push(`- commonInterfaces: ${c.commonInterfaces.join(', ')}`);
    }
    if (c.namingPattern) out.push(`- namingPattern: ${c.namingPattern}`);
    if (Array.isArray(c.declarationKeywords) && c.declarationKeywords.length) {
      out.push(`- declarationKeywords: ${c.declarationKeywords.map(k => `"${k}"`).join(', ')}`);
    }
    if (Array.isArray(c.declarationSuffix) && c.declarationSuffix.length) {
      out.push(`- declarationSuffix: ${c.declarationSuffix.map(k => `"${k}"`).join(', ')}`);
    }
    if (Array.isArray(c.topOfFileLines) && c.topOfFileLines.length) {
      out.push('- topOfFileLines:');
      for (const line of c.topOfFileLines) out.push(`    ${line}`);
    }
    if (Array.isArray(c.memberSuffixes) && c.memberSuffixes.length) {
      out.push(`- memberSuffixes: ${c.memberSuffixes.join(', ')}`);
    }
    out.push('');
  }
  return out.join('\n');
}

/** Best-effort resolution of cluster sample → absolute file path. */
function resolveSampleAbsPath(cluster, sample, root) {
  // Sample may include folder path already
  const direct = path.join(root, sample);
  try {
    if (fs.existsSync(direct) && fs.statSync(direct).isFile()) return direct;
  } catch { /* fall through */ }

  const folders = Array.isArray(cluster.folders) ? cluster.folders
                : (cluster.folder ? [cluster.folder] : []);
  for (const folder of folders) {
    const candidate = path.join(root, folder, sample);
    try {
      if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate;
    } catch { /* skip */ }
  }
  return null;
}

/**
 * Build a markdown block with the first ≤60 lines of one sample per cluster.
 * Agent uses these to seed `references/examples.md` without re-Reading.
 * Cluster.folders are relative to the SUBPROJECT path, not the monorepo root —
 * so we resolve samples against `<root>/<sub.path>`, not `<root>` alone.
 *
 * @param {Object} sub
 * @param {Object|null} registry
 * @param {string} root
 * @returns {string}
 */
function buildSamplesBlock(sub, registry, root) {
  if (!registry || !registry._patterns) return '';

  const SAMPLE_LINE_CAP = 40;
  const subRoot = path.join(root, sub.path);
  const out = ['## Sample code per cluster', ''];
  let any = false;

  // Same ranking + cap as buildClustersBlock so the two sections align.
  const matched = [];
  for (const stackId of Object.keys(registry._patterns)) {
    const stackEntry = registry._patterns[stackId] || {};
    const discovered = Array.isArray(stackEntry.discovered) ? stackEntry.discovered : [];
    for (const c of discovered) {
      if (clusterBelongsToSubproject(c, sub)) matched.push(c);
    }
  }
  matched.sort((a, b) => (b.fileCount || 0) - (a.fileCount || 0));
  const top = matched.slice(0, MAX_CLUSTERS_IN_PROMPT);

  for (const c of top) {
    {
      const samples = Array.isArray(c.samples) ? c.samples : [];
      if (samples.length === 0) continue;

      let abs = null;
      for (const s of samples) {
        abs = resolveSampleAbsPath(c, s, subRoot);
        if (abs) break;
      }
      if (!abs) continue;

      const content = readFileSafe(abs);
      if (!content) continue;

      const ext = path.extname(abs).slice(1) || '';
      const lines = content.split(/\r?\n/).slice(0, SAMPLE_LINE_CAP);
      const label = c.label || c.suffix || c.commonBaseClass || c.kind || '(unnamed)';
      const relPath = path.relative(root, abs).split(path.sep).join('/');

      out.push(`### ${label} — ${relPath} (lines 1-${lines.length})`);
      out.push('```' + ext);
      out.push(...lines);
      out.push('```');
      out.push('');
      any = true;
    }
  }

  return any ? out.join('\n') : '';
}

// ---------------------------------------------------------------------------
// Agent prompt rendering
// ---------------------------------------------------------------------------

function loadPromptTemplate() {
  const tpl = readFileSafe(PROMPT_TEMPLATE);
  if (!tpl) {
    throw new Error(`prompt template missing at ${PROMPT_TEMPLATE}`);
  }
  return tpl;
}

// Budget + evidence rule blocks — selected per-prompt based on whether the
// orchestrator was able to inject clusters/samples for this subproject.

const BUDGET_FULL = `## Budget guidance (soft)
- Target: ~50 tool uses, ~30k tokens of context. The orchestrator already injected enriched clusters + sample code above — most Read work is already done for you.
- Heuristic: if your last 3 Reads revealed no new pattern (same structure as previous samples), STOP exploring and emit skills with what you have.
- Skills with fewer fields > skills with invented fields. Always cite \`tool_uses_used: N\` in the return JSON.
- A typical skill needs only: 1 Glob (verify paths if uncertain) + 1 Write (SKILL.md) + 1 Write (references/examples.md from injected sample). Estimate 2-3 ops per skill.`;

const BUDGET_MINIMAL = `## Budget guidance (soft)
- Target: ~60 tool uses, ~80k tokens. Stop after last 3 Reads reveal no new pattern.
- Skills with fewer fields > skills with invented fields. Cite \`tool_uses_used: N\` in return JSON.`;

const EVIDENCE_FULL = `## EVIDENCE RULE — applies to every skill you emit

1. **Cluster backing.** Each skill must correspond to a cluster from the \`## Clusters detected for this subproject\` block above (or to the registry's \`_patterns[stack].discovered[]\` if that block is empty), with \`fileCount >= 3\`. The cluster's \`suffix\` (slugified) MUST appear as a token in the skill name. Convention: \`{name-short}-{suffix-slug}-pattern\`. Never rename to library brands (no \`react-query-pattern\` if the cluster suffix is \`service-hook\`).

2. **Path verification.** Paths under \`## Real examples\` or \`## Samples in this project\` come from \`cluster.samples[]\` (visible in the block above) — those were already discovered by \`cluster-discovery.js\` from real files. Use them directly. Only Glob to verify if you genuinely can't find a sample.

3. **Convention fields come from the cluster object above — no Read needed.** Each cluster includes structural fields (suffix, folders, fileCount, samples, commonBaseClass, commonInterfaces) plus universal enrichment fields when present (namingPattern, declarationKeywords, declarationSuffix, topOfFileLines, memberSuffixes). Use whichever fields are present. Skip fields that are absent — do NOT Read source files to "fill in" missing fields. The orchestrator already extracted what universal heuristics could find; absent fields mean the heuristic didn't apply (e.g. unusual language conventions). That is OK.

4. **Code in \`references/examples.md\` comes from the "Sample code per cluster" block above.** Copy verbatim from that block into \`references/examples.md\` — do NOT Read the source file again. The orchestrator already extracted the first 60 lines of one sample per cluster. If a cluster lacks a sample block (because no readable file existed), you may skip the examples.md for that skill or do ONE Read.

5. **Skip over invent.** If you cannot meet rules 1-4 for a candidate skill, skip it. Empty is better than invented.`;

const EVIDENCE_MINIMAL = `## EVIDENCE RULE — applies to every skill you emit

1. **Cluster backing.** Each skill must correspond to a cluster in \`_patterns[stack].discovered[]\` of \`.claude/entity-registry.json\` with \`fileCount >= 3\`. The cluster's \`suffix\` (slugified) MUST appear as a token in the skill name. Convention: \`{name-short}-{suffix-slug}-pattern\`. Never rename to library brands.

2. **Path verification.** Every path under \`## Real examples\` or \`## Samples in this project\` MUST be confirmed via Glob/Read. Drop entries that do not exist on disk.

3. **Convention fields are derived from real files.** Start from cluster keys (suffix, folders, fileCount, commonBaseClass, commonInterfaces). Add fields ONLY IF backed by a verbatim Read of ≥3 real files under the cluster's folders. Value reflects the MAJORITY observed.

4. **No code in SKILL.md body.** All concrete code goes to \`references/examples.md\`, extracted via Read from a real source file (verbatim, ≤80 lines).

5. **Skip over invent.** If you cannot meet rules 1-4 for a candidate skill, skip it. Empty is better than invented.`;

function renderPrompt(template, sub, registry) {
  const forceBlock = FORCE
    ? `FORCE MODE ACTIVE:
- Before generating skills, scan ${sub.path}/.claude/skills/ and delete every subdirectory
  whose SKILL.md contains "<!-- mustard:generated" (preserve user-authored skills).
- Also delete any pre-existing _backup/ under ${sub.path}/.claude/commands/ to avoid stacking stale backups.
`
    : '';

  const absSubprojectPath = path.resolve(ROOT, sub.path).split(path.sep).join('/');
  const clustersBlock = buildClustersBlock(sub, registry);
  const samplesBlock = buildSamplesBlock(sub, registry, ROOT);

  // Choose budget + evidence variants: FULL when orchestrator injected
  // clusters/samples, MINIMAL when no injection (avoids confusing the agent
  // with "use injected blocks above" instructions when no blocks exist).
  const hasInjectedContext = !!(clustersBlock || samplesBlock);
  const budgetBlock   = hasInjectedContext ? BUDGET_FULL   : BUDGET_MINIMAL;
  const evidenceBlock = hasInjectedContext ? EVIDENCE_FULL : EVIDENCE_MINIMAL;

  return template
    .replace(/\{\{name\}\}/g, sub.name)
    .replace(/\{\{path\}\}/g, sub.path)
    .replace(/\{\{absSubprojectPath\}\}/g, absSubprojectPath)
    .replace(/\{\{role\}\}/g, sub.role || 'general')
    .replace(/\{\{stack\}\}/g, sub.stackSummary || '(unknown)')
    .replace(/\{\{forceBlock\}\}/g, forceBlock)
    .replace(/\{\{clustersBlock\}\}/g, clustersBlock)
    .replace(/\{\{samplesBlock\}\}/g, samplesBlock)
    .replace(/\{\{budgetBlock\}\}/g, budgetBlock)
    .replace(/\{\{evidenceBlock\}\}/g, evidenceBlock);
}

// ---------------------------------------------------------------------------
// Main orchestration
// ---------------------------------------------------------------------------

function main() {
  // Load old cache (used for hash comparison + cleanup)
  const oldCache = readJsonSafe(DETECT_CACHE);

  // Step 1 — detect subprojects
  const detect = tryStep('detect', runDetect);
  if (!detect || !Array.isArray(detect.subprojects)) {
    result.errors.push('detect: no subprojects discovered (sync-detect output invalid)');
    process.stdout.write(JSON.stringify(result, null, 2) + '\n');
    return;
  }

  // Step 2.5 — cleanup stale (uses old cache to compare)
  result.cleanup = tryStep('cleanup', () => cleanupStale(detect, oldCache)) || [];

  // Step 2.6 — bootstrap foundational files (skip if fast-path applies)
  result.generated.push(...(tryStep('bootstrap', () => bootstrap(detect)) || []));

  // Step 2.7 — scan product docs (frontmatter injection)
  result.generated.push(...(tryStep('scanProductDocs', scanProductDocs) || []));

  // Step 4 — update root CLAUDE.md project structure table
  const rootUpdated = tryStep('updateRootClaudeMd', () => updateRootClaudeMd(detect));
  if (rootUpdated) result.generated.push(rootUpdated);

  // Step 4.5 — generate per-subproject agent files
  result.generated.push(...(tryStep('generateAgentFiles', () => generateAgentFiles(detect)) || []));

  // Load entity-registry once — used to slice clusters/samples per subproject
  // when rendering the agent prompt. Registry may be empty/missing on first run;
  // in that case the prompt skips the clusters/samples blocks (agent falls back
  // to manual exploration).
  const registry = readJsonSafe(REGISTRY_PATH);

  // Classify subprojects: dispatch vs skip
  const classified = tryStep('classify', () => classifyForDispatch(detect, oldCache));
  if (classified) {
    result.skipped = classified.skipped;

    // Render agent prompt for each dispatch target
    const template = tryStep('loadPromptTemplate', loadPromptTemplate);
    if (template) {
      for (const sub of classified.dispatch) {
        result.dispatch.push({
          name: sub.name,
          path: sub.path,
          role: sub.role,
          stackSummary: sub.stackSummary || '',
          agentPrompt: renderPrompt(template, sub, registry),
        });
      }
    } else {
      result.errors.push('agent prompt template unavailable; dispatch list empty');
    }
  }

  // Carry forward warnings from sync-detect
  if (Array.isArray(detect.warnings)) {
    result.warnings.push(...detect.warnings);
  }

  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

main();
