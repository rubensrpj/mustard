#!/usr/bin/env node
'use strict';

/**
 * skill-validate.js
 *
 * Validates SKILL.md files across the project:
 *   - ROOT `.claude/skills/` (registry-generated role skills)
 *   - Each subproject's `{sub}/.claude/skills/` (agent-generated step 4.6 skills)
 *
 * Runs pure validation (YAML frontmatter, kebab-case name, description length/triggers,
 * `source: scan|manual` field). Does NOT write/modify anything.
 *
 * Usage:
 *   node .claude/scripts/skill-validate.js                   # validate all
 *   node .claude/scripts/skill-validate.js --json            # JSON output
 *   node .claude/scripts/skill-validate.js --only scan       # skip manual skills
 *   node .claude/scripts/skill-validate.js --quiet           # only show failures
 *
 * Exit codes:
 *   0 — all skills valid (or none found)
 *   2 — at least one validation failure (non-zero, but never crashes host)
 */

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..', '..');
const DETECT_CACHE_PATH = path.join(ROOT, '.claude', '.detect-cache.json');

const args = process.argv.slice(2);
const JSON_OUT = args.includes('--json');
const QUIET = args.includes('--quiet');
const ONLY = (() => {
  const idx = args.indexOf('--only');
  return idx !== -1 && args[idx + 1] ? args[idx + 1] : null;
})();

function readJsonSafe(filePath) {
  try { return JSON.parse(fs.readFileSync(filePath, 'utf-8')); } catch { return null; }
}

/**
 * Attempt to validate a skill file using the skill-creator Python validator if available.
 * Fail-open: returns { ok: true, skipped: true } when Python or skill-creator is absent.
 * @param {string} skillPath - absolute path to SKILL.md
 * @returns {{ ok: boolean, skipped?: boolean, output?: string, errors?: string[] }}
 */
function validateWithPython(skillPath) {
  const validator = path.join(ROOT, '.claude', 'skills', 'skill-creator', 'scripts', 'quick_validate.py');
  if (!fs.existsSync(validator)) return { ok: true, skipped: true };
  try {
    const out = execFileSync('python', [validator, skillPath], { encoding: 'utf-8' });
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
  const normalized = content.replace(/\r\n/g, '\n');
  const fm = normalized.match(/^---\n([\s\S]*?)\n---/);
  if (!fm) {
    errors.push('missing YAML frontmatter');
    return { ok: false, errors, source: null };
  }

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
  try { entries = fs.readdirSync(skillsDir, { withFileTypes: true }); } catch { return []; }
  const out = [];
  for (const e of entries) {
    if (!e.isDirectory()) continue;
    const candidate = path.join(skillsDir, e.name, 'SKILL.md');
    if (fs.existsSync(candidate)) out.push(candidate);
  }
  return out;
}

/**
 * Build the list of skill directories to validate:
 *   - ROOT/.claude/skills
 *   - every subproject/.claude/skills from detect cache
 * @returns {Array<{ dir: string, label: string }>}
 */
function collectSkillDirs() {
  const dirs = [];
  dirs.push({ dir: path.join(ROOT, '.claude', 'skills'), label: '<root>' });

  const cache = readJsonSafe(DETECT_CACHE_PATH);
  const subs = cache?.subprojects || [];
  for (const sub of subs) {
    const p = path.join(ROOT, sub.path, '.claude', 'skills');
    dirs.push({ dir: p, label: sub.name });
  }
  return dirs;
}

function main() {
  const locations = collectSkillDirs();
  const results = [];

  for (const { dir, label } of locations) {
    const files = collectSkills(dir);
    for (const file of files) {
      let content;
      try { content = fs.readFileSync(file, 'utf-8'); } catch {
        results.push({ location: label, path: file, ok: false, errors: ['unreadable'], source: null });
        continue;
      }
      const res = validateSkill(content);
      if (ONLY && res.source && res.source !== ONLY) continue;
      results.push({ location: label, path: path.relative(ROOT, file).replace(/\\/g, '/'), ok: res.ok, errors: res.errors, source: res.source });
    }
  }

  const failures = results.filter(r => !r.ok);
  const summary = {
    total: results.length,
    ok: results.length - failures.length,
    failed: failures.length,
  };

  if (JSON_OUT) {
    process.stdout.write(JSON.stringify({ summary, results }, null, 2) + '\n');
  } else {
    const rowsToShow = QUIET ? failures : results;
    if (!rowsToShow.length) {
      console.log('skill-validate: no SKILL.md files found.');
    } else {
      console.log('skill-validate:');
      for (const r of rowsToShow) {
        const tag = r.ok ? '[ok]  ' : '[fail]';
        const errs = r.errors.length ? ` — ${r.errors.join('; ')}` : '';
        console.log(`  ${tag} ${r.path}${errs}`);
      }
    }
    console.log(`\nskill-validate: ${summary.ok}/${summary.total} ok, ${summary.failed} failed.`);
  }

  process.exit(failures.length > 0 ? 2 : 0);
}

// Fail-open wrapper keeps parent processes alive even on a crash.
if (require.main === module) {
  try {
    main();
  } catch (err) {
    process.stderr.write(`[skill-validate] Fatal error: ${err.message}\n${err.stack}\n`);
    process.exit(0);
  }
}

module.exports = { validateSkill, validateWithPython, collectSkills, collectSkillDirs };
