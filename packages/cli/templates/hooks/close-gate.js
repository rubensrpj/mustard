#!/usr/bin/env bun
/**
 * CLOSE-GATE: PreToolUse hook that blocks pipeline CLOSE if sensors fail.
 *
 * Triggers on Write|Edit to .pipeline-states/*.json when the content
 * transitions phase to "CLOSE". Checks:
 *   1. QA gate (Wave 10): verifies qa.result overall=pass in events.jsonl
 *   2. Build → type → lint → test in order (Wave 9 behavior)
 * On any real failure: permissionDecision deny. On hook/env bugs: fail-open.
 *
 * Wave 9+10 — strict gate (exception to fail-open default).
 *
 * Env:
 *   MUSTARD_CLOSE_GATE_MODE=strict (default) | warn | off
 *   MUSTARD_QA_GATE_MODE=strict (default) | warn | off
 *   MUSTARD_CHECKLIST_GATE_MODE=strict (default) | warn | off
 *
 * @version 2.1.0
 */

'use strict';

const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const { emit } = require('./_lib/harness-event.js');
const { emitMetric } = require('./_lib/metrics-emit.js');
const { headingRegex } = require('../scripts/_lib/spec-sections.js');
const { formatGateMessage } = require('./_lib/gate-message.js');

// fail-open: an unknown section key must never throw past the hook's guard.
const safeHeading = (key) => {
  try { return headingRegex(key); } catch (_) { return /(?!)/; }
};

const TRUNCATE_CHARS = 500;
const COMMAND_TIMEOUT_MS = 5 * 60 * 1000; // 5 min per command

function getMode() {
  return (process.env.MUSTARD_CLOSE_GATE_MODE || 'strict').toLowerCase();
}

function getQAMode() {
  return (process.env.MUSTARD_QA_GATE_MODE || 'strict').toLowerCase();
}

function getChecklistMode() {
  return (process.env.MUSTARD_CHECKLIST_GATE_MODE || 'strict').toLowerCase();
}

/**
 * Read the active spec for {spec} and return unmarked checklist items.
 * Returns { found: bool, unmarked: string[] }.
 *   found=false  — spec or Checklist section not found (treat as skip)
 *   found=true   — Checklist section located; unmarked is the list of trimmed item texts (may be empty)
 */
function findUnmarkedChecklistItems(cwd, spec) {
  if (!spec) return { found: false, unmarked: [] };
  const specPath = path.join(cwd, '.claude', 'spec', 'active', spec, 'spec.md');
  if (!fs.existsSync(specPath)) return { found: false, unmarked: [] };

  let raw;
  try { raw = fs.readFileSync(specPath, 'utf8'); }
  catch (_) { return { found: false, unmarked: [] }; }

  const lines = raw.split('\n');
  let startIdx = -1;
  for (let i = 0; i < lines.length; i++) {
    if (/^##\s+Checklist\b/.test(lines[i])) { startIdx = i + 1; break; }
  }
  if (startIdx === -1) return { found: false, unmarked: [] };

  let endIdx = lines.length;
  for (let i = startIdx; i < lines.length; i++) {
    if (/^##\s/.test(lines[i])) { endIdx = i; break; }
  }

  const unmarked = [];
  const re = /^\s*-\s+\[ \]\s+(.*)$/;
  for (let i = startIdx; i < endIdx; i++) {
    const m = lines[i].match(re);
    if (m) unmarked.push(m[1].trim());
  }
  return { found: true, unmarked };
}

/**
 * Scan an active spec for debt markers that should NOT exist when closing.
 *
 * Returns { found: bool, markers: Array<{line: number, snippet: string, pattern: string}> }
 *
 * Detects:
 *   - "future hook" / "future X" (Wave-1 admission anti-pattern)
 *   - "not part of Wave" / "not part of this wave"
 *   - "TODO:" / "FIXME:" / "XXX:" with content (not "TODO: done", e.g.)
 *   - "not yet implemented" / "not implemented"
 *
 * Skips comments inside fenced code blocks (```...```) — those are examples.
 */
function findDebtMarkers(cwd, spec) {
  if (!spec) return { found: false, markers: [] };
  const specPath = path.join(cwd, '.claude', 'spec', 'active', spec, 'spec.md');
  if (!fs.existsSync(specPath)) return { found: false, markers: [] };

  let raw;
  try { raw = fs.readFileSync(specPath, 'utf8'); } catch (_) { return { found: false, markers: [] }; }

  const lines = raw.split('\n');
  const markers = [];
  let inFence = false;

  // Scope: only flag markers inside ACTIONABLE sections (Tasks, Checklist,
  // Acceptance Criteria — and their PT equivalents Tarefas / Critérios de
  // Aceitação). Pendências is by convention where authors document legitimate
  // open follow-ups — exempt to avoid false-positive close blocks.
  // Concerns/Summary/Non-Goals/Contexto are likewise historical/external debt.
  const tasksHeading = safeHeading('tasks');
  const acHeading = safeHeading('acceptanceCriteria');
  const isActionableHeading = (line) =>
    tasksHeading.test(line) || acHeading.test(line);
  const ANY_H2 = /^##\s+\S/;
  let inActionable = false;

  // Patterns are case-insensitive. Each entry: [regex, label]
  const PATTERNS = [
    [/\bfuture\s+hook\b/i,                'future-hook'],
    [/\bnot\s+part\s+of\s+(?:this\s+)?wave\s*\d*\b/i, 'not-part-of-wave'],
    [/\bnot\s+yet\s+implemented\b/i,      'not-yet-implemented'],
    [/\bTODO:[^\s]*\s+\S/,                'TODO'],
    [/\bFIXME:[^\s]*\s+\S/,               'FIXME'],
    [/\bXXX:[^\s]*\s+\S/,                 'XXX'],
  ];

  // Inline-backtick stripper — markers inside `code` are descriptive examples.
  const stripInlineCode = (s) => s.replace(/`[^`]*`/g, '');

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^```/.test(line.trim())) { inFence = !inFence; continue; }
    if (inFence) continue;
    if (ANY_H2.test(line)) {
      inActionable = isActionableHeading(line);
      continue;
    }
    if (!inActionable) continue;

    const cleaned = stripInlineCode(line);
    for (const [re, label] of PATTERNS) {
      if (re.test(cleaned)) {
        markers.push({
          line: i + 1,
          snippet: line.trim().slice(0, 140),
          pattern: label,
        });
        break;
      }
    }
  }

  return { found: markers.length > 0, markers };
}

function getDebtMode() {
  return (process.env.MUSTARD_DEBT_GATE_MODE || 'strict').toLowerCase();
}

/** True if the file path looks like a pipeline-state file */
function isPipelineStateFile(filePath) {
  if (!filePath) return false;
  const normalized = filePath.replace(/\\/g, '/');
  return /\.pipeline-states\/[^/]+\.json$/.test(normalized);
}

/** Extract content string from tool_input depending on tool (Write vs Edit) */
function extractContent(toolInput) {
  if (!toolInput) return null;
  // Write tool uses tool_input.content
  if (typeof toolInput.content === 'string') return toolInput.content;
  // Edit tool uses tool_input.new_string
  if (typeof toolInput.new_string === 'string') return toolInput.new_string;
  return null;
}

/** Parse JSON and return phase, or null if not parseable */
function extractPhase(content) {
  if (!content) return null;
  try {
    const obj = JSON.parse(content);
    if (!obj) return null;
    // Pipeline-state files use a numeric `phase` index plus a string
    // `phaseName`. Legacy fixtures use a string `phase`. Read whichever
    // string field is present.
    const raw = typeof obj.phaseName === 'string' ? obj.phaseName
      : typeof obj.phase === 'string' ? obj.phase
      : null;
    return raw ? raw.toUpperCase() : null;
  } catch (_) {
    return null;
  }
}

/**
 * Find the last qa.result event for a spec.
 *
 * Read path: EventStore (SQLite projection of events.jsonl). Falls back to a
 * direct events.jsonl scan when the store is unavailable — keeps the gate
 * functional in projects that haven't run the migration yet.
 *
 * Returns { found: bool, overall: 'pass'|'fail'|'skip'|null, failedCount: number }
 */
function findLastQAResult(cwd, spec) {
  let lastQAResult = null;

  // Preferred path: EventStore.query() — O(log n) indexed lookup.
  try {
    const { getStore } = require('./_lib/event-store.js');
    const store = getStore(path.join(cwd, '.claude'));
    if (store) {
      const filter = spec ? { event: 'qa.result', spec } : { event: 'qa.result' };
      const events = store.query(filter);
      if (Array.isArray(events) && events.length > 0) {
        // EventStore.query returns chronological order; last entry wins.
        lastQAResult = events[events.length - 1];
      }
    }
  } catch (_) {} // fail-open: fall through to jsonl read

  // Fallback: read events.jsonl directly. Required when EventStore is missing
  // (legacy installs, runtime without SQLite driver, or pre-migration projects).
  if (!lastQAResult) {
    const eventsFile = path.join(cwd, '.claude', '.harness', 'events.jsonl');
    if (!fs.existsSync(eventsFile)) {
      return { found: false, overall: null, failedCount: 0 };
    }
    try {
      const lines = fs.readFileSync(eventsFile, 'utf8').split('\n').filter(Boolean);
      for (const line of lines) {
        try {
          const ev = JSON.parse(line);
          if (ev.event !== 'qa.result') continue;
          if (!ev.payload) continue;
          if (spec && ev.payload.spec && ev.payload.spec !== spec) continue;
          lastQAResult = ev;
        } catch (_) {}
      }
    } catch (_) {
      return { found: false, overall: null, failedCount: 0 };
    }
  }

  if (!lastQAResult || !lastQAResult.payload) {
    return { found: false, overall: null, failedCount: 0 };
  }
  const overall = lastQAResult.payload.overall || null;
  const criteria = Array.isArray(lastQAResult.payload.criteria) ? lastQAResult.payload.criteria : [];
  const failedCount = criteria.filter(c => c.status === 'fail').length;
  return { found: true, overall, failedCount };
}

/** Extract spec name from pipeline state content */
function extractSpecFromContent(content) {
  try {
    const obj = JSON.parse(content);
    return obj && typeof obj.spec === 'string' ? obj.spec
      : obj && typeof obj.specName === 'string' ? obj.specName
      : null;
  } catch (_) {
    return null;
  }
}

/** Read mustard.json from cwd and return command fields */
function readMustardCommands(cwd) {
  try {
    const p = path.join(cwd, 'mustard.json');
    if (!fs.existsSync(p)) return null;
    const cfg = JSON.parse(fs.readFileSync(p, 'utf8'));
    return {
      buildCommand: cfg.buildCommand || null,
      typeCheckCommand: cfg.typeCheckCommand || null,
      lintCommand: cfg.lintCommand || null,
      testCommand: cfg.testCommand || null,
    };
  } catch (_) {
    return null;
  }
}

/**
 * Run a single command via the system shell.
 * Returns { ok, output, envError }
 *
 * envError=true means: the shell could not be launched, or the command was
 * an empty string — i.e. a hook/environment bug, not a real test/build failure.
 * envError=false + ok=false means: real failure (non-zero exit) → block.
 */
function runCommand(cmd, cwd) {
  if (!cmd || !cmd.trim()) {
    return { ok: false, output: 'empty command', envError: true };
  }

  const IS_WIN = process.platform === 'win32';
  const shellCmd = IS_WIN ? 'cmd' : 'sh';
  const shellArgs = IS_WIN ? ['/c', cmd] : ['-c', cmd];

  let result;
  try {
    result = spawnSync(shellCmd, shellArgs, {
      cwd,
      stdio: 'pipe',
      timeout: COMMAND_TIMEOUT_MS,
      windowsHide: true,
      encoding: 'utf8',
    });
  } catch (spawnErr) {
    // spawnSync threw synchronously — this is an env bug
    return { ok: false, output: spawnErr.message || String(spawnErr), envError: true };
  }

  // The shell itself could not be launched (ENOENT for sh/cmd, very unusual)
  if (result.error) {
    return { ok: false, output: result.error.message || String(result.error), envError: true };
  }

  // Timeout: spawnSync sets status=null and may set signal='SIGTERM'
  if (result.status === null) {
    return { ok: false, output: `[timeout after ${COMMAND_TIMEOUT_MS}ms] ${cmd}`, envError: true };
  }

  if (result.status === 0) {
    return { ok: true, output: '', envError: false };
  }

  // Non-zero exit → real sensor failure
  const raw = [result.stdout || '', result.stderr || ''].filter(Boolean).join('\n').trim();
  return { ok: false, output: raw, envError: false };
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  const mode = getMode();

  // Mode: off — skip everything
  if (mode === 'off') {
    process.exit(0);
  }

  let data;
  try {
    data = JSON.parse(input);
  } catch (_) {
    // Unparseable input → fail-open
    process.exit(0);
  }

  try {
    const toolInput = data.tool_input || {};
    const filePath = toolInput.file_path || toolInput.path || '';

    // Only trigger on pipeline-state files
    if (!isPipelineStateFile(filePath)) {
      process.exit(0);
    }

    // Extract the content being written
    const content = extractContent(toolInput);
    if (!content) {
      process.exit(0);
    }

    // Only trigger on phase=CLOSE
    const phase = extractPhase(content);
    if (phase !== 'CLOSE') {
      process.exit(0);
    }

    const cwd = data.cwd || process.cwd();
    const specName = extractSpecFromContent(content);

    // ── Debt-marker gate (prevents "Wave 1 closed with TODOs" anti-pattern) ──
    const debtMode = getDebtMode();
    if (debtMode !== 'off') {
      const debt = findDebtMarkers(cwd, specName);
      if (debt.found) {
        const top = debt.markers.slice(0, 5).map(m => `  - line ${m.line} (${m.pattern}): ${m.snippet}`).join('\n');
        const more = debt.markers.length > 5 ? `\n  …and ${debt.markers.length - 5} more` : '';
        const reason = formatGateMessage({
          gate: 'Close Gate',
          what: `spec "${specName}" still contains ${debt.markers.length} debt marker(s)`,
          why: 'closing a spec with open TODO/FIXME hides unfinished work',
          exit: 'resolve them or move to a follow-up spec, or set MUSTARD_DEBT_GATE_MODE=warn',
        }) + `\n${top}${more}`;

        if (debtMode === 'warn') {
          process.stderr.write(`[close-gate] WARN: ${reason}\n`);
          // fall through
        } else {
          try {
            emit('close-gate.check', { result: 'deny-debt-markers', mode, debtMode, spec: specName, markerCount: debt.markers.length, patterns: debt.markers.map(m => m.pattern) }, { cwd, hookInput: data });
          } catch (_) {}
          emitMetric('close-gate', {
            tokensAffected: 0,
            tokensSaved: 0,
            note: 'blocked-debt-markers',
            extras: { reason: 'debt-markers', specName, markerCount: debt.markers.length, category: 'prevention' },
          });
          process.stdout.write(JSON.stringify({
            hookSpecificOutput: {
              hookEventName: 'PreToolUse',
              permissionDecision: 'deny',
              permissionDecisionReason: reason,
            },
          }) + '\n');
          process.exit(0);
        }
      }
    }

    // ── Checklist consistency gate ────────────────────────────────────────────
    const checklistMode = getChecklistMode();
    if (checklistMode !== 'off') {
      const cl = findUnmarkedChecklistItems(cwd, specName);
      if (cl.found && cl.unmarked.length > 0) {
        const preview = cl.unmarked.slice(0, 5).map(t => `  - ${t}`).join('\n');
        const more = cl.unmarked.length > 5 ? `\n  …and ${cl.unmarked.length - 5} more` : '';
        const reason = formatGateMessage({
          gate: 'Close Gate',
          what: `checklist has ${cl.unmarked.length} unmarked item(s) for spec "${specName}"`,
          why: 'an incomplete checklist means the spec is not done',
          exit: `mark each via \`bun .claude/scripts/mark-checklist-item.js --spec ${specName} --item "<text>"\`, or set MUSTARD_CHECKLIST_GATE_MODE=warn`,
        }) + `\n${preview}${more}`;

        if (checklistMode === 'warn') {
          process.stderr.write(`[close-gate] WARN: ${reason}\n`);
          // fall through
        } else {
          try {
            emit('close-gate.check', { result: 'deny-checklist-unmarked', mode, checklistMode, spec: specName, unmarkedCount: cl.unmarked.length }, { cwd, hookInput: data });
          } catch (_) {}
          emitMetric('close-gate', {
            tokensAffected: 0,
            tokensSaved: 0,
            note: 'blocked-checklist',
            extras: { reason: 'checklist-unmarked', specName, unmarkedCount: cl.unmarked.length, category: 'prevention' },
          });
          process.stdout.write(JSON.stringify({
            hookSpecificOutput: {
              hookEventName: 'PreToolUse',
              permissionDecision: 'deny',
              permissionDecisionReason: reason,
            },
          }) + '\n');
          process.exit(0);
        }
      }
    }

    // ── Wave 10: QA gate check ────────────────────────────────────────────────
    const qaMode = getQAMode();
    if (qaMode !== 'off') {
      const qaResult = findLastQAResult(cwd, specName);

      if (!qaResult.found) {
        const qaReason = formatGateMessage({
          gate: 'Close Gate',
          what: specName ? `no QA pass recorded for spec "${specName}"` : 'no QA pass recorded',
          why: 'CLOSE requires the acceptance criteria to be verified',
          exit: specName
            ? `run /mustard:qa or bun .claude/scripts/qa-run.js --spec ${specName}, or set MUSTARD_QA_GATE_MODE=warn`
            : 'run /mustard:qa before closing, or set MUSTARD_QA_GATE_MODE=warn',
        });

        if (qaMode === 'warn') {
          process.stderr.write(`[close-gate] WARN: ${qaReason}\n`);
          // allow — fall through to build/test checks
        } else {
          // strict — deny
          try {
            emit('close-gate.check', { result: 'deny-qa-missing', mode, qaMode, spec: specName }, { cwd, hookInput: data });
          } catch (_) {}
          emitMetric('close-gate', {
            tokensAffected: 0,
            tokensSaved: 0,
            note: 'blocked-qa-missing',
            extras: { reason: 'qa-missing', specName, category: 'prevention' },
          });
          process.stdout.write(JSON.stringify({
            hookSpecificOutput: {
              hookEventName: 'PreToolUse',
              permissionDecision: 'deny',
              permissionDecisionReason: qaReason,
            },
          }) + '\n');
          process.exit(0);
        }
      } else if (qaResult.overall === 'skip') {
        // No testable AC — QA is advisory (matches /mustard:qa § Step 5).
        process.stderr.write(`[close-gate] WARN: QA skipped for spec "${specName}" (no acceptance criteria) — proceeding.\n`);
        // fall through to build/test checks
      } else if (qaResult.overall !== 'pass') {
        const failedStr = qaResult.failedCount > 0 ? `${qaResult.failedCount} criteria failed` : `overall=${qaResult.overall}`;
        const qaReason = formatGateMessage({
          gate: 'Close Gate',
          what: specName ? `QA failed for spec "${specName}": ${failedStr}` : `QA did not pass (${failedStr})`,
          why: 'CLOSE requires every acceptance criterion to pass',
          exit: 'fix the failing criteria and re-run /mustard:qa, or set MUSTARD_QA_GATE_MODE=warn',
        });

        if (qaMode === 'warn') {
          process.stderr.write(`[close-gate] WARN: ${qaReason}\n`);
          // allow — fall through
        } else {
          // strict — deny
          try {
            emit('close-gate.check', { result: 'deny-qa-fail', mode, qaMode, spec: specName, qaOverall: qaResult.overall }, { cwd, hookInput: data });
          } catch (_) {}
          emitMetric('close-gate', {
            tokensAffected: 0,
            tokensSaved: 0,
            note: 'blocked-qa-fail',
            extras: { reason: 'qa-fail', specName, qaOverall: qaResult.overall, failedCount: qaResult.failedCount, category: 'prevention' },
          });
          process.stdout.write(JSON.stringify({
            hookSpecificOutput: {
              hookEventName: 'PreToolUse',
              permissionDecision: 'deny',
              permissionDecisionReason: qaReason,
            },
          }) + '\n');
          process.exit(0);
        }
      }
      // QA passed — fall through to build/test checks
    }

    // ── Wave 9: build/test gate ───────────────────────────────────────────────
    // Read mustard.json for commands
    const cmds = readMustardCommands(cwd);
    if (!cmds) {
      process.stderr.write('[close-gate] mustard.json not found or unreadable — skipping gate\n');
      process.exit(0);
    }

    // Build ordered stage list (skip null/empty commands)
    const stages = [
      { name: 'build', cmd: cmds.buildCommand },
      { name: 'type', cmd: cmds.typeCheckCommand },
      { name: 'lint', cmd: cmds.lintCommand },
      { name: 'test', cmd: cmds.testCommand },
    ].filter(s => s.cmd);

    if (stages.length === 0) {
      process.stderr.write('[close-gate] No commands configured in mustard.json — skipping gate\n');
      process.exit(0);
    }

    const stageResults = [];
    let firstFailure = null;

    for (const stage of stages) {
      const result = runCommand(stage.cmd, cwd);

      if (!result.ok && result.envError) {
        // Hook/env bug → fail-open with warning
        process.stderr.write(`[close-gate] env error running ${stage.name} (${stage.cmd}): ${result.output}\n`);
        // Record as env-error and continue to allow
        stageResults.push({ stage: stage.name, result: 'env-error' });
        continue;
      }

      stageResults.push({ stage: stage.name, result: result.ok ? 'pass' : 'fail', output: result.ok ? undefined : result.output });

      if (!result.ok && !firstFailure) {
        firstFailure = { stage: stage.name, output: result.output };
      }
    }

    // Emit harness event
    try {
      emit('close-gate.check', {
        result: firstFailure ? 'fail' : 'pass',
        stages: stageResults,
        mode,
      }, { cwd, hookInput: data });
    } catch (_) {}

    if (firstFailure) {
      const truncated = firstFailure.output
        ? firstFailure.output.slice(0, TRUNCATE_CHARS) + (firstFailure.output.length > TRUNCATE_CHARS ? '…' : '')
        : '(no output)';

      const reason = formatGateMessage({
        gate: 'Close Gate',
        what: `${firstFailure.stage} failed: ${truncated}`,
        why: 'CLOSE requires build, type, lint, and test to pass',
        exit: `fix the ${firstFailure.stage} failure and retry, or set MUSTARD_CLOSE_GATE_MODE=warn`,
      });

      if (mode === 'warn') {
        process.stderr.write(`[close-gate] WARN: ${reason}\n`);
        emitMetric('close-gate', {
          tokensAffected: 0,
          tokensSaved: 0,
          note: 'warned-' + firstFailure.stage,
          extras: { reason: firstFailure.stage, specName, mode },
        });
        // allow
        process.exit(0);
      }

      // mode === 'strict'
      emitMetric('close-gate', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'blocked-' + firstFailure.stage,
        extras: { reason: firstFailure.stage, specName, category: 'prevention' },
      });
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason: reason,
        },
      }) + '\n');
      process.exit(0);
    }

    // All passed
    emitMetric('close-gate', {
      tokensAffected: 0,
      tokensSaved: 0,
      note: 'passed',
      extras: { specName, stages: stageResults.map(s => s.stage + ':' + s.result).join(',') },
    });
    process.exit(0);

  } catch (err) {
    // Bug in hook itself → fail-open
    process.stderr.write(`[close-gate] Hook error (fail-open): ${err.message}\n`);
    process.exit(0);
  }
});
