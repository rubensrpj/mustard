#!/usr/bin/env bun
'use strict';
/**
 * RECOMMENDED-SKILLS-AUDIT: PreToolUse hook (matcher: Task) that measures the
 * number of skills a pipeline dispatch lists in its `recommended_skills`
 * hint and estimates the bytes Claude would load if it honored every one.
 *
 * Advisory only — never blocks. Emits `recommended-skills` metric with
 * skill_count, resolved skills, bytes, and subagent_type. Prints a stderr
 * WARN if skill_count > 10 so the orchestrator can prune the list.
 *
 * Fail-open: exits 0 on any error. No external dependencies.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

const WARN_THRESHOLD = 10;

// ── Parsing ────────────────────────────────────────────────────────────────
/**
 * Extract skill names from a prompt. Tolerates two common shapes:
 *   1) `recommended_skills: [alpha, beta, gamma]`  (array literal)
 *   2) a markdown section headed "recommended skills" with bulleted items
 *      until the next `##` header or end of text
 *
 * Returns a de-duplicated array of skill names (lowercase, trimmed).
 * @param {string} prompt
 * @returns {string[]}
 */
function extractSkills(prompt) {
  if (!prompt || typeof prompt !== 'string') return [];
  const found = new Set();

  // Shape 1: recommended_skills: [a, b, c] — tolerant to spacing/underscore/dash
  const arrRe = /recommended[_\s-]?skills?[:\s]*\[([^\]]+)\]/gi;
  let m;
  while ((m = arrRe.exec(prompt)) !== null) {
    const inner = m[1];
    for (const raw of inner.split(/[,\n]/)) {
      const name = raw.replace(/["'`]/g, '').trim();
      if (name) found.add(name.toLowerCase());
    }
  }

  // Shape 2: ## Recommended Skills\n... until next heading or EOF
  const secRe = /^##?\s*recommended\s+skills?\s*$([\s\S]*?)(?=^##?\s|$(?![\s\S]))/gim;
  while ((m = secRe.exec(prompt)) !== null) {
    const body = m[1] || '';
    for (const line of body.split('\n')) {
      // Match bullets or backtick-name lines; also plain "- name" or "* name"
      const bm = line.match(/^\s*(?:[-*]|\d+\.)\s+`?([a-z0-9][a-z0-9._\/:-]+)`?/i);
      if (bm) found.add(bm[1].toLowerCase());
    }
  }

  return [...found];
}

// ── Skill resolution ───────────────────────────────────────────────────────
/**
 * Attempt to resolve a skill name to its SKILL.md path under the project.
 * Checks `.claude/skills/{name}/SKILL.md` first; then looks through any
 * `<sub>/.claude/skills/{name}/SKILL.md` via a shallow subproject walk.
 * Returns { path, bytes } or null.
 */
function resolveSkill(projectDir, name) {
  try {
    const safe = name.replace(/[^a-z0-9._-]/gi, '');
    if (!safe) return null;
    const primary = path.join(projectDir, '.claude', 'skills', safe, 'SKILL.md');
    if (fs.existsSync(primary)) {
      return { path: primary, bytes: fs.statSync(primary).size };
    }
    // Shallow subproject scan (1 level deep)
    const entries = fs.readdirSync(projectDir, { withFileTypes: true });
    for (const ent of entries) {
      if (!ent.isDirectory()) continue;
      if (ent.name.startsWith('.') || ent.name === 'node_modules') continue;
      const cand = path.join(projectDir, ent.name, '.claude', 'skills', safe, 'SKILL.md');
      if (fs.existsSync(cand)) {
        return { path: cand, bytes: fs.statSync(cand).size };
      }
    }
  } catch (_) { /* fail-silent */ }
  return null;
}

// ── Main ───────────────────────────────────────────────────────────────────
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('recommended-skills-audit')) {
      process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n');
      process.exit(0);
    }

    const data = JSON.parse(input || '{}');
    const event = data.hook_event_name || '';
    const toolName = data.tool_name || '';
    if (event !== 'PreToolUse' || toolName !== 'Task') {
      process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n');
      process.exit(0);
    }

    const projectDir = process.env.CLAUDE_PROJECT_DIR || data.cwd || process.cwd();
    const toolInput = data.tool_input || {};
    const prompt = toolInput.prompt || '';
    const subagentType = toolInput.subagent_type || '';

    const skills = extractSkills(prompt);
    const skillCount = skills.length;

    if (skillCount === 0) {
      // Still emit so operators can see "most dispatches pass zero skills"
      emitMetric('recommended-skills', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'pipeline dispatch',
        extras: {
          skill_count: 0,
          skills: '',
          subagent_type: subagentType,
        },
      });
      process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n');
      process.exit(0);
    }

    let totalBytes = 0;
    let resolved = 0;
    for (const name of skills) {
      const r = resolveSkill(projectDir, name);
      if (r) {
        totalBytes += r.bytes;
        resolved++;
      }
    }

    emitMetric('recommended-skills', {
      tokensAffected: Math.round(totalBytes / 4),
      tokensSaved: 0, // advisory only — no pruning here
      note: 'pipeline dispatch',
      extras: {
        skill_count: skillCount,
        resolved_count: resolved,
        skills: skills.slice(0, 20).join(','),
        subagent_type: subagentType,
      },
    });

    if (skillCount > WARN_THRESHOLD) {
      process.stderr.write(
        `[recommended-skills] WARN: pipeline dispatch lists ${skillCount}>${WARN_THRESHOLD} skills — considere podar\n`
      );
    }

    process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n');
    process.exit(0);
  } catch (err) {
    // fail-open
    try { process.stderr.write('[recommended-skills-audit] ' + (err && err.message || err) + '\n'); } catch (_) {}
    try { process.stdout.write(JSON.stringify({ permissionDecision: 'allow' }) + '\n'); } catch (_) {}
    process.exit(0);
  }
});
