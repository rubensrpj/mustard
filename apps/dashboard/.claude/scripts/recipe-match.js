#!/usr/bin/env node
/**
 * RECIPE-MATCH: Match a recipe from .claude/recipes/ by entity and operation
 *
 * Reads structured recipe JSON files, matches by operation (and entity requirement),
 * and outputs the matched recipe with resolved file paths.
 *
 * Usage: node .claude/scripts/recipe-match.js --entity <name> --operation <type> [--subproject <path>]
 * Output: JSON to stdout with matched recipe + resolved paths
 * Exit 0 with empty output if no match or no recipes dir
 *
 * @version 1.0.0
 */

'use strict';

const fs = require('fs');
const path = require('path');

/**
 * Convert a string to PascalCase (already assumed PascalCase from user input,
 * but ensure first letter is uppercase just in case).
 * @param {string} str
 * @returns {string}
 */
function toPascalCase(str) {
  if (!str) return str;
  return str.charAt(0).toUpperCase() + str.slice(1);
}

/**
 * Resolve path pattern placeholders for a given entity and subproject.
 * @param {string} pattern
 * @param {string} entity
 * @param {string|null} subproject
 * @param {string} cwd
 * @returns {string}
 */
function resolvePattern(pattern, entity, subproject, cwd) {
  const entityPascal = toPascalCase(entity);
  const entityLower = entity.toLowerCase();

  let resolved = pattern;

  // Replace {Entity} and {entity}
  resolved = resolved.replace(/\{Entity\}/g, entityPascal);
  resolved = resolved.replace(/\{entity\}/g, entityLower);

  // Replace {subproject}
  if (subproject) {
    resolved = resolved.replace(/\{subproject\}/g, subproject);
  }

  // Replace {backend}, {frontend}, {admin} with directory lookup
  const placeholders = ['backend', 'frontend', 'admin'];
  for (const placeholder of placeholders) {
    if (resolved.includes(`{${placeholder}}`)) {
      const found = findDirByConvention(cwd, placeholder);
      if (found) {
        resolved = resolved.replace(new RegExp(`\\{${placeholder}\\}`, 'g'), found);
      }
      // If not found, leave placeholder as-is
    }
  }

  return resolved;
}

/**
 * Look for a directory at cwd level that matches a common naming convention for the placeholder.
 * Returns the directory name if found, null otherwise.
 * @param {string} cwd
 * @param {string} placeholder
 * @returns {string|null}
 */
function findDirByConvention(cwd, placeholder) {
  const candidates = {
    backend: ['backend', 'Backend', 'api', 'Api', 'server', 'Server', 'src'],
    frontend: ['frontend', 'Frontend', 'web', 'Web', 'client', 'Client', 'app', 'App'],
    admin: ['admin', 'Admin', 'dashboard', 'Dashboard'],
  };

  const names = candidates[placeholder] || [placeholder];

  try {
    for (const name of names) {
      const candidate = path.join(cwd, name);
      if (fs.existsSync(candidate) && fs.statSync(candidate).isDirectory()) {
        return name;
      }
    }
  } catch {
    // Ignore errors — return null
  }

  return null;
}

function main() {
  try {
    const cwd = process.cwd();
    const args = process.argv.slice(2);

    // Parse --entity
    const entityIdx = args.indexOf('--entity');
    const entity = entityIdx >= 0 && args[entityIdx + 1] ? args[entityIdx + 1] : null;

    // Parse --operation
    const opIdx = args.indexOf('--operation');
    const operation = opIdx >= 0 && args[opIdx + 1] ? args[opIdx + 1] : null;

    // Parse --subproject
    const subIdx = args.indexOf('--subproject');
    const subproject = subIdx >= 0 && args[subIdx + 1] ? args[subIdx + 1] : null;

    // If either entity or operation is missing, exit silently
    if (!entity || !operation) {
      process.exit(0);
    }

    const recipesDir = path.join(cwd, '.claude', 'recipes');

    // If directory doesn't exist, exit silently
    if (!fs.existsSync(recipesDir)) {
      process.exit(0);
    }

    let entries;
    try {
      entries = fs.readdirSync(recipesDir);
    } catch {
      process.exit(0);
    }

    const jsonFiles = entries.filter(f => f.endsWith('.json'));

    const operationLower = operation.toLowerCase();
    let matched = null;

    for (const file of jsonFiles) {
      let recipe;
      try {
        const raw = fs.readFileSync(path.join(recipesDir, file), 'utf8');
        recipe = JSON.parse(raw);
      } catch {
        // Invalid JSON — skip this file
        continue;
      }

      // Validate recipe has operations array
      if (!Array.isArray(recipe.operations)) {
        continue;
      }

      // Check if operation matches (case-insensitive)
      const operationMatches = recipe.operations.some(
        op => typeof op === 'string' && op.toLowerCase() === operationLower
      );

      if (!operationMatches) {
        continue;
      }

      // If recipe requires entity but none provided, skip
      if (recipe.requires_entity && !entity) {
        continue;
      }

      // First match wins
      matched = recipe;
      break;
    }

    if (!matched) {
      // No match — exit silently with empty output
      process.exit(0);
    }

    // Resolve file paths
    const resolvedFiles = Array.isArray(matched.files)
      ? matched.files.map(f => {
          const pattern = typeof f.pattern === 'string' ? f.pattern : '';
          return {
            resolved_path: resolvePattern(pattern, entity, subproject, cwd),
            action: f.action,
            hint: f.hint,
          };
        })
      : [];

    const output = {
      recipe: matched.name,
      entity: entity,
      operation: operation,
      description: matched.description || '',
      files: resolvedFiles,
      checklist: Array.isArray(matched.checklist) ? matched.checklist : [],
    };

    process.stdout.write(JSON.stringify(output, null, 2) + '\n');
  } catch (err) {
    process.stderr.write(`[recipe-match] Error: ${err.message}\n`);
  }

  process.exit(0);
}

main();
