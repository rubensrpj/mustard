#!/usr/bin/env bun
'use strict';
/**
 * REVIEW-RESULT: Records the outcome of a pipeline REVIEW phase.
 *
 * The REVIEW phase audits a pipeline before CLOSE and yields APPROVED or
 * REJECTED. Unlike QA, REVIEW emitted nothing — so `/stats` could not show
 * whether pipelines were reviewed. This script instruments that phase: it
 * writes a `review.result` harness event and a `review` hook metric.
 *
 * Usage:
 *   bun .claude/scripts/review-result.js --spec auth-login --verdict approved
 *   bun .claude/scripts/review-result.js --spec auth-login --verdict rejected --critical 2
 *   bun .claude/scripts/review-result.js --spec auth-login --verdict approved --subproject api
 *
 * Exported API:
 *   module.exports = { recordReview };
 *   recordReview({ spec, verdict, criticalCount, subproject, cwd }) → ReviewResult
 *
 * @version 1.0.0
 */

const { emit } = require('../hooks/_lib/harness-event.js');
const { emitMetric } = require('../hooks/_lib/metrics-emit.js');

// ── Main recordReview ──────────────────────────────────────────────────────────

/**
 * Record a REVIEW outcome.
 *
 * @param {{ spec: string, verdict: 'approved'|'rejected', criticalCount?: number,
 *           subproject?: string, cwd?: string }} opts
 * @returns {{ spec, verdict, criticalCount, subproject }}
 */
function recordReview({ spec, verdict, criticalCount, subproject, cwd: cwdArg } = {}) {
  const cwd = cwdArg || process.cwd();
  const criticalNum = Number.isFinite(criticalCount) ? criticalCount : 0;
  const payload = { spec, verdict, criticalCount: criticalNum, subproject: subproject || null };

  // ── Emit harness event ────────────────────────────────────────────────────────
  try {
    emit('review.result', payload, { cwd, actor: { kind: 'script', id: 'review-result' } });
  } catch (_) {
    // fail-open: event emission does not affect the review result
  }

  // ── Emit hook metric ──────────────────────────────────────────────────────────
  try {
    emitMetric('review', {
      note: verdict,
      extras: { spec, verdict, criticalCount: criticalNum, category: 'verification' },
      cwd,
    });
  } catch (_) {
    // fail-silent: metric emission never affects the review result
  }

  return payload;
}

// ── CLI entrypoint ────────────────────────────────────────────────────────────

if (require.main === module) {
  const args = process.argv.slice(2);
  let spec = null;
  let verdict = null;
  let criticalCount = 0;
  let subproject = null;
  let cwdArg = null;

  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--spec' && args[i + 1]) { spec = args[++i]; }
    else if (args[i] === '--verdict' && args[i + 1]) { verdict = args[++i]; }
    else if (args[i] === '--critical' && args[i + 1]) { criticalCount = parseInt(args[++i], 10); }
    else if (args[i] === '--subproject' && args[i + 1]) { subproject = args[++i]; }
    else if (args[i] === '--cwd' && args[i + 1]) { cwdArg = args[++i]; }
  }

  if (!spec || !verdict) {
    process.stderr.write('Usage: bun review-result.js --spec <name> --verdict approved|rejected [--critical <N>] [--subproject <name>]\n');
    process.exit(1);
  }
  if (verdict !== 'approved' && verdict !== 'rejected') {
    process.stderr.write(`[review-result] Invalid --verdict "${verdict}" — expected approved|rejected\n`);
    process.exit(1);
  }

  try {
    const result = recordReview({ spec, verdict, criticalCount, subproject, cwd: cwdArg || process.cwd() });
    process.stdout.write(JSON.stringify({ event: 'review.result', payload: result }, null, 2) + '\n');
    process.exit(0);
  } catch (err) {
    process.stderr.write(`[review-result] Fatal error: ${err.message}\n`);
    process.exit(0); // fail-open
  }
}

module.exports = { recordReview };
