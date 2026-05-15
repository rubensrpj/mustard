#!/usr/bin/env bun
// <!-- mustard:generated -->
'use strict';
/**
 * skill-graph — emit a Mermaid graph (or JSON) of skill ↔ skill references.
 *
 * Discovery: same as skill-orphan-audit.js — templates/, .claude/, one level of
 * subprojects' .claude/skills/.
 *
 * Reference extraction (conservative; the source set is the discovered names):
 *   1. `[[skill-name]]`        — wiki-style link
 *   2. `Skill(skill-name)`     — programmatic invocation
 *   3. bare `\bskill-name\b`   — accepted only when `skill-name` is in the set
 *
 * Self-references are skipped. Edges deduplicated.
 *
 * Output (default): Mermaid `graph TD` to stdout. If any cycle is detected, a
 * `%% cycle detected: a -> b -> c -> a` comment line is emitted above the graph
 * (one comment per cycle).
 *
 * --json: { nodes: [name…], edges: [{from, to}…], cycles: [[a,b,…,a]…] }
 *
 * Exit: always 0.
 */

const fs = require('node:fs');
const path = require('node:path');

function parseArgs(argv) {
  const out = { json: false, cwd: null };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const next = argv[i + 1];
    switch (flag) {
      case '--json': out.json = true; break;
      case '--cwd': out.cwd = next; i++; break;
      case '-h':
      case '--help':
        process.stdout.write('Usage: skill-graph [--json] [--cwd PATH]\n');
        process.exit(0);
      default: break;
    }
  }
  return out;
}

function resolveProjectDir(override) {
  if (override) return path.resolve(override);
  if (process.env.CLAUDE_PROJECT_DIR) return process.env.CLAUDE_PROJECT_DIR;
  return process.cwd();
}

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

function discoverSkills(projectDir) {
  const found = new Map(); // name → { name, file, content }
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
      found.set(name, { name, file: md, content });
    }
  }
  return Array.from(found.values()).sort((a, b) => a.name.localeCompare(b.name));
}

/** Escape a string for use inside a regex. */
function escapeRegex(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * Return Set<string> of skill names referenced from `body`. Excludes `self`.
 * @param {string} body  SKILL.md body (frontmatter stripped)
 * @param {string} self  name of the skill that owns this body
 * @param {Set<string>} known  set of all discovered skill names
 */
function findReferences(body, self, known) {
  const refs = new Set();
  for (const candidate of known) {
    if (candidate === self) continue;
    const esc = escapeRegex(candidate);
    // [[name]] OR Skill(name) OR \bname\b
    const re = new RegExp(`\\[\\[${esc}\\]\\]|Skill\\(${esc}\\)|\\b${esc}\\b`);
    if (re.test(body)) refs.add(candidate);
  }
  return refs;
}

/**
 * Build adjacency list { name → [outgoing names] }. Edges sorted alphabetically
 * inside each adjacency list for deterministic output.
 */
function buildGraph(skills) {
  const known = new Set(skills.map(s => s.name));
  const adj = new Map();
  for (const sk of skills) {
    const body = stripFrontmatter(sk.content);
    const refs = findReferences(body, sk.name, known);
    adj.set(sk.name, Array.from(refs).sort());
  }
  return adj;
}

/**
 * DFS-based cycle detection. Returns a list of cycles, each represented as
 * [a, b, …, a]. Each distinct cycle is reported once (canonicalised by rotation
 * starting from the alphabetically-smallest node).
 */
function findCycles(adj) {
  const cycles = [];
  const seen = new Set(); // canonical-string → dedup
  const WHITE = 0, GRAY = 1, BLACK = 2;
  const color = new Map();
  for (const node of adj.keys()) color.set(node, WHITE);

  function canonical(cycle) {
    // cycle is [a, b, ..., a] — drop trailing dup, rotate to start at min
    const ring = cycle.slice(0, -1);
    let minIdx = 0;
    for (let i = 1; i < ring.length; i++) {
      if (ring[i] < ring[minIdx]) minIdx = i;
    }
    const rotated = ring.slice(minIdx).concat(ring.slice(0, minIdx));
    return rotated.join('>');
  }

  function dfs(node, stack) {
    color.set(node, GRAY);
    stack.push(node);
    for (const next of adj.get(node) || []) {
      const c = color.get(next);
      if (c === GRAY) {
        // back-edge → cycle from `next` index in stack to end
        const idx = stack.indexOf(next);
        if (idx !== -1) {
          const cyc = stack.slice(idx).concat([next]);
          const key = canonical(cyc);
          if (!seen.has(key)) { seen.add(key); cycles.push(cyc); }
        }
      } else if (c === WHITE) {
        dfs(next, stack);
      }
    }
    stack.pop();
    color.set(node, BLACK);
  }

  for (const node of Array.from(adj.keys()).sort()) {
    if (color.get(node) === WHITE) dfs(node, []);
  }
  return cycles;
}

/** Mermaid node IDs: hyphens are valid; just prefix to avoid clashing with reserved words. */
function nodeId(name) {
  return 'skill_' + name;
}

function renderMermaid(skills, adj, cycles) {
  // `graph TD` must be the first line (AC-14: `skill-graph | head -1` == "graph TD").
  // Cycle comments sit immediately under the header — still valid Mermaid, still
  // co-located with the graph they describe.
  const lines = ['graph TD'];
  for (const cyc of cycles) {
    lines.push('  %% cycle detected: ' + cyc.join(' -> '));
  }
  for (const sk of skills) {
    lines.push(`  ${nodeId(sk.name)}["${sk.name}"]`);
  }
  // Edges in stable order
  for (const sk of skills) {
    const outs = adj.get(sk.name) || [];
    for (const to of outs) {
      lines.push(`  ${nodeId(sk.name)} --> ${nodeId(to)}`);
    }
  }
  return lines.join('\n') + '\n';
}

function renderJson(skills, adj, cycles) {
  const nodes = skills.map(s => s.name);
  const edges = [];
  for (const sk of skills) {
    for (const to of adj.get(sk.name) || []) edges.push({ from: sk.name, to });
  }
  return JSON.stringify({ nodes, edges, cycles }, null, 2) + '\n';
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const projectDir = resolveProjectDir(args.cwd);
  const skills = discoverSkills(projectDir);
  const adj = buildGraph(skills);
  const cycles = findCycles(adj);

  if (args.json) process.stdout.write(renderJson(skills, adj, cycles));
  else process.stdout.write(renderMermaid(skills, adj, cycles));
  process.exit(0);
}

try { main(); }
catch (_) { process.exit(0); }
