#!/usr/bin/env node
/**
 * ENFORCEMENT L0+L2: Asks confirmation for code edits
 *
 * Configuration files are automatically allowed.
 * Code files ask for confirmation (Claude checks memory MCP).
 *
 * @version 1.0.0
 * @see mustard/claude/core/enforcement.md
 */

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const filePath = data.tool_input?.file_path || '';

    // Configuration files - ALLOW automatically
    if (isConfigFile(filePath)) {
      process.exit(0);
    }

    // Code file - ASK
    // Claude checks memory MCP to decide
    const response = {
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "ask",
        permissionDecisionReason: `⚠️ L0+L2: Code edit detected

File: ${filePath}

Check via memory MCP:
1. Is there an active pipeline? (search_nodes "pipeline phase")
2. Is it in "implement" phase?

If NO active pipeline:
→ Use /feature or /bugfix to start

If pipeline exists but not in "implement":
→ Approve the spec first with /approve`
      }
    };
    console.log(JSON.stringify(response));
    process.exit(0);
  } catch (err) {
    console.error('Hook error:', err.message);
    process.exit(0);
  }
});

/**
 * Checks if the file is a configuration file (allowed without pipeline)
 * @param {string} filePath - File path
 * @returns {boolean}
 */
function isConfigFile(filePath) {
  const patterns = [
    // Documentation and configuration
    /\.md$/i,
    /\.json$/i,
    /\.yaml$/i,
    /\.yml$/i,
    /\.env/i,
    /\.gitignore$/i,
    /\.config\./i,
    /\.editorconfig$/i,

    // Special folders (always allowed)
    /\.claude[\/\\]/i,
    /spec[\/\\]/i,
    /mustard[\/\\]/i,

    // CI/CD files
    /\.github[\/\\]/i,
    /Dockerfile/i,
    /docker-compose/i,

    // Lock files (generated)
    /package-lock\.json$/i,
    /pnpm-lock\.yaml$/i,
    /\.lock$/i
  ];

  return patterns.some(p => p.test(filePath));
}
