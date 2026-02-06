#!/usr/bin/env node
/**
 * ENFORCEMENT L1: Blocks Grep/Glob, forces use of grepai MCP
 *
 * grepai offers superior semantic search and should be used
 * instead of simple text search tools.
 *
 * @version 1.0.0
 * @see mustard/cli/templates/core/enforcement.md
 */

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const response = {
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: `⛔ L1: Grep/Glob FORBIDDEN

Use grepai MCP instead:

  grepai_search({ query: "..." })
  grepai_trace_callers({ symbol: "..." })
  grepai_trace_callees({ symbol: "..." })

These tools provide superior semantic search:
- Understands context and intent
- Finds semantically related code
- Maps dependencies automatically

To search files by name pattern, use:
  grepai_search({ query: "*.ts files in modules folder" })`
      }
    };
    console.log(JSON.stringify(response));
    process.exit(0);
  } catch (err) {
    console.error('Hook error:', err.message);
    process.exit(0);
  }
});
