#!/usr/bin/env node
'use strict';

/**
 * migrate-skill-paths.js
 *
 * One-shot migration helper for target projects that still have role-prefixed
 * pattern skills (`frontend-*`, `backend-*`, `general-*`, plus any other agent
 * prefix from `.claude/.detect-cache.json`) duplicated inside each
 * `{subproject}/.claude/skills/`. Moves them to ROOT `{project}/.claude/skills/`
 * (the current output location of `skill-generator.js`) and deletes the
 * now-empty copies.
 *
 * SAFE BY DEFAULT: runs in dry-run unless `--apply` is passed.
 *
 * Usage (inside a Mustard-initialized project, NOT Mustard itself):
 *   node .claude/scripts/migrate-skill-paths.js             # dry-run, prints plan
 *   node .claude/scripts/migrate-skill-paths.js --apply     # actually move files
 *
 * Algorithm:
 *   1. Read `.claude/.detect-cache.json` → list of subprojects & their agent prefixes.
 *   2. For each subproject, scan `{sub}/.claude/skills/` for folders whose name
 *      begins with one of the known agent prefixes (frontend-, backend-, general-,
 *      api-, app-, database-, mobile-, …) — those are role skills in the wrong place.
 *   3. For each such folder:
 *        a. If ROOT/.claude/skills/{folder} already exists with identical content
 *           → just delete the duplicate.
 *        b. If ROOT missing or different → move the folder to ROOT (prefer the
 *           freshest mtime if there is already one at ROOT).
 *   4. Log every action. On --apply, perform the FS changes; otherwise just log.
 *
 * Non-goals:
 *   - Does NOT touch subproject-short prefixed skills (e.g. `admin-auth-guard`,
 *     `api-endpoint-wiring`) — those belong in their subproject.
 *   - Does NOT run skill-generator.js — user does that separately post-migration.
 */

const fs = require('fs');
const path = require('path');

const args = process.argv.slice(2);
const APPLY = args.includes('--apply');
const TARGET_ROOT = (() => {
  const idx = args.indexOf('--root');
  if (idx !== -1 && args[idx + 1]) return path.resolve(args[idx + 1]);
  return process.cwd();
})();

const DETECT_CACHE = path.join(TARGET_ROOT, '.claude', '.detect-cache.json');
const ROOT_SKILLS = path.join(TARGET_ROOT, '.claude', 'skills');

function readJsonSafe(p) {
  try { return JSON.parse(fs.readFileSync(p, 'utf-8')); } catch { return null; }
}

function hasSameContent(a, b) {
  try {
    return fs.readFileSync(a).equals(fs.readFileSync(b));
  } catch { return false; }
}

function folderChecksum(dir) {
  // Cheap signature: sorted list of relative paths + SKILL.md first-300 bytes.
  try {
    const entries = fs.readdirSync(dir, { withFileTypes: true })
      .flatMap(e => e.isDirectory()
        ? fs.readdirSync(path.join(dir, e.name), { withFileTypes: true }).map(f => `${e.name}/${f.name}`)
        : [e.name]
      ).sort().join('|');
    const skillMd = path.join(dir, 'SKILL.md');
    const head = fs.existsSync(skillMd) ? fs.readFileSync(skillMd).slice(0, 300).toString('utf-8') : '';
    return `${entries}\n${head}`;
  } catch { return ''; }
}

function main() {
  const cache = readJsonSafe(DETECT_CACHE);
  if (!cache) {
    console.error(`No .detect-cache.json at ${DETECT_CACHE}. Run: node .claude/scripts/sync-detect.js`);
    process.exit(1);
  }

  const subs = cache.subprojects || [];

  // A folder is "role-shared" (belongs at ROOT) only if its name prefix matches
  // THIS subproject's agent and the same agent occurs in 2+ subprojects. Skills
  // whose prefix is the subproject's unique short-name stay per-subproject.
  const agentCount = new Map();
  for (const s of subs) {
    const a = s.agent || s.role || 'general';
    agentCount.set(a, (agentCount.get(a) || 0) + 1);
  }

  const actions = []; // { type, from, to? }

  for (const sub of subs) {
    const subSkills = path.join(TARGET_ROOT, sub.path, '.claude', 'skills');
    if (!fs.existsSync(subSkills)) continue;
    let entries;
    try { entries = fs.readdirSync(subSkills, { withFileTypes: true }); } catch { continue; }

    const agent = sub.agent || sub.role || 'general';
    // Only migrate if this agent is shared across subs, OR if the folder name
    // prefix is a well-known role keyword (frontend/backend/general). A unique
    // subproject-short prefix (e.g. `admin-auth-guard` when only sialia-admin
    // exists) stays with the subproject.
    const wellKnownRole = ['frontend', 'backend', 'general', 'database', 'mobile'].includes(agent);
    const sharedAgent = (agentCount.get(agent) || 0) > 1;
    if (!wellKnownRole && !sharedAgent) continue;

    for (const e of entries) {
      if (!e.isDirectory()) continue;
      const folderName = e.name;
      // Only match THIS subproject's own agent prefix.
      if (!folderName.startsWith(agent + '-')) continue;

      const fromDir = path.join(subSkills, folderName);
      const toDir = path.join(ROOT_SKILLS, folderName);

      if (!fs.existsSync(toDir)) {
        actions.push({ type: 'move', from: fromDir, to: toDir, sub: sub.name });
      } else if (folderChecksum(fromDir) === folderChecksum(toDir)) {
        actions.push({ type: 'delete-dup', from: fromDir, sub: sub.name });
      } else {
        actions.push({ type: 'conflict', from: fromDir, to: toDir, sub: sub.name });
      }
    }
  }

  if (!actions.length) {
    console.log('No role-prefixed skills found in any subproject. Nothing to migrate.');
    return;
  }

  console.log(`\nPlanned actions${APPLY ? ' (APPLYING)' : ' (dry-run — pass --apply to execute)'}:\n`);
  for (const a of actions) {
    const relFrom = path.relative(TARGET_ROOT, a.from).replace(/\\/g, '/');
    if (a.type === 'move') {
      const relTo = path.relative(TARGET_ROOT, a.to).replace(/\\/g, '/');
      console.log(`  [move]        ${relFrom}  →  ${relTo}`);
    } else if (a.type === 'delete-dup') {
      console.log(`  [delete-dup]  ${relFrom}  (identical copy already at ROOT)`);
    } else {
      const relTo = path.relative(TARGET_ROOT, a.to).replace(/\\/g, '/');
      console.log(`  [conflict]    ${relFrom}  (ROOT has different content at ${relTo}) — manual review needed`);
    }
  }

  if (!APPLY) {
    console.log('\n(dry-run — no changes made. Re-run with --apply to execute.)');
    return;
  }

  fs.mkdirSync(ROOT_SKILLS, { recursive: true });
  let ok = 0, failed = 0, skipped = 0;
  for (const a of actions) {
    try {
      if (a.type === 'move') {
        // Re-check: an earlier action this run may have already placed a
        // folder at `a.to` (common when 3 subs share the same agent prefix).
        if (fs.existsSync(a.to)) {
          if (folderChecksum(a.from) === folderChecksum(a.to)) {
            fs.rmSync(a.from, { recursive: true, force: true });
            ok++;
          } else {
            console.error(`  CONFLICT at ${a.to} — content differs, leaving ${a.from} in place`);
            skipped++;
          }
        } else {
          fs.renameSync(a.from, a.to);
          ok++;
        }
      } else if (a.type === 'delete-dup') {
        fs.rmSync(a.from, { recursive: true, force: true });
        ok++;
      } else {
        skipped++;
      }
    } catch (err) {
      console.error(`  FAILED on ${a.from}: ${err.message}`);
      failed++;
    }
  }
  console.log(`\nMigration done: ${ok} applied, ${skipped} skipped (conflict), ${failed} failed.`);
}

try { main(); } catch (err) {
  console.error(`Fatal: ${err.message}`);
  process.exit(1);
}
