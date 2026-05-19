#!/usr/bin/env bun
// <!-- mustard:generated -->
'use strict';
/**
 * skills — unified CLI for the skill-family scripts.
 *
 * Subcommands:
 *   skills validate [flags]   — structural/factual/lines validation (was skill-validate.js)
 *   skills graph    [flags]   — Mermaid / JSON dependency graph     (was skill-graph.js)
 *   skills orphans  [flags]   — orphan-invocation audit             (was skill-orphan-audit.js)
 *
 * With no subcommand or an unknown subcommand, prints usage and exits 0.
 *
 * All flags, output formats, exit codes, and env vars are identical to the
 * original standalone scripts.
 */

const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

// ---------------------------------------------------------------------------
// Shared: discovery helpers (replaces per-script copies — behaviour identical)
// ---------------------------------------------------------------------------

function readJsonSafe(filePath) {
  try { return JSON.parse(fs.readFileSync(filePath, 'utf-8')); }
  catch { return null; }
}

function fsReadSafe(filePath) {
  try { return fs.readFileSync(filePath, 'utf-8'); }
  catch { return ''; }
}

/** Parse `name:` from YAML frontmatter. Tolerates CRLF. */
function extractSkillName(content) {
  const normalized = content.replace(/\r\n/g, '\n');
  const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
  if (!fm) return null;
  const m = fm[1].match(/^name:\s*(.+)$/m);
  return m ? m[1].trim() : null;
}

function stripFrontmatter(content) {
  return content.replace(/\r\n/g, '\n').replace(/^---\n[\s\S]*?\n---\n?/, '');
}

/** Collect SKILL.md paths one level under a skills/ directory. */
function collectSkillsAt(skillsDir) {
  const out = [];
  if (!fs.existsSync(skillsDir)) return out;
  let entries;
  try { entries = fs.readdirSync(skillsDir, { withFileTypes: true }); }
  catch (_) { return out; }
  for (const e of entries) {
    if (!e.isDirectory()) continue;
    const candidate = path.join(skillsDir, e.name, 'SKILL.md');
    if (fs.existsSync(candidate)) out.push(candidate);
  }
  return out;
}

/**
 * Unified skill discovery used by all three subcommands:
 *   - <projectDir>/templates/skills/
 *   - <projectDir>/.claude/skills/
 *   - <projectDir>/<sub>/.claude/skills/   (one level of subprojects)
 *
 * Returns Array<{ name, file, content? }> sorted by name.
 * Set `loadContent=false` for orphan-audit (which only needs name+file).
 */
function discoverSkills(projectDir, { loadContent = true } = {}) {
  const found = new Map();

  const candidates = [
    path.join(projectDir, 'templates', 'skills'),
    path.join(projectDir, '.claude', 'skills'),
  ];
  try {
    for (const e of fs.readdirSync(projectDir, { withFileTypes: true })) {
      if (!e.isDirectory()) continue;
      if (e.name.startsWith('.') || e.name === 'node_modules') continue;
      candidates.push(path.join(projectDir, e.name, '.claude', 'skills'));
    }
  } catch (_) {}

  for (const dir of candidates) {
    for (const md of collectSkillsAt(dir)) {
      let content;
      try { content = fs.readFileSync(md, 'utf8'); } catch (_) { continue; }
      const name = extractSkillName(content) || path.basename(path.dirname(md));
      if (found.has(name)) continue;
      found.set(name, loadContent ? { name, file: md, content } : { name, file: md });
    }
  }
  return Array.from(found.values()).sort((a, b) => a.name.localeCompare(b.name));
}

// ---------------------------------------------------------------------------
// Shared: collect skill dirs (validate + lines modes use this API)
// ---------------------------------------------------------------------------

const ROOT_VALIDATE = path.resolve(__dirname, '..', '..');
const DETECT_CACHE_PATH = path.join(ROOT_VALIDATE, '.claude', '.detect-cache.json');
const REGISTRY_PATH     = path.join(ROOT_VALIDATE, '.claude', 'entity-registry.json');

/** Returns Array<{ dir, label }> — validate's discovery (detect-cache aware). */
function collectSkillDirs() {
  const dirs = [];
  dirs.push({ dir: path.join(ROOT_VALIDATE, '.claude', 'skills'), label: '<root>' });

  const cache = readJsonSafe(DETECT_CACHE_PATH);
  const subs  = cache?.subprojects || [];
  for (const sub of subs) {
    dirs.push({ dir: path.join(ROOT_VALIDATE, sub.path, '.claude', 'skills'), label: sub.name });
  }

  if (subs.length === 0) {
    try {
      const entries = fs.readdirSync(ROOT_VALIDATE, { withFileTypes: true });
      for (const e of entries) {
        if (!e.isDirectory() || e.name.startsWith('.')) continue;
        const candidate = path.join(ROOT_VALIDATE, e.name, '.claude', 'skills');
        if (fs.existsSync(candidate)) dirs.push({ dir: candidate, label: e.name });
      }
    } catch (err) {
      process.stderr.write(`[skills validate] fallback discovery failed: ${err.message}\n`);
    }
  }
  return dirs;
}

/** Alias used by validate internals — wraps collectSkillsAt. */
function collectSkills(skillsDir) { return collectSkillsAt(skillsDir); }

// ---------------------------------------------------------------------------
// SUBCOMMAND: validate  (was skill-validate.js — 100% preserved)
// ---------------------------------------------------------------------------

function runValidate(argv) {
  const JSON_OUT = argv.includes('--json');
  const QUIET    = argv.includes('--quiet');
  const FACTUAL  = argv.includes('--factual');
  const LINES    = argv.includes('--lines');
  const ONLY = (() => {
    const idx = argv.indexOf('--only');
    return idx !== -1 && argv[idx + 1] ? argv[idx + 1] : null;
  })();

  const FACTUAL_MODE = (() => {
    const raw = (process.env.MUSTARD_SKILL_VALIDATE_MODE || 'strict').toLowerCase();
    return (raw === 'warn' || raw === 'off' || raw === 'strict') ? raw : 'strict';
  })();

  const WARN_LINES        = 200;
  const STRICT_WARN_LINES = 400;
  const BLOCK_LINES       = 500;

  const LINES_MODE = (() => {
    const raw = (process.env.MUSTARD_SKILL_VALIDATE_LINES_MODE || 'warn').toLowerCase();
    return (raw === 'warn' || raw === 'off' || raw === 'strict') ? raw : 'warn';
  })();

  // --- helpers ---------------------------------------------------------------

  function validateWithPython(skillPath) {
    const validator = path.join(ROOT_VALIDATE, '.claude', 'skills', 'skill-creator', 'scripts', 'quick_validate.py');
    if (!fs.existsSync(validator)) return { ok: true, skipped: true };
    try {
      const out = execFileSync('python', [validator, skillPath], { encoding: 'utf-8' });
      return { ok: true, output: out };
    } catch (err) {
      return { ok: false, errors: [err.stdout || err.message] };
    }
  }

  function validateSkill(content) {
    const errors = [];
    const normalized = content.replace(/\r\n/g, '\n');
    const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
    if (!fm) { errors.push('missing YAML frontmatter'); return { ok: false, errors, source: null }; }

    const body       = fm[1];
    const nameMatch  = body.match(/^name:\s*(.+)$/m);
    const descMatch  = body.match(/^description:\s*(?:"([\s\S]+?)"|([^\n]+(?:\n\s+[^\n]+)*))$/m);
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
      if (raw.length < 50)  errors.push(`description too short (${raw.length} chars, min 50)`);
      if (raw.length > 600) errors.push(`description too long (${raw.length} chars, max 600)`);
      if (!/\b(use when|when the user|add|create|new|detect|check|write|even if)\b/i.test(raw)) {
        errors.push('description lacks trigger words (use when / when / add / create / ...)');
      }
    }

    if (!sourceMatch) errors.push('frontmatter: missing "source" (expected scan|manual)');

    return { ok: errors.length === 0, errors, source: sourceMatch ? sourceMatch[1] : null };
  }

  function slugify(s) {
    if (!s) return '';
    return String(s)
      .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
      .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '');
  }

  function loadClusterIndex() {
    const registry = readJsonSafe(REGISTRY_PATH);
    const suffixes = new Set();
    const rawClusters = [];
    if (!registry || !registry._patterns) return { suffixes, rawClusters };
    for (const stack of Object.keys(registry._patterns)) {
      const disc = registry._patterns[stack]?.discovered || [];
      for (const c of disc) {
        if (typeof c !== 'object' || c == null) continue;
        const fc = typeof c.fileCount === 'number' ? c.fileCount : 0;
        if (fc < 3) continue;
        const sfx = slugify(c.suffix || c.label || '');
        if (sfx) suffixes.add(sfx);
        rawClusters.push({ ...c, _stack: stack, _slugSuffix: sfx });
      }
    }
    return { suffixes, rawClusters };
  }

  function extractSamplePaths(content) {
    const normalized = content.replace(/\r\n/g, '\n');
    const lines = normalized.split('\n');
    const paths = [];
    let inSection = false;
    const sectionRe = /^##\s+(Real examples|Samples in this project|Real examples in this codebase)/i;
    for (const line of lines) {
      if (/^##\s+/.test(line)) { inSection = sectionRe.test(line); continue; }
      if (!inSection) continue;
      const m = line.match(/^\s*-\s+.*?`([^`]+)`/);
      if (m && m[1]) {
        const candidate = m[1].trim();
        const looksLikePath = /[/\\]/.test(candidate) || /\.[a-zA-Z0-9]{1,6}$/.test(candidate);
        if (candidate && !/^https?:/i.test(candidate) && looksLikePath) paths.push(candidate);
      }
    }
    return paths;
  }

  function countFencedBlocks(content) {
    const normalized = content.replace(/\r\n/g, '\n');
    const stripped = normalized.replace(/^---\n[\s\S]*?\n---\n?/, '');
    const matches = stripped.match(/^```/gm);
    return matches ? matches.length : 0;
  }

  function extractReferenceSources(content) {
    const normalized = content.replace(/\r\n/g, '\n');
    const out = [];
    const re = /^\s*Source:\s*`([^`]+)`/gm;
    let m;
    while ((m = re.exec(normalized)) !== null) { if (m[1]) out.push(m[1].trim()); }
    return out;
  }

  function pathExistsUnderRoot(p, subprojectRoot) {
    if (!p) return false;
    try {
      if (path.isAbsolute(p)) return fs.existsSync(p);
      if (subprojectRoot && fs.existsSync(path.resolve(subprojectRoot, p))) return true;
      return fs.existsSync(path.resolve(ROOT_VALIDATE, p));
    } catch { return false; }
  }

  function factualCheckSkill(skillPath, clusterIndex, subprojectRoot) {
    const violations = [];
    const warnings = [];
    let content;
    try { content = fs.readFileSync(skillPath, 'utf-8'); }
    catch (err) {
      process.stderr.write(`[skills validate] unreadable: ${skillPath}: ${err.message}\n`);
      return { gated: true, violations, warnings };
    }

    if (!/<!--\s*mustard:generated/.test(content)) return { gated: true, violations, warnings };

    const samples = extractSamplePaths(content);
    let validSampleCount = 0;
    for (const p of samples) {
      if (pathExistsUnderRoot(p, subprojectRoot)) validSampleCount++;
      else violations.push({ code: 'STALE_SAMPLE', detail: p });
    }

    const fenceCount = countFencedBlocks(content);
    if (fenceCount > 0) violations.push({ code: 'CODE_IN_BODY', detail: `${fenceCount} fenced code block(s)` });

    let validReferenceCount = 0;
    const refsFile = path.join(path.dirname(skillPath), 'references', 'examples.md');
    if (fs.existsSync(refsFile)) {
      let refsContent;
      try { refsContent = fs.readFileSync(refsFile, 'utf-8'); }
      catch (err) { process.stderr.write(`[skills validate] unreadable references: ${refsFile}: ${err.message}\n`); refsContent = null; }
      if (refsContent) {
        const sources = extractReferenceSources(refsContent);
        for (const p of sources) {
          if (pathExistsUnderRoot(p, subprojectRoot)) validReferenceCount++;
          else violations.push({ code: 'STALE_REFERENCE', detail: p });
        }
      }
    }

    const name = extractSkillName(content);
    if (name) {
      const rawParts = name.split('-').filter(Boolean);
      const parts = rawParts[rawParts.length - 1] === 'pattern' ? rawParts.slice(0, -1) : rawParts;
      if (parts.length >= 1) {
        let matched = false;
        outer: for (let len = parts.length; len >= 1; len--) {
          for (let start = 0; start <= parts.length - len; start++) {
            const candidate = parts.slice(start, start + len).join('-');
            if (clusterIndex.suffixes.has(candidate)) { matched = true; break outer; }
          }
        }
        if (!matched && clusterIndex.suffixes.has(name)) matched = true;
        if (!matched && clusterIndex.suffixes.size > 0) {
          const hasBackingEvidence = validSampleCount + validReferenceCount > 0;
          const entry = { code: 'NO_CLUSTER', detail: `skill name "${name}" does not match any _patterns[*].discovered[].suffix with fileCount >= 3` };
          if (hasBackingEvidence) warnings.push(entry);
          else violations.push(entry);
        }
      }
    }

    return { gated: false, violations, warnings };
  }

  // --- factual mode ----------------------------------------------------------

  function runFactualMode() {
    if (FACTUAL_MODE === 'off') {
      const payload = { mode: 'off', total: 0, violations: [] };
      if (JSON_OUT) process.stdout.write(JSON.stringify(payload, null, 2) + '\n');
      else console.log('skill-validate (factual): mode=off, skipping.');
      process.exit(0);
    }

    let clusterIndex = { suffixes: new Set(), rawClusters: [] };
    try { clusterIndex = loadClusterIndex(); }
    catch (err) { process.stderr.write(`[skills validate] cluster index load failed: ${err.message}\n`); }

    const locations = collectSkillDirs();
    let total = 0;
    const violationsAll = [];
    const warningsAll   = [];

    for (const { dir, label } of locations) {
      let files = [];
      try { files = collectSkills(dir); }
      catch (err) { process.stderr.write(`[skills validate] collect failed for ${dir}: ${err.message}\n`); continue; }
      const subprojectRoot = path.dirname(path.dirname(dir));
      for (const file of files) {
        total++;
        let result;
        try { result = factualCheckSkill(file, clusterIndex, subprojectRoot); }
        catch (err) { process.stderr.write(`[skills validate] check failed for ${file}: ${err.message}\n`); continue; }
        if (result.gated) continue;
        const rel = path.relative(ROOT_VALIDATE, file).replace(/\\/g, '/');
        const skillName = extractSkillName(fsReadSafe(file)) || path.basename(path.dirname(file));
        for (const v of result.violations) violationsAll.push({ skill: skillName, file: rel, code: v.code, detail: v.detail, location: label });
        for (const w of result.warnings || []) warningsAll.push({ skill: skillName, file: rel, code: w.code, detail: w.detail, location: label });
      }
    }

    if (JSON_OUT) {
      process.stdout.write(JSON.stringify({ mode: FACTUAL_MODE, total, violations: violationsAll, warnings: warningsAll }, null, 2) + '\n');
    } else {
      const groupBy = (items) => { const m = new Map(); for (const it of items) { if (!m.has(it.file)) m.set(it.file, []); m.get(it.file).push(it); } return m; };
      if (violationsAll.length === 0 && warningsAll.length === 0) {
        console.log(`skill-validate (factual): ${total} skill(s) scanned, 0 violations. mode=${FACTUAL_MODE}`);
      } else {
        console.log(`skill-validate (factual): ${violationsAll.length} violation(s), ${warningsAll.length} warning(s) across ${total} skill(s). mode=${FACTUAL_MODE}`);
        if (violationsAll.length > 0) {
          for (const [file, vs] of groupBy(violationsAll)) { console.log(`  ${file}`); for (const v of vs) console.log(`    [${v.code}] ${v.detail}`); }
        }
        if (warningsAll.length > 0) {
          console.log('  --- warnings (non-blocking) ---');
          for (const [file, ws] of groupBy(warningsAll)) { console.log(`  ${file}`); for (const w of ws) console.log(`    [WARN ${w.code}] ${w.detail}`); }
        }
      }
    }

    if (FACTUAL_MODE === 'warn') process.exit(0);
    process.exit(violationsAll.length > 0 ? 1 : 0);
  }

  // --- lines mode ------------------------------------------------------------

  function linesTier(lineCount) {
    if (lineCount >= BLOCK_LINES)       return 'block';
    if (lineCount >= STRICT_WARN_LINES) return 'strict-warn';
    if (lineCount >= WARN_LINES)        return 'warn';
    return 'ok';
  }

  function runLinesMode() {
    if (LINES_MODE === 'off') {
      if (JSON_OUT) process.stdout.write(JSON.stringify({ mode: 'off', total: 0, results: [] }, null, 2) + '\n');
      else console.log('skill-validate (lines): mode=off, skipping.');
      process.exit(0);
    }

    const locations = collectSkillDirs();
    const results = [];
    let hasBlock = false;

    for (const { dir, label } of locations) {
      let files = [];
      try { files = collectSkills(dir); }
      catch (err) { process.stderr.write(`[skills validate] collect failed for ${dir}: ${err.message}\n`); continue; }
      for (const file of files) {
        const content = fsReadSafe(file);
        const lineCount = content ? content.split('\n').length : 0;
        const tier = linesTier(lineCount);
        if (tier === 'block') hasBlock = true;
        const rel = path.relative(ROOT_VALIDATE, file).replace(/\\/g, '/');
        results.push({ file: rel, lineCount, tier, location: label });
      }
    }

    if (JSON_OUT) {
      process.stdout.write(JSON.stringify({ mode: LINES_MODE, total: results.length, results }, null, 2) + '\n');
    } else {
      const notable = results.filter(r => r.tier !== 'ok');
      if (notable.length === 0) console.log(`skill-validate (lines): ${results.length} skill(s) scanned, all within thresholds. mode=${LINES_MODE}`);
      else {
        console.log(`skill-validate (lines): ${notable.length} skill(s) above threshold. mode=${LINES_MODE}`);
        for (const r of notable) console.log(`  [LINES] ${r.file}: ${r.lineCount} lines — tier=${r.tier}`);
      }
    }

    if (LINES_MODE === 'strict' && hasBlock) process.exit(1);
    process.exit(0);
  }

  // --- structural mode (default) --------------------------------------------

  function runStructural() {
    const locations = collectSkillDirs();
    const results = [];

    for (const { dir, label } of locations) {
      const files = collectSkills(dir);
      for (const file of files) {
        let content;
        try { content = fs.readFileSync(file, 'utf-8'); }
        catch { results.push({ location: label, path: file, ok: false, errors: ['unreadable'], source: null }); continue; }
        const res = validateSkill(content);
        if (ONLY && res.source && res.source !== ONLY) continue;
        results.push({ location: label, path: path.relative(ROOT_VALIDATE, file).replace(/\\/g, '/'), ok: res.ok, errors: res.errors, source: res.source });
      }
    }

    const failures = results.filter(r => !r.ok);
    const summary  = { total: results.length, ok: results.length - failures.length, failed: failures.length };

    if (JSON_OUT) {
      process.stdout.write(JSON.stringify({ summary, results }, null, 2) + '\n');
    } else {
      const rowsToShow = QUIET ? failures : results;
      if (!rowsToShow.length) {
        console.log('skill-validate: no SKILL.md files found.');
      } else {
        console.log('skill-validate:');
        for (const r of rowsToShow) {
          const tag  = r.ok ? '[ok]  ' : '[fail]';
          const errs = r.errors.length ? ` — ${r.errors.join('; ')}` : '';
          console.log(`  ${tag} ${r.path}${errs}`);
        }
      }
      console.log(`\nskill-validate: ${summary.ok}/${summary.total} ok, ${summary.failed} failed.`);
    }

    process.exit(failures.length > 0 ? 2 : 0);
  }

  // --- self-test mode --------------------------------------------------------

  if (process.env.MUSTARD_SKILL_VALIDATE_SELFTEST === '1') {
    const testSuffixes = new Set(['alpha', 'beta', 'gamma', 'delta', 'compound-word']);
    const testIndex = { suffixes: testSuffixes, rawClusters: [] };
    function testMatch(skillName) {
      const rawParts = skillName.split('-').filter(Boolean);
      const parts = rawParts[rawParts.length - 1] === 'pattern' ? rawParts.slice(0, -1) : rawParts;
      if (parts.length < 1) return false;
      for (let len = parts.length; len >= 1; len--) {
        for (let start = 0; start <= parts.length - len; start++) {
          const candidate = parts.slice(start, start + len).join('-');
          if (testIndex.suffixes.has(candidate)) return true;
        }
      }
      return testIndex.suffixes.has(skillName);
    }
    const cases = [
      { name: 'sub-alpha-pattern',         expect: true  },
      { name: 'sub-beta-pattern',          expect: true  },
      { name: 'sub-compound-word-pattern', expect: true  },
      { name: 'sub-unknown-pattern',       expect: false },
    ];
    let allPassed = true;
    for (const { name: skillName, expect } of cases) {
      const got = testMatch(skillName);
      const status = got === expect ? 'PASS' : 'FAIL';
      if (got !== expect) allPassed = false;
      const label = expect ? 'MATCH' : 'NO_CLUSTER';
      process.stdout.write(`[self-test] ${status}  ${skillName} → ${got ? 'MATCH' : 'NO_CLUSTER'} (expected: ${label})\n`);
    }
    process.exit(allPassed ? 0 : 1);
  }

  // --- dispatch --------------------------------------------------------------

  if (FACTUAL) { runFactualMode(); return; }
  if (LINES)   { runLinesMode();   return; }
  runStructural();
}

// ---------------------------------------------------------------------------
// SUBCOMMAND: graph  (was skill-graph.js — 100% preserved)
// ---------------------------------------------------------------------------

function runGraph(argv) {
  function parseArgs(args) {
    const out = { json: false, cwd: null };
    for (let i = 0; i < args.length; i++) {
      const flag = args[i]; const next = args[i + 1];
      switch (flag) {
        case '--json': out.json = true; break;
        case '--cwd':  out.cwd = next; i++; break;
        case '-h': case '--help':
          process.stdout.write('Usage: skills graph [--json] [--cwd PATH]\n');
          process.exit(0);
      }
    }
    return out;
  }

  function resolveProjectDir(override) {
    if (override) return path.resolve(override);
    if (process.env.CLAUDE_PROJECT_DIR) return process.env.CLAUDE_PROJECT_DIR;
    return process.cwd();
  }

  function escapeRegex(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

  function findReferences(body, self, known) {
    const refs = new Set();
    for (const candidate of known) {
      if (candidate === self) continue;
      const esc = escapeRegex(candidate);
      const re  = new RegExp(`\\[\\[${esc}\\]\\]|Skill\\(${esc}\\)|\\b${esc}\\b`);
      if (re.test(body)) refs.add(candidate);
    }
    return refs;
  }

  function buildGraph(skills) {
    const known = new Set(skills.map(s => s.name));
    const adj   = new Map();
    for (const sk of skills) {
      const body = stripFrontmatter(sk.content);
      const refs = findReferences(body, sk.name, known);
      adj.set(sk.name, Array.from(refs).sort());
    }
    return adj;
  }

  function findCycles(adj) {
    const cycles = [];
    const seen   = new Set();
    const WHITE  = 0, GRAY = 1, BLACK = 2;
    const color  = new Map();
    for (const node of adj.keys()) color.set(node, WHITE);

    function canonical(cycle) {
      const ring = cycle.slice(0, -1);
      let minIdx = 0;
      for (let i = 1; i < ring.length; i++) { if (ring[i] < ring[minIdx]) minIdx = i; }
      const rotated = ring.slice(minIdx).concat(ring.slice(0, minIdx));
      return rotated.join('>');
    }

    function dfs(node, stack) {
      color.set(node, GRAY); stack.push(node);
      for (const next of adj.get(node) || []) {
        const c = color.get(next);
        if (c === GRAY) {
          const idx = stack.indexOf(next);
          if (idx !== -1) { const cyc = stack.slice(idx).concat([next]); const key = canonical(cyc); if (!seen.has(key)) { seen.add(key); cycles.push(cyc); } }
        } else if (c === WHITE) { dfs(next, stack); }
      }
      stack.pop(); color.set(node, BLACK);
    }

    for (const node of Array.from(adj.keys()).sort()) { if (color.get(node) === WHITE) dfs(node, []); }
    return cycles;
  }

  function nodeId(name) { return 'skill_' + name; }

  function renderMermaid(skills, adj, cycles) {
    const lines = ['graph TD'];
    for (const cyc of cycles) lines.push('  %% cycle detected: ' + cyc.join(' -> '));
    for (const sk of skills)  lines.push(`  ${nodeId(sk.name)}["${sk.name}"]`);
    for (const sk of skills) { const outs = adj.get(sk.name) || []; for (const to of outs) lines.push(`  ${nodeId(sk.name)} --> ${nodeId(to)}`); }
    return lines.join('\n') + '\n';
  }

  function renderJson(skills, adj, cycles) {
    const nodes = skills.map(s => s.name);
    const edges = [];
    for (const sk of skills) { for (const to of adj.get(sk.name) || []) edges.push({ from: sk.name, to }); }
    return JSON.stringify({ nodes, edges, cycles }, null, 2) + '\n';
  }

  const args       = parseArgs(argv);
  const projectDir = resolveProjectDir(args.cwd);
  const skills     = discoverSkills(projectDir);
  const adj        = buildGraph(skills);
  const cycles     = findCycles(adj);

  if (args.json) process.stdout.write(renderJson(skills, adj, cycles));
  else           process.stdout.write(renderMermaid(skills, adj, cycles));
  process.exit(0);
}

// ---------------------------------------------------------------------------
// SUBCOMMAND: orphans  (was skill-orphan-audit.js — 100% preserved)
// ---------------------------------------------------------------------------

function runOrphans(argv) {
  function parseArgs(args) {
    const out = { days: null, json: false, cwd: null };
    for (let i = 0; i < args.length; i++) {
      const flag = args[i]; const next = args[i + 1];
      switch (flag) {
        case '--days': out.days = Number.parseInt(next, 10); i++; break;
        case '--json': out.json = true; break;
        case '--cwd':  out.cwd  = next; i++; break;
        case '-h': case '--help':
          process.stdout.write('Usage: skills orphans [--days N] [--json] [--cwd PATH]\n');
          process.exit(0);
      }
    }
    if (!Number.isFinite(out.days) || out.days <= 0) {
      const envDays = Number.parseInt(process.env.MUSTARD_SKILL_ORPHAN_DAYS || '', 10);
      out.days = Number.isFinite(envDays) && envDays > 0 ? envDays : 30;
    }
    return out;
  }

  function resolveProjectDir(override) {
    if (override) return path.resolve(override);
    if (process.env.CLAUDE_PROJECT_DIR) return process.env.CLAUDE_PROJECT_DIR;
    return process.cwd();
  }

  function queryEventStore(projectDir, sinceIso) {
    try {
      const wrapper = path.join(projectDir, '.claude', 'hooks', '_lib', 'event-store.js');
      if (!fs.existsSync(wrapper)) return null;
      const { getStore } = require(wrapper);
      const claudeDir = path.join(projectDir, '.claude');
      const store = getStore(claudeDir);
      if (!store || typeof store.query !== 'function') return null;
      const events = store.query({ event: 'skill.invoked', since: sinceIso });
      const last = new Map();
      for (const ev of events) {
        let skillName = null;
        try { const p = ev.payload || {}; skillName = p.skill || null; } catch (_) {}
        if (!skillName) continue;
        const prev = last.get(skillName);
        if (!prev || ev.ts > prev) last.set(skillName, ev.ts);
      }
      return last;
    } catch (_) { return null; }
  }

  function scanEventsJsonl(projectDir, sinceIso) {
    const last = new Map();
    const file  = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
    if (!fs.existsSync(file)) return last;
    let raw;
    try { raw = fs.readFileSync(file, 'utf8'); } catch (_) { return last; }
    for (const line of raw.split('\n')) {
      if (!line) continue;
      let ev;
      try { ev = JSON.parse(line); } catch (_) { continue; }
      if (!ev || ev.event !== 'skill.invoked') continue;
      if (sinceIso && ev.ts < sinceIso) continue;
      const skillName = ev.payload && ev.payload.skill;
      if (!skillName) continue;
      const prev = last.get(skillName);
      if (!prev || ev.ts > prev) last.set(skillName, ev.ts);
    }
    return last;
  }

  function isoNDaysAgo(days) {
    return new Date(Date.now() - days * 24 * 60 * 60 * 1000).toISOString();
  }

  const args       = parseArgs(argv);
  const projectDir = resolveProjectDir(args.cwd);
  const sinceIso   = isoNDaysAgo(args.days);

  // discoverSkills with loadContent=false is sufficient (name+file only)
  const skills = discoverSkills(projectDir, { loadContent: false });

  let invocations = queryEventStore(projectDir, sinceIso);
  if (invocations == null) invocations = scanEventsJsonl(projectDir, sinceIso);

  const orphans = [];
  const lastInvoked = {};
  for (const sk of skills) {
    const ts = invocations.get(sk.name);
    if (ts) lastInvoked[sk.name] = ts;
    else orphans.push(sk.name);
  }

  if (args.json) {
    process.stdout.write(JSON.stringify({ skills: skills.map(s => s.name), orphans, lookback_days: args.days, last_invoked: lastInvoked }, null, 2) + '\n');
    process.exit(0);
  }

  process.stdout.write(`skill-orphan-audit: ${orphans.length}/${skills.length} skill(s) orphaned (lookback=${args.days}d)\n`);
  for (const name of orphans.sort()) {
    const ts   = lastInvoked[name];
    const date = ts ? ts.slice(0, 10) : 'never';
    process.stdout.write(`  ${name} (last invoked: ${date})\n`);
  }
  process.exit(0);
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

const SUBCMDS = {
  validate: runValidate,
  graph:    runGraph,
  orphans:  runOrphans,
};

const USAGE = `Usage: skills <subcommand> [flags]

Subcommands:
  validate [--json] [--quiet] [--factual] [--lines] [--only scan|manual]
  graph    [--json] [--cwd PATH]
  orphans  [--days N] [--json] [--cwd PATH]
`;

function main() {
  const argv = process.argv.slice(2);
  const subcmd = argv[0];
  const handler = SUBCMDS[subcmd];
  if (!handler) {
    process.stdout.write(USAGE);
    process.exit(0);
  }
  handler(argv.slice(1));
}

if (require.main === module) {
  try { main(); }
  catch (err) {
    process.stderr.write(`[skills] Fatal error: ${err.message}\n${err.stack}\n`);
    process.exit(0);
  }
}

// Module exports — skill-validate-gate.js loads this file via loadValidator()
// and calls validateSkill(content) to validate SKILL.md writes against the same
// rules as the `validate` subcommand.
module.exports = {
  validateSkill: (() => {
    // Inline closure so the export works without runValidate's local scope.
    return function validateSkill(content) {
      const errors = [];
      const normalized = content.replace(/\r\n/g, '\n');
      const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
      if (!fm) { errors.push('missing YAML frontmatter'); return { ok: false, errors, source: null }; }
      const body = fm[1];
      const nameMatch  = body.match(/^name:\s*(.+)$/m);
      const descMatch  = body.match(/^description:\s*(?:"([\s\S]+?)"|([^\n]+(?:\n\s+[^\n]+)*))$/m);
      const sourceMatch = body.match(/^source:\s*(scan|manual)$/m);
      if (!nameMatch) { errors.push('frontmatter: missing "name"'); }
      else if (!/^[a-z][a-z0-9-]+$/.test(nameMatch[1].trim())) { errors.push(`name not kebab-case: ${nameMatch[1]}`); }
      if (!descMatch) { errors.push('frontmatter: missing "description"'); }
      else {
        const raw = (descMatch[1] || descMatch[2] || '').replace(/\s+/g, ' ').trim();
        if (raw.length < 50)  errors.push(`description too short (${raw.length} chars, min 50)`);
        if (raw.length > 600) errors.push(`description too long (${raw.length} chars, max 600)`);
        if (!/\b(use when|when the user|add|create|new|detect|check|write|even if)\b/i.test(raw)) errors.push('description lacks trigger words (use when / when / add / create / ...)');
      }
      if (!sourceMatch) errors.push('frontmatter: missing "source" (expected scan|manual)');
      return { ok: errors.length === 0, errors, source: sourceMatch ? sourceMatch[1] : null };
    };
  })(),
  collectSkills: collectSkillsAt,
  collectSkillDirs,
  discoverSkills,
  extractSkillName,
  extractSamplePaths: (() => {
    return function extractSamplePaths(content) {
      const normalized = content.replace(/\r\n/g, '\n');
      const lines = normalized.split('\n');
      const paths = [];
      let inSection = false;
      const sectionRe = /^##\s+(Real examples|Samples in this project|Real examples in this codebase)/i;
      for (const line of lines) {
        if (/^##\s+/.test(line)) { inSection = sectionRe.test(line); continue; }
        if (!inSection) continue;
        const m = line.match(/^\s*-\s+.*?`([^`]+)`/);
        if (m && m[1]) {
          const candidate = m[1].trim();
          const looksLikePath = /[/\\]/.test(candidate) || /\.[a-zA-Z0-9]{1,6}$/.test(candidate);
          if (candidate && !/^https?:/i.test(candidate) && looksLikePath) paths.push(candidate);
        }
      }
      return paths;
    };
  })(),
  extractReferenceSources: (() => {
    return function extractReferenceSources(content) {
      const normalized = content.replace(/\r\n/g, '\n');
      const out = [];
      const re  = /^\s*Source:\s*`([^`]+)`/gm;
      let m;
      while ((m = re.exec(normalized)) !== null) { if (m[1]) out.push(m[1].trim()); }
      return out;
    };
  })(),
  countFencedBlocks: (() => {
    return function countFencedBlocks(content) {
      const stripped = content.replace(/\r\n/g, '\n').replace(/^---\n[\s\S]*?\n---\n?/, '');
      const matches  = stripped.match(/^```/gm);
      return matches ? matches.length : 0;
    };
  })(),
  slugify: (() => {
    return function slugify(s) {
      if (!s) return '';
      return String(s).replace(/([a-z0-9])([A-Z])/g, '$1-$2').replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
    };
  })(),
};
