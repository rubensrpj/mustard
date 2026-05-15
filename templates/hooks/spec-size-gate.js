#!/usr/bin/env bun
'use strict';
/**
 * spec-size-gate: PreToolUse hook for spec files.
 *
 * Two responsibilities, layered:
 *
 *   (1) Size gate — warns/blocks oversized spec files.
 *       Thresholds: warn 200 → strict-warn 400 → block 500.
 *       Env: MUSTARD_SPEC_SIZE_MODE = off | warn (default) | strict
 *
 *   (2) AC quality audit — warns when Acceptance Criteria are weak.
 *       A "weak" AC list is one where every AC's Command: clause is only a
 *       generic build/test invocation (npm test, bun test, npm run build, etc.)
 *       without verifying ACTUAL DATA (node -e, bash -c, grep, ack of payload).
 *       This is advisory only — never blocks.
 *       Env: MUSTARD_AC_QUALITY_MODE = off | warn (default)
 *
 * Triggers on Write|Edit when file_path matches:
 *   .claude/spec/active/.../*.md
 *   .claude/spec/completed/.../*.md
 *   .../spec/.../*.md
 *
 * @version 2.1.0
 */

const fs = require('fs');
const path = require('path');
const { emitMetric } = require('./_lib/metrics-emit.js');

function isSpecPath(filePath) {
  if (!filePath) return false;
  const p = filePath.replace(/\\/g, '/');
  if (/\.claude\/spec\/(active|completed)\/.+\.md$/.test(p)) return true;
  if (/\/spec\/.+\.md$/.test(p)) return true;
  return false;
}

/**
 * Extract the content of the `## Acceptance Criteria` section.
 * Returns the raw text between that header and the next `## ` header (or EOF).
 * Returns null if the section is absent.
 */
function extractACSection(content) {
  if (!content || typeof content !== 'string') return null;
  const lines = content.split('\n');
  let start = -1;
  for (let i = 0; i < lines.length; i++) {
    if (/^##\s+Acceptance\s+Criteria\b/i.test(lines[i])) { start = i + 1; break; }
  }
  if (start === -1) return null;
  let end = lines.length;
  for (let i = start; i < lines.length; i++) {
    if (/^##\s/.test(lines[i])) { end = i; break; }
  }
  return lines.slice(start, end).join('\n');
}

/**
 * Audit the AC section for empirical-rigor signals.
 *
 *   - "rich": at least one AC Command: uses node -e | bash -c | bun -e | grep | jq |
 *             curl | sqlite3 (these inspect real payload/state)
 *   - "poor": Command: is exclusively a build/test wrapper
 *
 * Returns { total, rich, poor, ratio: rich/total }.
 * Total === 0 means no AC defined (orthogonal — handled elsewhere).
 */
function auditAC(acText) {
  if (!acText) return { total: 0, rich: 0, poor: 0, ratio: 0 };

  const acItems = [];
  const lines = acText.split('\n');
  let curr = null;
  for (const line of lines) {
    // AC start: "- [ ] AC-N:" or "- [x] AC-N:"
    if (/^\s*-\s+\[[ x]\]\s+AC-\d+/i.test(line)) {
      if (curr) acItems.push(curr);
      curr = line;
    } else if (curr && /^\s+/.test(line)) {
      // continuation (indented)
      curr += '\n' + line;
    }
  }
  if (curr) acItems.push(curr);

  const RICH_PATTERNS = [
    /Command:.*\bnode\s+-e\b/i,
    /Command:.*\bbash\s+-c\b/i,
    /Command:.*\bbun\s+-e\b/i,
    /Command:.*\bgrep\b/i,
    /Command:.*\bjq\b/i,
    /Command:.*\bcurl\b/i,
    /Command:.*\bsqlite3?\b/i,
    /Command:.*\bcat\b.*\|/i, // cat with pipe = inspecting content
  ];
  const POOR_PATTERNS = [
    /Command:\s*`?(?:bun|npm|pnpm|yarn)\s+(?:run\s+)?(?:test|build|lint|tsc|type-check)\s*`?\s*$/im,
  ];
  // Non-binary AC — describes past validation OR references a moveable path —
  // can't be re-evaluated cleanly. Seen in fix-loop / resume sessions: authors
  // write "Command: já validado nesta sessão" or hard-code .claude/spec/active/
  // which breaks once the spec moves to completed/.
  const NON_BINARY_PATTERNS = [
    /Command:\s*j[áa]\s+validad[oa]/i,            // pt: "já validado" / "já validada"
    /Command:\s*\(?\s*validated(\s+nesta|\s+in)?/i, // en: "validated" / "validated in this session"
    /Command:\s*same\s+as\s+AC-/i,
    /Command:\s*\(?\s*(nenhum|none|n\/a|—|-)\s*\)?\s*$/im,
    /Command:[^\n]*\.claude\/spec\/active\//i,     // hard-coded active spec path
  ];
  // AC item without any "Command:" at all — descriptive bullet, not testable.
  const HAS_COMMAND = /Command\s*:/i;

  let rich = 0, poor = 0, nonBinary = 0;
  const nonBinaryReasons = new Set();
  for (const ac of acItems) {
    const isRich = RICH_PATTERNS.some(re => re.test(ac));
    const isPoor = !isRich && POOR_PATTERNS.some(re => re.test(ac));
    if (isRich) rich++;
    else if (isPoor) poor++;
    if (!HAS_COMMAND.test(ac)) {
      nonBinary++;
      nonBinaryReasons.add('missing-command');
    } else {
      for (let i = 0; i < NON_BINARY_PATTERNS.length; i++) {
        if (NON_BINARY_PATTERNS[i].test(ac)) {
          nonBinary++;
          nonBinaryReasons.add(['past-tense', 'past-tense-en', 'same-as', 'empty', 'active-path'][i]);
          break;
        }
      }
    }
  }
  return {
    total: acItems.length,
    rich,
    poor,
    nonBinary,
    nonBinaryReasons: Array.from(nonBinaryReasons),
    ratio: acItems.length > 0 ? rich / acItems.length : 0,
  };
}

function getACMode() {
  return (process.env.MUSTARD_AC_QUALITY_MODE || 'warn').toLowerCase();
}

/** Extract content from tool_input regardless of Write vs Edit. */
function extractContent(toolInput) {
  if (!toolInput) return null;
  if (typeof toolInput.content === 'string') return toolInput.content;
  if (typeof toolInput.new_string === 'string') return toolInput.new_string;
  return null;
}

// AC quality runs ASYNC alongside the size delegate. We tap stdin into a buffer
// and parse it ourselves (the size delegate also reads stdin but starts after
// process.on('end') is fully consumed — so we have to mirror that).
let buffered = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => { buffered += chunk; });
process.stdin.on('end', () => {
  // (2) AC quality audit (advisory). Runs BEFORE the size delegate so its
  // emit/warn output appears chronologically first. Never blocks.
  try {
    if (getACMode() !== 'off') {
      const data = JSON.parse(buffered);
      const toolInput = data.tool_input || {};
      const filePath = toolInput.file_path || toolInput.path || '';
      if (isSpecPath(filePath)) {
        const content = extractContent(toolInput);
        if (content) {
          const acText = extractACSection(content);
          if (acText) {
            const audit = auditAC(acText);
            if (audit.total >= 3 && audit.rich === 0 && audit.poor > 0) {
              // All AC commands are generic build/test — flag it.
              process.stderr.write(
                `[spec-size-gate] AC quality WARN: ${audit.poor}/${audit.total} AC use only build/test commands (no node -e / bash -c / grep / jq verifying real payload). Specs marked complete with only "build passes" AC don't prove the feature works end-to-end. See refs/feature/ac-cross-shell.md.\n`
              );
              try {
                emitMetric('spec-ac-quality', {
                  tokensAffected: 0,
                  tokensSaved: 0,
                  note: 'weak-ac-warned',
                  extras: { total: audit.total, rich: audit.rich, poor: audit.poor, file: filePath, category: 'workflow' },
                });
              } catch (_) {}
            }
            if (audit.nonBinary > 0) {
              process.stderr.write(
                `[spec-size-gate] AC quality WARN: ${audit.nonBinary}/${audit.total} AC não-binário (${audit.nonBinaryReasons.join(', ')}) — descreve validação passada, referencia path moveable (spec/active/), ou não tem Command. Re-rodar QA depois de CLOSE vai falhar. Use 'Command: bash -c ...' com assertion executável sobre estado atual.\n`
              );
              try {
                emitMetric('spec-ac-quality', {
                  tokensAffected: 0,
                  tokensSaved: 0,
                  note: 'non-binary-ac-warned',
                  extras: { total: audit.total, nonBinary: audit.nonBinary, reasons: audit.nonBinaryReasons, file: filePath, category: 'workflow' },
                });
              } catch (_) {}
            }
          }
        }
      }
    }
  } catch (_) { /* fail-silent */ }

  // (1) Size gate — re-feed stdin to the delegate by spawning logic inline.
  // The size delegate's `run()` reads stdin itself; since we've already
  // consumed it, we can't simply call run(). Instead, write a tiny wrapper
  // that reproduces the relevant size-gate behavior using the parsed input.
  delegateSizeGate(buffered);
});

/** Inline size-gate logic so we don't need to re-emit stdin. */
function delegateSizeGate(input) {
  let data;
  try { data = JSON.parse(input); } catch (_) { process.exit(0); }
  const mode = (process.env.MUSTARD_SPEC_SIZE_MODE || 'warn').toLowerCase();
  if (mode === 'off') process.exit(0);

  const toolInput = data.tool_input || {};
  const filePath = toolInput.file_path || toolInput.path || '';
  if (!isSpecPath(filePath)) process.exit(0);

  const content = extractContent(toolInput);
  if (!content) process.exit(0);

  const lines = content.split('\n').length;
  const thresholds = { warn: 200, strictWarn: 400, block: 500 };

  if (lines >= thresholds.block && mode === 'strict') {
    const reason = `[spec-size-gate] spec exceeds 500 lines (${lines} lines) — split into references/{section}.md (see feature/SKILL.md § Spec Layout)`;
    try {
      emitMetric('spec-size-gate', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'blocked',
        extras: { lines, limit: 500, file: filePath, category: 'prevention' },
      });
    } catch (_) {}
    process.stdout.write(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: reason,
      },
    }) + '\n');
    process.exit(0);
  }
  if (lines >= thresholds.warn) {
    process.stderr.write(`[spec-size-gate] WARN: spec has ${lines} lines (warn at ${thresholds.warn}, strict at ${thresholds.strictWarn}, block at ${thresholds.block})\n`);
    try {
      emitMetric('spec-size-gate', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'over-size',
        extras: { lines, limit: thresholds.block, file: filePath, category: 'workflow' },
      });
    } catch (_) {}
  }
  process.exit(0);
}
