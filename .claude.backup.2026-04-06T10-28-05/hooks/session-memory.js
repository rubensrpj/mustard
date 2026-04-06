#!/usr/bin/env node
/**
 * SESSION-MEMORY: Injects persistent memory into session context
 * @version 1.0.0
 */
const fs = require('fs');
const path = require('path');
const { shouldRun } = require('./_lib/hook-env.js');

const MAX_CHARS = 2000;

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('session-memory')) { process.exit(0); }
    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();
    const memDir = path.join(cwd, '.claude', 'memory');

    const parts = [];

    // Load decisions (last 10)
    const decisions = loadEntries(path.join(memDir, 'decisions.json'), 10);
    if (decisions.length > 0) {
      parts.push('## Recent Decisions');
      decisions.forEach(d => parts.push(`- [${d.source}] ${d.content}`));
    }

    // Load lessons (last 10)
    const lessons = loadEntries(path.join(memDir, 'lessons.json'), 10);
    if (lessons.length > 0) {
      parts.push('## Lessons Learned');
      lessons.forEach(l => parts.push(`- [${l.source}] ${l.content}`));
    }

    if (parts.length > 0) {
      let context = parts.join('\n');
      if (context.length > MAX_CHARS) context = context.slice(0, MAX_CHARS) + '\n...truncated';

      console.log(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'SessionStart',
          additionalContext: `[Persistent Memory]\n${context}`
        }
      }));
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[session-memory] Error: ${err.message}\n`);
    process.exit(0);
  }
});

function loadEntries(filePath, max) {
  try {
    if (!fs.existsSync(filePath)) return [];
    const data = JSON.parse(fs.readFileSync(filePath, 'utf8'));
    const entries = data.entries || [];
    return entries.slice(-max);
  } catch { return []; }
}
