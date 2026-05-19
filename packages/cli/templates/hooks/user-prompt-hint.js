#!/usr/bin/env bun
'use strict';
/**
 * USER-PROMPT-HINT: UserPromptSubmit hook that detects bugfix/feature/analysis
 * intent and suggests the appropriate Mustard slash command, routing work through
 * the pipeline (with gates + Light-scope auto-detect) instead of ad-hoc Opus.
 *
 * Triggers on:
 *   - Bugfix keywords → suggests /mustard:bugfix
 *   - Feature keywords → suggests /mustard:feature
 *   - Analysis keywords → suggests /mustard:task
 *
 * Pass-through:
 *   - Prompts starting with `/` (already using a command)
 *   - No keyword match
 *   - MUSTARD_DISABLED_HOOKS contains 'user-prompt-hint'
 *
 * Fail-open: exits 0 on any error.
 *
 * @version 1.1.0
 */

const { shouldRun } = require('./_lib/hook-env.js');

const BUGFIX_KEYWORDS = [
  'erro', 'bug', 'não funciona', 'nao funciona', 'quebrou',
  'broken', 'fix', 'failed', 'not working', 'corrigir', 'arrumar', 'crash',
];

const FEATURE_KEYWORDS = [
  'criar', 'adicionar', 'adiciona', 'novo', 'nova', 'implementar', 'implementa',
  'create', 'add', 'new', 'implement', 'build', 'construir',
  'ajustar', 'melhorar', 'alterar', 'mudar', 'improve', 'update', 'change',
];

const ANALYSIS_KEYWORDS = [
  'analise', 'analyze', 'verifica', 'check', 'audit', 'explain', 'explica',
];

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('user-prompt-hint')) { process.exit(0); }
    if ((process.env.MUSTARD_PROMPT_HINT_MODE || 'off').toLowerCase() === 'off') { process.exit(0); }

    const data = JSON.parse(input);
    const prompt = (data.prompt || '').trim();

    // Pass-through: already a slash command
    if (prompt.startsWith('/')) { process.exit(0); }

    const lower = prompt.toLowerCase();

    let hint = null;

    if (BUGFIX_KEYWORDS.some(kw => lower.includes(kw))) {
      hint = '💡 Dica: este prompt parece ser de bugfix. Considere usar `/mustard:bugfix` para delegar via Task (Sonnet) e economizar tokens vs Opus direto.';
    } else if (FEATURE_KEYWORDS.some(kw => lower.includes(kw))) {
      hint = '💡 Dica: este prompt parece ser de feature/enhancement. Considere usar `/mustard:feature` para rodar o pipeline estruturado (ANALYZE → PLAN → EXECUTE) com gates e auto-detect de Light scope.';
    } else if (ANALYSIS_KEYWORDS.some(kw => lower.includes(kw))) {
      hint = '💡 Dica: este prompt parece ser de análise. Considere usar `/mustard:task` para delegar via Task (Sonnet/Haiku) e economizar tokens vs Opus direto.';
    }

    if (!hint) { process.exit(0); }

    console.log(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'UserPromptSubmit',
        additionalContext: hint,
      },
    }));
    process.exit(0);
  } catch (err) {
    process.stderr.write(`[user-prompt-hint] Error: ${err.message}\n`);
    process.exit(0);
  }
});
