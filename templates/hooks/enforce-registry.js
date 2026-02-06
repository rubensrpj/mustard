#!/usr/bin/env node
/**
 * ENFORCEMENT: Entity Registry validation (UserPromptSubmit)
 *
 * Blocks /feature, /bugfix, /feature-team, /bugfix-team if entity-registry.json:
 * - Does not exist
 * - Is empty (no entities)
 * - Has no _patterns defined
 *
 * @version 2.0.0
 * @see mustard/cli/templates/core/entity-registry-spec.md
 */

const fs = require('fs');
const path = require('path');

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const userMessage = data.user_message || '';

    // Check if user is invoking a pipeline skill
    const pipelinePattern = /^\s*\/(feature|bugfix|feature-team|bugfix-team)(\s|$)/i;
    if (!pipelinePattern.test(userMessage)) {
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

/**
 * Find entity-registry.json in .claude folder
 */
function findRegistry() {
  const cwd = process.cwd();
  const registryPath = path.join(cwd, '.claude', 'entity-registry.json');

  if (fs.existsSync(registryPath)) {
    return registryPath;
  }

  return null;
}

/**
 * Validate registry has required content
 */
function validateRegistry(registry) {
  // Check version
  if (!registry._meta?.version?.startsWith('3.')) {
    return {
      valid: false,
      message: `Registry version ${registry._meta?.version || 'unknown'} is outdated. Run /sync-registry to update to v3.1.`
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

  // Check _patterns is defined (helps with finding reference entities)
  if (!registry._patterns || Object.keys(registry._patterns).length === 0) {
    return {
      valid: false,
      message: `Registry has ${entities.length} entities but no _patterns defined. Run /sync-registry to add reference patterns.`
    };
  }

  return { valid: true };
}

/**
 * Block with helpful message
 */
function blockWithMessage(reason) {
  const response = {
    hookSpecificOutput: {
      hookEventName: "UserPromptSubmit",
      decision: "block",
      reason: `Entity Registry Required

${reason}

The entity registry helps save tokens by:
- Listing all entities and their relationships
- Providing reference entities for each pattern type
- Cataloging enum values

Run /sync-registry to update the registry, then retry your command.`
    }
  };
  console.log(JSON.stringify(response));
  process.exit(0);
}
