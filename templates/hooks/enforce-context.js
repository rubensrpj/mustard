#!/usr/bin/env node
/**
 * ENFORCEMENT: Memory MCP Context validation (Advisory)
 *
 * Checks if AgentContext entities exist in Memory MCP.
 * Advisory only — allows execution with a reminder to
 * run /sync-context if Memory MCP is empty.
 *
 * Cannot check MCP directly from a hook, so validates
 * that context source files exist in .claude/context/
 * and reminds to load them via /sync-context.
 *
 * @version 2.0.0
 */

const fs = require('fs');
const path = require('path');

// Agents that require context in Memory MCP
const REQUIRED_AGENTS = [
  'orchestrator',
  'backend',
  'frontend',
  'database',
  'bugfix',
  'review'
];

// Expected Memory MCP entities per agent
const AGENT_CONTEXT_ENTITIES = {
  shared: ['AgentContext:shared:conventions'],
  orchestrator: ['AgentContext:orchestrator:orchestrator.core'],
  backend: ['AgentContext:backend:backend.core', 'AgentContext:backend:patterns'],
  frontend: ['AgentContext:frontend:frontend.core', 'AgentContext:frontend:patterns'],
  database: ['AgentContext:database:database.core', 'AgentContext:database:patterns'],
  bugfix: ['AgentContext:bugfix:bugfix.core'],
  review: ['AgentContext:review:review.core']
};

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

    // Validate context source files exist
    const validation = validateContextSources();

    if (!validation.valid) {
      advisoryAllow(validation.message);
      return;
    }

    // Source files exist - remind about Memory MCP
    advisoryRemind();

  } catch (err) {
    console.error('Hook error:', err.message);
    process.exit(0); // Don't block on hook errors
  }
});

function findClaudeFolder() {
  const cwd = process.cwd();
  const claudePath = path.join(cwd, '.claude');
  if (fs.existsSync(claudePath)) {
    return claudePath;
  }
  return null;
}

function validateContextSources() {
  const claudeFolder = findClaudeFolder();

  if (!claudeFolder) {
    return {
      valid: false,
      message: '.claude folder not found.'
    };
  }

  const contextDir = path.join(claudeFolder, 'context');
  if (!fs.existsSync(contextDir)) {
    return {
      valid: false,
      message: 'Context directory .claude/context/ not found.\nRun /sync-context to populate Memory MCP.'
    };
  }

  // Check shared context exists
  const sharedDir = path.join(contextDir, 'shared');
  const sharedFiles = getContextFiles(sharedDir);
  if (sharedFiles.length === 0) {
    return {
      valid: false,
      message: 'No shared context files in .claude/context/shared/.\nRun /sync-context to populate Memory MCP.'
    };
  }

  return { valid: true };
}

function getContextFiles(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir).filter(f => f.endsWith('.md') && f !== 'README.md');
}

function advisoryAllow(reason) {
  const response = {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "allow",
      permissionDecisionReason: 'Memory MCP Context Advisory\n\n' + reason
    }
  };
  console.log(JSON.stringify(response));
  process.exit(0);
}

function advisoryRemind() {
  const entities = Object.values(AGENT_CONTEXT_ENTITIES).flat();
  const response = {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "allow",
      permissionDecisionReason: 'Memory MCP Context: Ensure AgentContext entities are loaded.\n\nExpected entities: ' + entities.join(', ') + '\n\nUse mcp__memory__search_nodes("AgentContext") to verify.\nIf empty, run /sync-context to populate.'
    }
  };
  console.log(JSON.stringify(response));
  process.exit(0);
}
