#!/usr/bin/env bun
'use strict';
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
const { shouldRun } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');
const { formatGateMessage } = require('./_lib/gate-message.js');

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('enforce-registry')) { process.exit(0); }
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
      emitMetric('enforce-registry', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: 'blocked-no-registry',
        extras: { reason: 'not-found', version: null, category: 'prevention' },
      });
      blockWithMessage(formatGateMessage({
        gate: 'Registry Gate',
        what: 'Entity registry not found',
        why: '/feature and /bugfix need it to resolve known entities',
        exit: 'run /sync-registry, then retry the command',
      }));
      return;
    }

    // Validate registry content
    const registry = JSON.parse(fs.readFileSync(registryPath, 'utf8'));
    const validation = validateRegistry(registry);

    if (!validation.valid) {
      const version = registry && registry._meta && registry._meta.version ? registry._meta.version : null;
      const reason = validation.reason || 'invalid';
      emitMetric('enforce-registry', {
        tokensAffected: 0,
        tokensSaved: 0,
        note: reason === 'stale-version' ? 'blocked-stale-version' : 'blocked-' + reason,
        extras: { reason, version, category: 'prevention' },
      });
      blockWithMessage(validation.message);
      return;
    }

    // Registry is valid - allow
    process.exit(0);

  } catch (err) {
    process.stderr.write(`[enforce-registry] Parse error: ${err.message}\n`);
    process.exit(0);
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
      reason: 'stale-version',
      message: formatGateMessage({
        gate: 'Registry Gate',
        what: 'Registry version ' + (registry._meta?.version || 'unknown') + ' is outdated',
        why: '/feature and /bugfix expect schema v3.1',
        exit: 'run /sync-registry to update the registry',
      }),
    };
  }

  // Check entities exist
  const entities = Object.keys(registry.e || {}).filter(k => k !== '_placeholder');
  if (entities.length === 0) {
    return {
      valid: false,
      reason: 'no-entities',
      message: formatGateMessage({
        gate: 'Registry Gate',
        what: 'Registry has no entities',
        why: 'the pipeline cannot resolve any known entity',
        exit: 'run /sync-registry to populate the registry',
      }),
    };
  }

  // Check _patterns is defined
  if (!registry._patterns || Object.keys(registry._patterns).length === 0) {
    return {
      valid: false,
      reason: 'no-patterns',
      message: formatGateMessage({
        gate: 'Registry Gate',
        what: 'Registry has ' + entities.length + ' entities but no _patterns defined',
        why: 'the pipeline needs reference patterns to scaffold code',
        exit: 'run /sync-registry to add reference patterns',
      }),
    };
  }

  return { valid: true };
}

function blockWithMessage(reason) {
  const response = {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "block",
      permissionDecisionReason: reason,
    }
  };
  console.log(JSON.stringify(response));
  process.exit(0);
}
