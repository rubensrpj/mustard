#!/usr/bin/env node
'use strict';
/**
 * CONTEXT-BUDGET: PreToolUse hook that warns about excessive context loaded into subagents.
 * Advisory only — never blocks.
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

// Conservative regex: only match .claude/skills/**/*.md, .claude/context/**/*.md, SKILL.md references
const MD_REF_PATTERN = /\.claude\/(?:skills|context)\/[^\s"'`]+\.md|SKILL\.md/g;

const TOKEN_THRESHOLD = 50000;

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('context-budget')) { process.exit(0); }

    const data = JSON.parse(input);
    const projectDir = process.env.CLAUDE_PROJECT_DIR || data.cwd || process.cwd();
    const toolInput = (data.tool_input) || {};
    const prompt = toolInput.prompt || '';

    if (!prompt) { process.exit(0); }

    // Extract markdown file references from the prompt
    const matches = prompt.match(MD_REF_PATTERN) || [];

    // Deduplicate
    const uniquePaths = [...new Set(matches)];

    let totalBytes = 0;
    for (const relPath of uniquePaths) {
      try {
        const absPath = path.join(projectDir, relPath);
        if (fs.existsSync(absPath)) {
          totalBytes += fs.statSync(absPath).size;
        }
      } catch (e) { /* skip unreadable paths */ }
    }

    if (totalBytes === 0) { process.exit(0); }

    const estimatedTokens = Math.round(totalBytes / 4);

    if (estimatedTokens > TOKEN_THRESHOLD) {
      const kTokens = Math.round(estimatedTokens / 1000);
      console.log(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          additionalContext:
            '[Context Budget Advisory] Context budget warning: ~' + kTokens + 'K tokens will be loaded into this subagent (>50K threshold). Consider trimming recommended_skills or splitting the task.'
        }
      }));
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[context-budget] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
