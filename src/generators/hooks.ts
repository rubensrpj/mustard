import type { ProjectInfo, GeneratedHooks, GeneratorOptions } from '../types.js';

/**
 * Generate hook files
 */
export function generateHooks(projectInfo: ProjectInfo, options: GeneratorOptions = {}): GeneratedHooks {
  const hooks: GeneratedHooks = {
    // Pipeline enforcement hook (always)
    'enforce-pipeline.js': generateEnforcePipelineHook(),
    // Registry enforcement hook (always)
    'enforce-registry.js': generateEnforceRegistryHook(),
    // Context compilation enforcement hook (always)
    'enforce-context.js': generateEnforceContextHook()
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

function generateEnforceRegistryHook(): string {
  return `#!/usr/bin/env node
/**
 * ENFORCEMENT: Entity Registry validation
 *
 * Blocks /feature and /bugfix if entity-registry.json:
 * - Does not exist
 * - Is empty (no entities)
 * - Has no _patterns defined
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const toolName = data.tool_name || '';

    // Only check on Skill invocations for feature/bugfix
    if (toolName !== 'Skill') {
      process.exit(0);
    }

    const skillName = data.tool_input?.skill || '';

    // Only enforce for feature and bugfix skills
    if (!['mustard:feature', 'mustard:bugfix', 'feature', 'bugfix'].includes(skillName)) {
      process.exit(0);
    }

    // Find entity-registry.json
    const registryPath = findRegistry();

    if (!registryPath) {
      blockWithMessage('Entity registry not found. Run /sync-registry first.');
      return;
    }

    // Validate registry content
    const registry = JSON.parse(fs.readFileSync(registryPath, 'utf8'));
    const validation = validateRegistry(registry);

    if (!validation.valid) {
      blockWithMessage(validation.message);
      return;
    }

    // Registry is valid - allow
    process.exit(0);

  } catch (err) {
    console.error('Hook error:', err.message);
    process.exit(0); // Don't block on hook errors
  }
});

function findRegistry() {
  const cwd = process.cwd();
  const registryPath = path.join(cwd, '.claude', 'entity-registry.json');
  if (fs.existsSync(registryPath)) {
    return registryPath;
  }
  return null;
}

function validateRegistry(registry) {
  // Check version
  if (!registry._meta?.version?.startsWith('3.')) {
    return {
      valid: false,
      message: 'Registry version ' + (registry._meta?.version || 'unknown') + ' is outdated. Run /sync-registry to update to v3.1.'
    };
  }

  // Check entities exist
  const entities = Object.keys(registry.e || {}).filter(k => k !== '_placeholder');
  if (entities.length === 0) {
    return {
      valid: false,
      message: 'Registry has no entities. Run /sync-registry to populate.'
    };
  }

  // Check _patterns is defined
  if (!registry._patterns || Object.keys(registry._patterns).length === 0) {
    return {
      valid: false,
      message: 'Registry has ' + entities.length + ' entities but no _patterns defined. Run /sync-registry to add reference patterns.'
    };
  }

  return { valid: true };
}

function blockWithMessage(reason) {
  const response = {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "block",
      permissionDecisionReason: 'Entity Registry Required\\n\\n' + reason + '\\n\\nRun /sync-registry to update the registry, then retry your command.'
    }
  };
  console.log(JSON.stringify(response));
  process.exit(0);
}
`;
}

function generateEnforceContextHook(): string {
  return `#!/usr/bin/env node
/**
 * ENFORCEMENT: Context Compilation validation
 *
 * Blocks /feature and /bugfix if compiled contexts:
 * - Do not exist for required agents
 * - Are outdated (hash != current git commit)
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

// Agents that require compiled context
const REQUIRED_AGENTS = [
  'orchestrator',
  'backend',
  'frontend',
  'database',
  'bugfix',
  'review'
];

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const toolName = data.tool_name || '';

    // Only check on Skill invocations for feature/bugfix
    if (toolName !== 'Skill') {
      process.exit(0);
    }

    const skillName = data.tool_input?.skill || '';

    // Only enforce for feature and bugfix skills
    if (!['mustard:feature', 'mustard:bugfix', 'feature', 'bugfix'].includes(skillName)) {
      process.exit(0);
    }

    // Get current git commit hash
    const currentHash = getCurrentCommitHash();
    if (!currentHash) {
      // Not a git repo or git not available - skip validation
      process.exit(0);
    }

    // Check all required contexts
    const validation = validateContexts(currentHash);

    if (!validation.valid) {
      blockWithMessage(validation.message, currentHash);
      return;
    }

    // All contexts valid - allow
    process.exit(0);

  } catch (err) {
    console.error('Hook error:', err.message);
    process.exit(0); // Don't block on hook errors
  }
});

function getCurrentCommitHash() {
  try {
    return execSync('git rev-parse --short HEAD', { encoding: 'utf8' }).trim();
  } catch {
    return null;
  }
}

function findClaudeFolder() {
  const cwd = process.cwd();
  const claudePath = path.join(cwd, '.claude');
  if (fs.existsSync(claudePath)) {
    return claudePath;
  }
  return null;
}

function validateContexts(currentHash) {
  const claudeFolder = findClaudeFolder();

  if (!claudeFolder) {
    return {
      valid: false,
      message: '.claude folder not found.'
    };
  }

  const missing = [];
  const outdated = [];

  for (const agent of REQUIRED_AGENTS) {
    const contextPath = path.join(claudeFolder, 'prompts', agent + '.context.md');

    if (!fs.existsSync(contextPath)) {
      missing.push(agent);
      continue;
    }

    // Check hash in file
    const content = fs.readFileSync(contextPath, 'utf8');
    const hashMatch = content.match(/compiled-from-commit:\\s*(\\w+)/);
    const fileHash = hashMatch ? hashMatch[1] : null;

    if (!fileHash || fileHash !== currentHash) {
      outdated.push(agent + ' (has: ' + (fileHash || 'none') + ')');
    }
  }

  if (missing.length > 0 || outdated.length > 0) {
    const parts = [];
    if (missing.length > 0) {
      parts.push('Missing: ' + missing.join(', '));
    }
    if (outdated.length > 0) {
      parts.push('Outdated: ' + outdated.join(', '));
    }
    return {
      valid: false,
      message: parts.join('\\n')
    };
  }

  return { valid: true };
}

function blockWithMessage(reason, currentHash) {
  const response = {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "block",
      permissionDecisionReason: 'Context Compilation Required\\n\\n' + reason + '\\n\\nCompiled contexts help agents by:\\n- Providing project-specific patterns\\n- Including shared + agent-specific context\\n- Reducing token usage\\n\\nRun /compile-context to update, then retry.\\n\\nCurrent commit: ' + currentHash
    }
  };
  console.log(JSON.stringify(response));
  process.exit(0);
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
