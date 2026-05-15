#!/usr/bin/env bun
// <!-- mustard:generated -->
'use strict';
/**
 * skill-orphan-audit — list skills that haven't been invoked in N days.
 *
 * Discovery:
 *   - `<root>/templates/skills/<name>/SKILL.md` (mustard-repo authoring view)
 *   - `<root>/.claude/skills/<name>/SKILL.md` (project root)
 *   - `<root>/<sub>/.claude/skills/<name>/SKILL.md` (one level of subprojects)
 *
 * Invocation source (in priority order):
 *   1. EventStore (`.harness/mustard.db`) when MUSTARD_HARNESS_DUAL_EMIT=1 and
 *      the dist/runtime/event-store.js is reachable. Single SQL query.
 *   2. Fallback: line-by-line scan of `.claude/.harness/events.jsonl`.
 *
 * CLI:
 *   --days N         lookback window (default: env MUSTARD_SKILL_ORPHAN_DAYS or 30)
 *   --json           machine-readable output
 *   --cwd PATH       override CLAUDE_PROJECT_DIR
 *
 * Output (human):
 *   skill-orphan-audit: 5/12 skill(s) orphaned (lookback=30d)
 *     karpathy-guidelines (last invoked: 2026-05-12)
 *     senior-architect    (last invoked: never)
 *
 * Output (--json):
 *   { skills: [...], orphans: [...], lookback_days: N, last_invoked: {name: iso} }
 *
 * Exit: always 0 (audit, never a gate).
 */

const fs = require('node:fs');
const path = require('node:path');

function parseArgs(argv) {
  const out = { days: null, json: false, cwd: null };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const next = argv[i + 1];
    switch (flag) {
      case '--days':
        out.days = Number.parseInt(next, 10); i++; break;
      case '--json':
        out.json = true; break;
      case '--cwd':
        out.cwd = next; i++; break;
      case '-h':
      case '--help':
        process.stdout.write('Usage: skill-orphan-audit [--days N] [--json] [--cwd PATH]\n');
        process.exit(0);
      default:
        break;
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

/** Parse `name:` from the SKILL.md YAML frontmatter. */
function extractSkillName(content) {
  const normalized = content.replace(/\r\n/g, '\n');
  const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
  if (!fm) return null;
  const m = fm[1].match(/^name:\s*(.+)$/m);
  return m ? m[1].trim() : null;
}

/** Read a SKILL.md and return { name, file } or null. */
function loadSkill(skillMdPath) {
  try {
    const content = fs.readFileSync(skillMdPath, 'utf8');
    const fallback = path.basename(path.dirname(skillMdPath));
    const name = extractSkillName(content) || fallback;
    return { name, file: skillMdPath };
  } catch (_) {
    return null;
  }
}

/** Collect SKILL.md files under a `skills/` directory (one level deep). */
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

/** Discover all skill locations: templates/, project root, one level of subprojects. */
function discoverSkills(projectDir) {
  const found = new Map(); // name → { name, file }

  const candidates = [
    path.join(projectDir, 'templates', 'skills'),
    path.join(projectDir, '.claude', 'skills'),
  ];

  // One level of subprojects: <projectDir>/<sub>/.claude/skills
  try {
    for (const e of fs.readdirSync(projectDir, { withFileTypes: true })) {
      if (!e.isDirectory()) continue;
      if (e.name.startsWith('.') || e.name === 'node_modules') continue;
      candidates.push(path.join(projectDir, e.name, '.claude', 'skills'));
    }
  } catch (_) {}

  for (const dir of candidates) {
    for (const md of collectSkillsAt(dir)) {
      const sk = loadSkill(md);
      if (!sk) continue;
      // First occurrence wins; later duplicates ignored (templates/ takes priority by ordering).
      if (!found.has(sk.name)) found.set(sk.name, sk);
    }
  }

  return Array.from(found.values()).sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Query EventStore for `skill.invoked` since `sinceIso`. Returns Map<skillName, lastIso>
 * or null when the store is unavailable. Fail-open: any error → null.
 */
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
      try {
        const p = ev.payload || {};
        skillName = p.skill || null;
      } catch (_) {}
      if (!skillName) continue;
      const prev = last.get(skillName);
      if (!prev || ev.ts > prev) last.set(skillName, ev.ts);
    }
    return last;
  } catch (_) {
    return null;
  }
}

/**
 * Fallback: scan events.jsonl line-by-line. Same return shape as queryEventStore.
 * Fail-open: any read/parse error → empty map (treated as "no invocations seen").
 */
function scanEventsJsonl(projectDir, sinceIso) {
  const last = new Map();
  const file = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
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
  const ms = Date.now() - days * 24 * 60 * 60 * 1000;
  return new Date(ms).toISOString();
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const projectDir = resolveProjectDir(args.cwd);
  const sinceIso = isoNDaysAgo(args.days);

  const skills = discoverSkills(projectDir);

  // Try EventStore first; if it returns null (not just empty), fall back to JSONL.
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
    const payload = {
      skills: skills.map(s => s.name),
      orphans,
      lookback_days: args.days,
      last_invoked: lastInvoked,
    };
    process.stdout.write(JSON.stringify(payload, null, 2) + '\n');
    process.exit(0);
  }

  process.stdout.write(
    `skill-orphan-audit: ${orphans.length}/${skills.length} skill(s) orphaned (lookback=${args.days}d)\n`
  );
  for (const name of orphans.sort()) {
    const ts = lastInvoked[name];
    const date = ts ? ts.slice(0, 10) : 'never';
    process.stdout.write(`  ${name} (last invoked: ${date})\n`);
  }
  process.exit(0);
}

try { main(); }
catch (_) { process.exit(0); }
