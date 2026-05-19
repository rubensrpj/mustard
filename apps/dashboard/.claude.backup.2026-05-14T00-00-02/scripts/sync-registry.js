#!/usr/bin/env bun
'use strict';

/**
 * sync-registry.js
 *
 * Generates .claude/entity-registry.json v4.0 by orchestrating per-stack scanners.
 * This script is intentionally thin — all scanning logic lives in registry/.
 *
 * Usage:
 *   bun .claude/scripts/sync-registry.js          # Skip if registry is populated
 *   bun .claude/scripts/sync-registry.js --force  # Regenerate unconditionally
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
const { discoverClusters, computeFolderFrequency } = require('./registry/cluster-discovery');
const { computeProjectConventions } = require('./registry/project-conventions');
const { enrichDescriptions } = require('./registry/description-enricher');

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
    if (!target.patterns) target.patterns = {};
    // _discovered is per-subproject (each cluster is tagged with subprojectName);
    // concatenate so the orchestrator can slice clusters per agent later.
    if (Array.isArray(source.patterns._discovered)) {
      target.patterns._discovered = [
        ...(Array.isArray(target.patterns._discovered) ? target.patterns._discovered : []),
        ...source.patterns._discovered,
      ];
    }
    // Other pattern keys (folderFrequency, conventions, etc.): shallow-merge
    // (last writer wins — they describe the stack, not the subproject).
    const { _discovered: _, ...otherSourcePatterns } = source.patterns;
    target.patterns = Object.assign({}, target.patterns, otherSourcePatterns);
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

      // Run generic cluster discovery (agnostic — discovers by structure, not by tech name)
      // Pass sub.name so each cluster carries its origin subproject (used by /scan
      // orchestrator to slice clusters per agent prompt).
      const discovered = discoverClusters(subPath, stackId, sub.name);
      if (discovered.length > 0) {
        if (!result.patterns) result.patterns = {};
        result.patterns._discovered = discovered;
        console.log(`    ${discovered.length} structural cluster(s) discovered`);
      }

      // Compute folder-segment frequency across the entire subproject.
      // Downstream (skill-generator) uses this as an agnostic stopword source:
      // segments appearing in >60% of folders are treated as structural noise.
      const folderFrequency = computeFolderFrequency(subPath, stackId);
      if (folderFrequency.totalFolders > 0) {
        if (!result.patterns) result.patterns = {};
        result.patterns._folderFrequency = folderFrequency;
      }

      // Compute declarative project conventions (naming, etc.) from filesystem.
      const conventions = computeProjectConventions(subPath, stackId);
      if (conventions && conventions.naming && conventions.naming.total > 0) {
        if (!result.patterns) result.patterns = {};
        result.patterns._conventions = conventions;
      }

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

  // 5b. Enrich entities with doc-comment descriptions (glossary).
  // Stack-agnostic: scans the canonical ref file for doc-comment immediately
  // above the entity declaration. Fail-open per entity.
  let enrichSummary = { enriched: 0, scanned: 0 };
  try {
    enrichSummary = enrichDescriptions(registry, ROOT);
  } catch (err) {
    console.error('Description enrichment failed (continuing):', err.message);
  }

  // 6. Write output
  fs.mkdirSync(path.dirname(REGISTRY_PATH), { recursive: true });
  const output = JSON.stringify(registry, null, 2) + '\n';
  fs.writeFileSync(REGISTRY_PATH, output, 'utf-8');

  const eCount = Object.keys(registry.e).length;
  const enumCount = Object.keys(registry._enums).length;
  const patternStacks = Object.keys(registry._patterns);

  console.log(`\nGenerated entity-registry.json v4.0`);
  console.log(`  ${eCount} entities, ${enumCount} enums, patterns: [${patternStacks.join(', ')}]`);
  if (enrichSummary.scanned > 0) {
    console.log(`  Glossary: ${enrichSummary.enriched}/${enrichSummary.scanned} entities enriched with doc-comment descriptions`);
  }
  console.log(`  Written to: ${REGISTRY_PATH}`);
}

main();
