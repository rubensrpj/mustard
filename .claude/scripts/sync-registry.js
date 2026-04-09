#!/usr/bin/env node
'use strict';

/**
 * sync-registry.js
 *
 * Generates .claude/entity-registry.json v4.0 by orchestrating per-stack scanners.
 * This script is intentionally thin — all scanning logic lives in registry/.
 *
 * Usage:
 *   node .claude/scripts/sync-registry.js          # Skip if registry is populated
 *   node .claude/scripts/sync-registry.js --force  # Regenerate unconditionally
 *
 * Architecture (SOLID):
 *   - scanner-loader.js  — Dependency Inversion, Open/Closed (add scanner = new file)
 *   - scanner-contract.js — Interface Segregation, Liskov Substitution
 *   - schema-builder.js  — Single Responsibility (JSON output)
 *   - file-utils.js      — Single Responsibility (filesystem helpers)
 *   - pluralize.js       — Single Responsibility (English pluralization)
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const { loadScanner, listAvailableScanners } = require('./registry/scanner-loader');
const { buildRegistry } = require('./registry/schema-builder');

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

// Root of the monorepo (parent of .claude/scripts/)
const ROOT = path.resolve(__dirname, '..', '..');
const REGISTRY_PATH = path.join(ROOT, '.claude', 'entity-registry.json');
const DETECT_SCRIPT = path.join(ROOT, '.claude', 'scripts', 'sync-detect.js');

// ---------------------------------------------------------------------------
// mergeResults — merge scan() output from two subprojects sharing a stack
// ---------------------------------------------------------------------------

/**
 * Merge source scan result into target (mutates target).
 * Maps are merged key-by-key; patterns are shallow-merged.
 *
 * @param {Object} target
 * @param {Object} source
 */
function mergeResults(target, source) {
  const mapKeys = ['entities', 'enums', 'interfaces', 'routes', 'dtos', 'services', 'repositories'];
  for (const key of mapKeys) {
    if (source[key] && source[key].size > 0) {
      if (!target[key]) target[key] = new Map();
      for (const [k, v] of source[key]) {
        target[key].set(k, v);
      }
    }
  }
  if (source.patterns && Object.keys(source.patterns).length > 0) {
    target.patterns = Object.assign({}, target.patterns, source.patterns);
  }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

function main() {
  let forceFlag = process.argv.includes('--force');

  // 1. Read current registry
  let currentRegistry = null;
  if (fs.existsSync(REGISTRY_PATH)) {
    try {
      currentRegistry = JSON.parse(fs.readFileSync(REGISTRY_PATH, 'utf-8'));
    } catch { /* invalid JSON, will regenerate */ }
  }

  // Auto-force upgrade if registry version < 4.0
  if (currentRegistry?._meta?.version && currentRegistry._meta.version < '4.0') {
    console.log(`Registry at v${currentRegistry._meta.version} — upgrading to v4.0.`);
    forceFlag = true;
  }

  // 2. Check if already populated
  if (currentRegistry && !forceFlag) {
    const entityCount = Object.keys(currentRegistry.e || {}).filter(k => !k.startsWith('_')).length;
    if (entityCount > 0) {
      console.log(`Registry v${currentRegistry._meta?.version || '?'} populated (${entityCount} entities). Use --force to regenerate.`);
      process.exit(0);
    }
  }

  // 3. Detect subprojects via sync-detect.js
  let detectResult;
  try {
    const output = execSync(`node "${DETECT_SCRIPT}"`, {
      cwd: ROOT,
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
      timeout: 30000,
      windowsHide: true,
    });
    detectResult = JSON.parse(output);
  } catch (err) {
    console.error('Failed to run sync-detect.js:', err.message);
    process.exit(1);
  }

  const subprojects = detectResult.subprojects || [];
  console.log(`Detected ${subprojects.length} subproject(s): ${subprojects.map(s => s.name).join(', ')}`);

  const available = listAvailableScanners();
  if (available.length === 0) {
    console.log('No scanner implementations found in registry/scanners/. Registry will be empty.');
  } else {
    console.log(`Available scanners: [${available.join(', ')}]`);
  }

  // 4. Scan each subproject
  /** @type {Map<string, Object>} */
  const scanResults = new Map();

  for (const sub of subprojects) {
    const subPath = path.join(ROOT, sub.path);
    const scanner = loadScanner(subPath, sub);

    if (!scanner) {
      console.log(`  ${sub.name}: no scanner available (agent: ${sub.agent})`);
      continue;
    }

    console.log(`  Scanning ${sub.name} (${scanner.constructor.name})...`);
    try {
      const result = scanner.scan();

      // Determine the stack key for this result
      const stackId = sub.stack || scanner.constructor.stackId || 'unknown';

      // Merge if same stack appears in multiple subprojects
      if (scanResults.has(stackId)) {
        mergeResults(scanResults.get(stackId), result);
      } else {
        scanResults.set(stackId, result);
      }

      const eCount = result.entities?.size || 0;
      const enumCount = result.enums?.size || 0;
      const routeCount = result.routes?.size || 0;
      const svcCount = result.services?.size || 0;
      console.log(`    ${eCount} entities, ${enumCount} enums, ${routeCount} route groups, ${svcCount} services`);
    } catch (err) {
      console.error(`    Scanner error for ${sub.name}: ${err.message}`);
    }
  }

  // 5. Build registry JSON
  const registry = buildRegistry({ scanResults });

  // 6. Write output
  fs.mkdirSync(path.dirname(REGISTRY_PATH), { recursive: true });
  const output = JSON.stringify(registry, null, 2) + '\n';
  fs.writeFileSync(REGISTRY_PATH, output, 'utf-8');

  const eCount = Object.keys(registry.e).length;
  const enumCount = Object.keys(registry._enums).length;
  const patternStacks = Object.keys(registry._patterns);

  console.log(`\nGenerated entity-registry.json v4.0`);
  console.log(`  ${eCount} entities, ${enumCount} enums, patterns: [${patternStacks.join(', ')}]`);
  console.log(`  Written to: ${REGISTRY_PATH}`);
}

main();
