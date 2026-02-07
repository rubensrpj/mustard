#!/usr/bin/env node
/**
 * ENFORCEMENT: Pipeline validation
 *
 * Reminds about pipeline when Edit/Write is used on code files.
 *
 * Exceptions:
 * - .md, .json, .yaml/.yml, .txt, .env.example files
 * - Files in .claude/, mustard/, spec/, node_modules/, bin/, obj/ directories
 *
 * @version 1.0.0
 */

const path = require('path');

const EXEMPT_EXTENSIONS = ['.md', '.json', '.yaml', '.yml', '.txt', '.env.example'];
const EXEMPT_DIRS = ['.claude', 'mustard', 'spec', 'node_modules', 'bin', 'obj'];

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const filePath = data.tool_input?.file_path || '';

    // Skip exempt files
    if (isExemptFile(filePath)) {
      process.exit(0);
    }

    // Allow but remind about pipeline
    const response = {
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "allow",
        permissionDecisionReason: "REMINDER: Ensure you're following the pipeline (Explore → Spec → Implement → Review) for code changes."
      }
    };
    console.log(JSON.stringify(response));
    process.exit(0);

  } catch (err) {
    console.error('Hook error:', err.message);
    process.exit(0);
  }
});

function isExemptFile(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  if (EXEMPT_EXTENSIONS.includes(ext)) {
    return true;
  }

  const normalized = filePath.replace(/\\/g, '/');
  if (EXEMPT_DIRS.some(dir => normalized.includes('/' + dir + '/') || normalized.startsWith(dir + '/'))) {
    return true;
  }

  return false;
}
