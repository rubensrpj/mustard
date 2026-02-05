/**
 * Hook: enforce-pipeline (HYBRID MODE)
 *
 * Enforces L0 Universal Delegation rule:
 * - BLOCKS: Source code files (.ts, .js, .tsx, .jsx, .cs, .py, etc.)
 * - ALLOWS with advisory: Configs, docs, and .claude/ files
 *
 * This ensures ALL code modifications happen via Task (separate context).
 */

// Source code extensions that MUST be delegated
const CODE_EXTENSIONS = [
  '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs',
  '.cs', '.py', '.java', '.go', '.rs', '.rb',
  '.php', '.swift', '.kt', '.scala', '.c', '.cpp', '.h'
];

// Extensions that are allowed with advisory only
const CONFIG_EXTENSIONS = ['.md', '.json', '.yaml', '.yml', '.txt', '.env.example', '.toml', '.xml'];

// Directories that are exempt from blocking
const EXEMPT_DIRS = ['.claude', 'mustard', 'spec', 'node_modules', 'bin', 'obj', 'dist', 'build'];

export default {
  name: 'enforce-pipeline',

  hooks: {
    preToolCall: {
      tools: ['Edit', 'Write'],
      handler: 'checkPipeline'
    }
  },

  /**
   * HYBRID MODE: Block code files, allow configs with advisory
   */
  checkPipeline(context) {
    const { parameters } = context;
    const filePath = parameters.file_path || '';

    // Check if in exempt directory
    if (isExemptDir(filePath)) {
      return { allowed: true };
    }

    // Check if it's a config/doc file (allow with advisory)
    if (isConfigFile(filePath)) {
      return {
        allowed: true,
        message: `📋 Advisory: Modifying config/doc file. Consider if this should be delegated.`
      };
    }

    // Check if it's source code (BLOCK)
    if (isSourceCode(filePath)) {
      return {
        allowed: false,
        message: `🚫 BLOCKED: Source code modifications MUST be delegated via Task tool.

L0 Universal Delegation Rule:
- Use /feature or /bugfix to start a pipeline
- Or delegate directly via Task(general-purpose)

The parent context should ONLY coordinate, not implement code.`
      };
    }

    // Unknown file type - allow with warning
    return {
      allowed: true,
      message: `⚠️ Unknown file type. Consider delegating via Task if this is code.`
    };
  }
};

function isSourceCode(filePath) {
  return CODE_EXTENSIONS.some(ext => filePath.toLowerCase().endsWith(ext));
}

function isConfigFile(filePath) {
  return CONFIG_EXTENSIONS.some(ext => filePath.toLowerCase().endsWith(ext));
}

function isExemptDir(filePath) {
  const normalizedPath = filePath.replace(/\\/g, '/').toLowerCase();
  return EXEMPT_DIRS.some(dir =>
    normalizedPath.includes(`/${dir}/`) ||
    normalizedPath.startsWith(`${dir}/`) ||
    normalizedPath === dir
  );
}
