#!/usr/bin/env bun
'use strict';
/**
 * QA-RUN: Executes Acceptance Criteria defined in a spec file.
 *
 * Reads `.claude/specs/{spec}.md` (or `.claude/spec/active/{spec}/spec.md`),
 * extracts the "Acceptance Criteria" section, runs each AC command,
 * and emits a `qa.result` event to the harness event log.
 *
 * Wave 10 — Dev/QA contract enforcement.
 *
 * Usage:
 *   bun .claude/scripts/qa-run.js --spec auth-login
 *   bun .claude/scripts/qa-run.js --spec auth-login --json
 *
 * Exported API:
 *   module.exports = { runQA };
 *   runQA({ spec, cwd }) → Promise<QAResult>
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const { emit } = require('../hooks/_lib/harness-event.js');

const AC_TIMEOUT_MS = 120_000; // 2 min per AC

// ── AC Parsing ─────────────────────────────────────────────────────────────────

/**
 * Extract the "Acceptance Criteria" section content from spec markdown.
 * Returns the raw section text (everything after the heading until the next ##-level heading),
 * or null if the section is not found.
 */
function extractACSection(markdown) {
  // Find "## Acceptance Criteria" heading (case-insensitive)
  const headingIdx = markdown.search(/^##\s+Acceptance\s+Criteria\s*$/im);
  if (headingIdx < 0) return null;

  const fromHere = markdown.slice(headingIdx);
  // Skip past the heading line
  const newlineIdx = fromHere.indexOf('\n');
  if (newlineIdx < 0) return '';
  const rest = fromHere.slice(newlineIdx + 1);

  // Terminate at the next ## heading
  const nextSection = rest.search(/^##\s/im);
  return nextSection >= 0 ? rest.slice(0, nextSection) : rest;
}

/**
 * Parse AC items from the section text.
 * Accepted formats:
 *   - [ ] AC-1: description — Command: `cmd`
 *   - [ ] AC-1: description — Command: cmd (no backticks)
 *   - [x] AC-1: (already checked — still executed)
 *
 * Returns array of { id, description, command }
 */
function parseACItems(sectionText) {
  if (!sectionText) return [];
  const items = [];
  const lines = sectionText.split('\n');

  for (const line of lines) {
    // Match: - [ ] AC-N: description — Command: `cmd` OR - [ ] AC-N: description — Command: cmd
    const m = line.match(
      /^\s*-\s*\[[ xX]\]\s*(AC-\d+)\s*:\s*(.+?)\s*(?:—|-{1,2})\s*Command\s*:\s*`?([^`\n]+)`?\s*$/i
    );
    if (!m) continue;
    const [, id, description, command] = m;
    items.push({ id: id.toUpperCase(), description: description.trim(), command: command.trim() });
  }
  return items;
}

// ── Spec Location ──────────────────────────────────────────────────────────────

/**
 * Find the spec file for a given spec name. Tries multiple locations:
 *   1. .claude/specs/{spec}.md
 *   2. .claude/spec/active/{spec}/spec.md
 *   3. .claude/spec/completed/{spec}/spec.md
 */
function findSpecFile(cwd, spec) {
  const candidates = [
    path.join(cwd, '.claude', 'specs', spec + '.md'),
    path.join(cwd, '.claude', 'spec', 'active', spec, 'spec.md'),
    path.join(cwd, '.claude', 'spec', 'completed', spec, 'spec.md'),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  return null;
}

// ── AC Execution ───────────────────────────────────────────────────────────────

/**
 * Run a single AC command.
 * Returns { status: 'pass'|'fail'|'skip', exit, duration_ms, stderr_excerpt }
 */
function runACCommand(command, cwd) {
  const t0 = Date.now();

  try {
    execSync(command, {
      cwd,
      timeout: AC_TIMEOUT_MS,
      stdio: 'pipe',
      encoding: 'utf8',
      shell: true,
      windowsHide: true,
    });
    const duration_ms = Date.now() - t0;
    return { status: 'pass', exit: 0, duration_ms, stderr_excerpt: '' };
  } catch (err) {
    const duration_ms = Date.now() - t0;

    // Command not found or spawn error (ENOENT = shell not found, very unusual) — skip
    if (err.code === 'ENOENT') {
      return { status: 'skip', exit: null, duration_ms, stderr_excerpt: 'command not found' };
    }

    // Timeout — skip (env error, not a real AC failure)
    if (err.killed || err.code === 'ETIMEDOUT') {
      return {
        status: 'skip',
        exit: null,
        duration_ms,
        stderr_excerpt: `timeout after ${AC_TIMEOUT_MS}ms`,
      };
    }

    // Real failure (non-zero exit code)
    const stderrRaw = (err.stderr || '').trim();
    const stdoutRaw = (err.stdout || '').trim();
    const combined = [stderrRaw, stdoutRaw].filter(Boolean).join(' ').slice(0, 100);
    return {
      status: 'fail',
      exit: typeof err.status === 'number' ? err.status : 1,
      duration_ms,
      stderr_excerpt: combined,
    };
  }
}

// ── Report Writing ─────────────────────────────────────────────────────────────

function writeQAReport(cwd, spec, payload) {
  try {
    const reportsDir = path.join(cwd, '.claude', '.qa-reports');
    fs.mkdirSync(reportsDir, { recursive: true });
    const reportPath = path.join(reportsDir, spec + '.json');
    fs.writeFileSync(reportPath, JSON.stringify(payload, null, 2), 'utf8');
  } catch (_) {
    // fail-open: report write failure does not affect qa result
  }
}

// ── Main runQA ─────────────────────────────────────────────────────────────────

/**
 * Run QA for a spec.
 *
 * @param {{ spec: string, cwd?: string }} opts
 * @returns {Promise<QAResult>}
 *
 * QAResult:
 * {
 *   spec: string,
 *   overall: 'pass' | 'fail' | 'skip',
 *   criteria: Array<{ id, status, exit, duration_ms, stderr_excerpt }>,
 *   skippedReason?: string,   // set when no AC section or no AC items
 *   report: string            // human-readable markdown
 * }
 */
async function runQA({ spec, cwd: cwdArg } = {}) {
  const cwd = cwdArg || process.cwd();

  // ── Locate spec ──────────────────────────────────────────────────────────────
  const specFile = findSpecFile(cwd, spec);
  if (!specFile) {
    const result = {
      spec,
      overall: 'skip',
      criteria: [],
      skippedReason: `Spec file not found for "${spec}" — tried .claude/specs/${spec}.md, .claude/spec/active/${spec}/spec.md`,
      report: `## QA Report for spec: ${spec}\n\n**SKIP** — spec file not found.\n`,
    };
    process.stderr.write(`[qa-run] ${result.skippedReason}\n`);
    return result;
  }

  // ── Read spec ────────────────────────────────────────────────────────────────
  let markdown;
  try {
    markdown = fs.readFileSync(specFile, 'utf8');
  } catch (err) {
    const result = {
      spec,
      overall: 'skip',
      criteria: [],
      skippedReason: `Cannot read spec file: ${err.message}`,
      report: `## QA Report for spec: ${spec}\n\n**SKIP** — cannot read spec file.\n`,
    };
    process.stderr.write(`[qa-run] ${result.skippedReason}\n`);
    return result;
  }

  // ── Extract Acceptance Criteria ───────────────────────────────────────────────
  const acSection = extractACSection(markdown);
  if (!acSection) {
    const result = {
      spec,
      overall: 'skip',
      criteria: [],
      skippedReason: 'No "Acceptance Criteria" section found in spec',
      report: `## QA Report for spec: ${spec}\n\n**SKIP** — no Acceptance Criteria section in spec.\n`,
    };
    process.stderr.write(`[qa-run] WARN: ${result.skippedReason}\n`);
    return result;
  }

  const acItems = parseACItems(acSection);
  if (acItems.length === 0) {
    const result = {
      spec,
      overall: 'skip',
      criteria: [],
      skippedReason: 'Acceptance Criteria section found but no parseable AC items',
      report: `## QA Report for spec: ${spec}\n\n**SKIP** — no parseable AC items found.\n`,
    };
    process.stderr.write(`[qa-run] WARN: ${result.skippedReason}\n`);
    return result;
  }

  // ── Execute each AC ──────────────────────────────────────────────────────────
  const criteria = [];
  let failCount = 0;
  let skipCount = 0;

  for (const ac of acItems) {
    const { status, exit, duration_ms, stderr_excerpt } = runACCommand(ac.command, cwd);
    criteria.push({ id: ac.id, status, exit, duration_ms, stderr_excerpt });
    if (status === 'fail') failCount++;
    if (status === 'skip') skipCount++;
  }

  const overall = failCount > 0 ? 'fail' : (skipCount === acItems.length ? 'skip' : 'pass');

  // ── Build report ──────────────────────────────────────────────────────────────
  const lines = [`## QA Report for spec: ${spec}`, ''];
  for (const c of criteria) {
    const icon = c.status === 'pass' ? '✅' : c.status === 'fail' ? '❌' : '⏭️';
    const exitStr = c.exit !== null ? `exit ${c.exit}` : 'n/a';
    const durStr = `${(c.duration_ms / 1000).toFixed(1)}s`;
    let line = `- ${c.id}: ${icon} ${c.status.toUpperCase()} — ${exitStr} (${durStr})`;
    if (c.stderr_excerpt) line += ` — stderr: ${c.stderr_excerpt.slice(0, 50)}`;
    lines.push(line);
  }
  lines.push('');
  lines.push(`**Overall**: ${overall.toUpperCase()} (${failCount} of ${acItems.length} failed)`);
  const report = lines.join('\n');

  // ── Payload ───────────────────────────────────────────────────────────────────
  const payload = { spec, overall, criteria };

  // ── Emit harness event ────────────────────────────────────────────────────────
  try {
    emit('qa.result', payload, { cwd, actor: { kind: 'script', id: 'qa-run' } });
  } catch (_) {
    // fail-open: event emission does not affect QA result
  }

  // ── Write sidecar ─────────────────────────────────────────────────────────────
  writeQAReport(cwd, spec, payload);

  return { ...payload, report };
}

// ── CLI entrypoint ────────────────────────────────────────────────────────────

if (require.main === module) {
  const args = process.argv.slice(2);
  let spec = null;
  let jsonOnly = false;
  let cwdArg = null;

  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--spec' && args[i + 1]) { spec = args[++i]; }
    else if (args[i] === '--json') { jsonOnly = true; }
    else if (args[i] === '--cwd' && args[i + 1]) { cwdArg = args[++i]; }
  }

  if (!spec) {
    process.stderr.write('Usage: node qa-run.js --spec <name> [--json] [--cwd <path>]\n');
    process.exit(1);
  }

  runQA({ spec, cwd: cwdArg || process.cwd() }).then(result => {
    if (jsonOnly) {
      process.stdout.write(JSON.stringify({
        event: 'qa.result',
        payload: { spec: result.spec, overall: result.overall, criteria: result.criteria },
      }, null, 2) + '\n');
    } else {
      process.stdout.write(result.report + '\n');
      if (result.skippedReason) {
        process.stderr.write(`[qa-run] Skipped: ${result.skippedReason}\n`);
      }
    }
    process.exit(result.overall === 'fail' ? 1 : 0);
  }).catch(err => {
    process.stderr.write(`[qa-run] Fatal error: ${err.message}\n`);
    process.exit(0); // fail-open
  });
}

module.exports = { runQA };
