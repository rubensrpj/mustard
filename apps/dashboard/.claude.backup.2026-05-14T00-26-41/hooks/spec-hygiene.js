#!/usr/bin/env bun
'use strict';
/**
 * spec-hygiene: SessionStart hook — auto-cleans stale specs in .claude/spec/active/
 * Mirror of logic in mustard/feature/SKILL.md "Spec Hygiene" section.
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

try {
  if (!shouldRun('spec-hygiene')) process.exit(0);

  let input = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => (input += chunk));
  process.stdin.on('end', () => {
    try {
      runHygiene();
    } catch (_) {
      // fail-open
    }
    process.exit(0);
  });
} catch (_) {
  process.exit(0);
}

function runHygiene() {
  const cwd = process.cwd();
  const activeDir = path.join(cwd, '.claude', 'spec', 'active');
  if (!fs.existsSync(activeDir)) return;

  let entries;
  try { entries = fs.readdirSync(activeDir); } catch (_) { return; }

  for (const name of entries) {
    try {
      const specDir = path.join(activeDir, name);
      const specFile = path.join(specDir, 'spec.md');
      if (!fs.existsSync(specFile)) continue;

      const content = fs.readFileSync(specFile, 'utf8');
      const classification = classify(content);

      if (classification === 'auto-move') {
        const completedDir = path.join(cwd, '.claude', 'spec', 'completed');
        const dest = path.join(completedDir, name);
        fs.mkdirSync(completedDir, { recursive: true });

        // Capture spec size BEFORE the rename so the path still resolves.
        let fileSize = 0;
        try { fileSize = fs.statSync(specFile).size; } catch (_) { /* best-effort */ }

        // Phase 1 (critical): atomic rename. If this fails, state is untouched.
        fs.renameSync(specDir, dest);
        process.stderr.write(`[hygiene] Moved ${name} → completed/\n`);

        // Heuristic: tokens "saved" ≈ file_size / 4 (chars-to-tokens). The spec
        // would otherwise have been re-read in future sessions; moving it to
        // completed/ removes it from the active scan path.
        const tokens = Math.round(fileSize / 4);
        emitMetric('spec-hygiene-move', {
          tokensAffected: tokens,
          tokensSaved: tokens,
          note: 'stale spec moved from active/',
          extras: { from: specDir, to: dest, category: 'extraction' },
          cwd,
        });

        // Phase 2 (best-effort): cleanup orphan state files.
        // Each wrapped independently so a failure in one doesn't skip the others.
        const statesDir = path.join(cwd, '.claude', '.pipeline-states');
        const stateFile = path.join(statesDir, `${name}.json`);
        const diffFile = path.join(statesDir, `${name}.diff.md`);
        for (const staleFile of [stateFile, diffFile]) {
          try {
            if (fs.existsSync(staleFile)) fs.unlinkSync(staleFile);
          } catch (_) {
            // orphan state is harmless — next hygiene run will retry
          }
        }

      } else if (classification === 'warn') {
        process.stderr.write(
          `[hygiene] Spec ${name} appears done but Status=implementing. Run /mustard:complete to finalize.\n`
        );
        // SILENT: do nothing for all other states
      }
    } catch (_) {
      // fail-open per spec
    }
  }
}

/**
 * Classify a spec based on its content.
 * Returns: 'auto-move' | 'warn' | 'silent'
 */
function classify(content) {
  // 1. Parse Status from header: "### Status: completed | Phase: ..."
  const statusMatch = content.match(/###\s*Status:\s*([\w|]+)/m);
  if (!statusMatch) return 'silent';
  // Status field may be "completed | Phase: CLOSE | Scope: light" — take first word
  const statusRaw = statusMatch[1].split(/[\s|]/)[0].toLowerCase();

  // 2. Check for BLOCKED concerns
  const concernsMatch = content.match(/##\s*Concerns([\s\S]*?)(?=\n##\s|$)/);
  if (concernsMatch && /BLOCKED/i.test(concernsMatch[1])) return 'silent';

  // 3. Count checkboxes only in Checklist region (greedy to end — subheaders like ### are fine)
  const checklistMatch = content.match(/##\s*Checklist([\s\S]*?)(?=\n##\s|$)/);
  const checklistSection = checklistMatch ? checklistMatch[1] : content;
  const checked = (checklistSection.match(/\[x\]/gi) || []).length;
  const unchecked = (checklistSection.match(/\[ \]/g) || []).length;
  const total = checked + unchecked;

  const allDone = total > 0 && unchecked === 0;

  if ((statusRaw === 'completed' || statusRaw === 'cancelled') && allDone) {
    return 'auto-move';
  }
  if (statusRaw === 'implementing' && allDone) {
    return 'warn';
  }
  return 'silent';
}
