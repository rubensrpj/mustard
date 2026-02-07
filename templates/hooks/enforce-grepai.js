#!/usr/bin/env node
/**
 * ENFORCEMENT: grepai over Grep/Glob for project-wide searches
 *
 * - BLOCK: Grep without a specific path (project-wide search)
 * - ALLOW: Grep with a specific path (targeted file/folder search)
 * - ALLOW: Glob without ** (simple, non-recursive search)
 * - ALLOW: Glob with ** + deep path (2+ levels = targeted search)
 * - BLOCK: Glob with ** + shallow/empty path (exploratory search)
 *
 * @version 3.0.0
 */

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const toolName = data.tool_name || '';
    const toolInput = data.tool_input || {};

    function block(reason) {
      console.log(JSON.stringify({ decision: 'block', reason }));
      process.exit(2);
    }

    // --- Grep enforcement ---
    if (toolName === 'Grep') {
      const searchPath = toolInput.path || '';
      if (searchPath && searchPath.trim() !== '') {
        process.exit(0); // Targeted search → allow
      }
      const pattern = toolInput.pattern || '';
      block(`Use grepai_search instead. Example: grepai_search({ query: "${pattern}" })`);
    }

    // --- Glob enforcement ---
    if (toolName === 'Glob') {
      const pattern = toolInput.pattern || '';
      const searchPath = (toolInput.path || '').replace(/\\/g, '/');

      // No ** → allow (non-recursive, simple search)
      if (!pattern.includes('**')) {
        process.exit(0);
      }

      // Path with 2+ levels → allow (targeted/directed search)
      const depth = searchPath.split('/').filter(Boolean).length;
      if (depth >= 2) {
        process.exit(0);
      }

      // ** with empty or shallow path → block (exploratory search)
      const searchTerm = pattern.replace(/\*\*/g, '').replace(/\*/g, '').replace(/\//g, ' ').trim();
      block(`Use grepai_search instead. Example: grepai_search({ query: "${searchTerm}" })`);
    }

    // All other tools → allow
    process.exit(0);

  } catch (err) {
    // On error, allow to avoid blocking legitimate usage
    process.exit(0);
  }
});
