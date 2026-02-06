import type { ProjectInfo, GeneratedHooks, GeneratorOptions } from '../types.js';

/**
 * Generate hook files
 */
export function generateHooks(projectInfo: ProjectInfo, options: GeneratorOptions = {}): GeneratedHooks {
  const hooks: GeneratedHooks = {
    // Pipeline enforcement hook (always)
    'enforce-pipeline.js': generateEnforcePipelineHook()
  };

  // grepai enforcement hook (if grepai available)
  if (options.hasGrepai !== false) {
    hooks['enforce-grepai.js'] = generateEnforceGrepaiHook();
  }

  return hooks;
}

function generateEnforcePipelineHook(): string {
  return `/**
 * Hook: enforce-pipeline
 *
 * Enforces that code changes go through the proper pipeline.
 * Triggers on Edit/Write to code files.
 *
 * Exceptions:
 * - .md files (documentation)
 * - .json files (config)
 * - .yaml/.yml files (config)
 * - Files in .claude/, mustard/, spec/ directories
 */

export default {
  name: 'enforce-pipeline',

  // Hook configuration
  hooks: {
    preToolCall: {
      tools: ['Edit', 'Write'],
      handler: 'checkPipeline'
    }
  },

  /**
   * Check if there's an active pipeline before allowing code edits
   */
  checkPipeline(context) {
    const { toolName, parameters } = context;
    const filePath = parameters.file_path || '';

    // Skip non-code files
    if (isExemptFile(filePath)) {
      return { allowed: true };
    }

    // Check for active pipeline
    // Note: This is a reminder hook - actual enforcement is done by Claude
    // following the CLAUDE.md instructions

    return {
      allowed: true,
      message: \`📋 REMINDER: Ensure you're following the pipeline for code changes.
Use /feature or /bugfix to start a proper pipeline.\`
    };
  }
};

function isExemptFile(filePath) {
  const exemptExtensions = ['.md', '.json', '.yaml', '.yml', '.txt', '.env.example'];
  const exemptDirs = ['.claude', 'mustard', 'spec', 'node_modules', 'bin', 'obj'];

  // Check extension
  if (exemptExtensions.some(ext => filePath.endsWith(ext))) {
    return true;
  }

  // Check directory
  if (exemptDirs.some(dir => filePath.includes(\`/\${dir}/\`) || filePath.includes(\`\\\\\${dir}\\\\\`))) {
    return true;
  }

  return false;
}
`;
}

function generateEnforceGrepaiHook(): string {
  return `/**
 * Hook: enforce-grepai
 *
 * Encourages use of grepai for semantic code search.
 * Triggers on Grep/Glob tools.
 *
 * Note: This is an advisory hook, not a blocker.
 */

export default {
  name: 'enforce-grepai',

  // Hook configuration
  hooks: {
    preToolCall: {
      tools: ['Grep', 'Glob'],
      handler: 'suggestGrepai'
    }
  },

  /**
   * Suggest using grepai for better semantic search
   */
  suggestGrepai(context) {
    const { toolName, parameters } = context;

    // Allow but remind
    return {
      allowed: true,
      message: \`💡 SUGGESTION: Consider using grepai for semantic search.

grepai provides:
- Semantic understanding of code intent
- Better results for complex queries
- Call graph tracing

Example:
  grepai_search({ query: "your search" })
  grepai_trace_callers({ symbol: "FunctionName" })
\`
    };
  }
};
`;
}
