#!/usr/bin/env node
/**
 * ENFORCEMENT: Context Compilation validation
 *
 * Blocks /feature, /bugfix, /feature-team, /bugfix-team if compiled contexts:
 * - Do not exist for required agents
 * - Are outdated (hash != current git commit)
 *
 * @version 1.1.0
 * @see mustard/cli/templates/commands/mustard/compile-context.md
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

// Agents that require compiled context (Task mode)
const REQUIRED_AGENTS_TASK = [
  'orchestrator',
  'backend',
  'frontend',
  'database',
  'bugfix',
  'review'
];

// Agents that require compiled context (Agent Teams mode)
const REQUIRED_AGENTS_TEAM = [
  'team-lead',
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

    // Only enforce for feature and bugfix skills (Task and Agent Teams modes)
    const enforcedSkills = [
      'mustard:feature', 'mustard:bugfix',
      'mustard:feature-team', 'mustard:bugfix-team',
      'feature', 'bugfix',
      'feature-team', 'bugfix-team'
    ];
    if (!enforcedSkills.includes(skillName)) {
      process.exit(0);
    }

    // Get current git commit hash
    const currentHash = getCurrentCommitHash();
    if (!currentHash) {
      // Not a git repo or git not available - skip validation
      process.exit(0);
    }

    // Determine if Agent Teams mode
    const isTeamMode = skillName.includes('team');

    // Check all required contexts
    const validation = validateContexts(currentHash, isTeamMode);

    if (!validation.valid) {
      blockWithMessage(validation.message, validation.missing, validation.outdated);
      return;
    }

    // All contexts valid - allow
    process.exit(0);

  } catch (err) {
    console.error('Hook error:', err.message);
    process.exit(0); // Don't block on hook errors
  }
});

/**
 * Get current git commit hash (short)
 */
function getCurrentCommitHash() {
  try {
    return execSync('git rev-parse --short HEAD', { encoding: 'utf8' }).trim();
  } catch {
    return null;
  }
}

/**
 * Find .claude folder
 */
function findClaudeFolder() {
  const cwd = process.cwd();
  const claudePath = path.join(cwd, '.claude');

  if (fs.existsSync(claudePath)) {
    return claudePath;
  }

  return null;
}

/**
 * Validate all required contexts exist and are up-to-date
 */
function validateContexts(currentHash, isTeamMode) {
  const claudeFolder = findClaudeFolder();
  const requiredAgents = isTeamMode ? REQUIRED_AGENTS_TEAM : REQUIRED_AGENTS_TASK;

  if (!claudeFolder) {
    return {
      valid: false,
      message: '.claude folder not found.',
      missing: requiredAgents,
      outdated: []
    };
  }

  const missing = [];
  const outdated = [];

  for (const agent of requiredAgents) {
    const contextPath = path.join(claudeFolder, 'prompts', `${agent}.context.md`);

    if (!fs.existsSync(contextPath)) {
      missing.push(agent);
      continue;
    }

    // Check hash in file
    const content = fs.readFileSync(contextPath, 'utf8');
    const hashMatch = content.match(/compiled-from-commit:\s*(\w+)/);
    const fileHash = hashMatch ? hashMatch[1] : null;

    if (!fileHash || fileHash !== currentHash) {
      outdated.push({
        agent,
        fileHash: fileHash || 'none',
        currentHash
      });
    }
  }

  if (missing.length > 0 || outdated.length > 0) {
    return {
      valid: false,
      message: buildErrorMessage(missing, outdated, currentHash),
      missing,
      outdated
    };
  }

  return { valid: true };
}

/**
 * Build detailed error message
 */
function buildErrorMessage(missing, outdated, currentHash) {
  const parts = [];

  if (missing.length > 0) {
    parts.push(`Missing contexts: ${missing.join(', ')}`);
  }

  if (outdated.length > 0) {
    const outdatedList = outdated.map(o =>
      `${o.agent} (has: ${o.fileHash}, need: ${currentHash})`
    ).join(', ');
    parts.push(`Outdated contexts: ${outdatedList}`);
  }

  return parts.join('\n');
}

/**
 * Block with helpful message
 */
function blockWithMessage(reason, missing, outdated) {
  const response = {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "block",
      permissionDecisionReason: `Context Compilation Required

${reason}

Compiled contexts help agents by:
- Providing project-specific patterns and conventions
- Including shared context (from context/shared/)
- Including agent-specific context (from context/{agent}/)
- Reducing token usage during implementation

Run /compile-context to update all contexts, then retry your command.

Current commit: ${getCurrentCommitHash() || 'unknown'}`
    }
  };
  console.log(JSON.stringify(response));
  process.exit(0);
}
