#!/usr/bin/env node
'use strict';
// analyze-validation.js — WARN-level spec validator (never blocks pipeline)
const fs = require('fs');
const path = require('path');

const LAYER_EXTENSIONS = {
  Backend:  ['.ts', '.cs', '.py', '.go', '.rs'],
  Frontend: ['.tsx', '.jsx', '.vue', '.svelte', '.html', '.css'],
  Database: ['.sql', '.prisma', 'schema.ts'],
  Mobile:   ['.swift', '.kt', '.dart'],
};

function run() {
  // Resolve specPath from --spec flag or stdin JSON
  let specPath;
  const flagIdx = process.argv.indexOf('--spec');
  if (flagIdx !== -1 && process.argv[flagIdx + 1]) {
    specPath = process.argv[flagIdx + 1];
  } else {
    try {
      const stdin = fs.readFileSync('/dev/stdin', 'utf8').trim();
      if (stdin) specPath = JSON.parse(stdin).specPath;
    } catch (_) { /* no stdin */ }
  }

  if (!specPath) {
    console.log(JSON.stringify({ ok: false, issues: [{ severity: 'ERROR', type: 'validator-crash', message: 'No spec path provided. Use --spec <path> or stdin JSON {specPath}' }] }));
    process.exit(1);
  }

  const absPath = path.resolve(specPath);
  if (!fs.existsSync(absPath)) {
    console.log(JSON.stringify({ ok: false, issues: [{ severity: 'ERROR', type: 'validator-crash', message: `Spec file not found: ${absPath}` }] }));
    process.exit(1);
  }

  const content = fs.readFileSync(absPath, 'utf8');
  const lines = content.split('\n');
  const issues = [];

  // --- Parse ## Files section ---
  let inFiles = false;
  const fileLines = [];
  for (const line of lines) {
    if (/^## Files/.test(line)) { inFiles = true; continue; }
    if (inFiles && /^##/.test(line)) { inFiles = false; }
    if (inFiles) fileLines.push(line);
  }

  // --- Validation 1: Layer coverage ---
  const layerMatches = content.match(/###\s+(Backend|Frontend|Database|Mobile)\s+Agent/g) || [];
  const declaredLayers = layerMatches.map(h => h.match(/###\s+(\w+)\s+Agent/)[1]);
  const filesText = fileLines.join('\n');
  for (const layer of declaredLayers) {
    const exts = LAYER_EXTENSIONS[layer] || [];
    const hasMatch = exts.some(ext => filesText.includes(ext));
    if (!hasMatch) {
      issues.push({ severity: 'WARN', type: 'layer-gap', message: `Spec declares ${layer} Agent but Files has no ${layer} extensions` });
    }
  }

  // --- Validation 2: File refs resolvable ---
  const refRegex = /`([\w./-]+\.\w+)`/g;
  let match;
  while ((match = refRegex.exec(filesText)) !== null) {
    const ref = match[1];
    const lineWithRef = fileLines.find(l => l.includes('`' + ref + '`')) || '';
    const isCreate = lineWithRef.toLowerCase().includes('(create)');
    if (!isCreate && !fs.existsSync(path.resolve(path.dirname(absPath), ref)) && !fs.existsSync(path.resolve(ref))) {
      issues.push({ severity: 'WARN', type: 'missing-file', file: ref, message: `File referenced but not found and not marked (create)` });
    }
  }

  // --- Validation 3: Task decomposition sane ---
  const agentHeaderRe = /###\s+(\S.*?)\s+Agent/g;
  let agentMatch;
  while ((agentMatch = agentHeaderRe.exec(content)) !== null) {
    const agentName = agentMatch[1];
    const start = agentMatch.index + agentMatch[0].length;
    const rest = content.slice(start);
    const nextSection = rest.search(/\n#{2,3}\s/);
    const block = nextSection === -1 ? rest : rest.slice(0, nextSection);
    const tasks = (block.match(/- \[[ x]\]/g) || []).length;
    if (tasks < 2 || tasks > 10) {
      issues.push({ severity: 'WARN', type: 'task-count', message: `${agentName} Agent has ${tasks} tasks (expected 2-10)` });
    }
  }

  // --- Validation 4: Extended Light scope requires entity in registry ---
  const scopeMatch = content.match(/scope:\s*["']?(extended-light)["']?/i);
  if (scopeMatch) {
    const entityMatch = content.match(/entity:\s*["']?(\w+)["']?/i);
    const entityName = entityMatch ? entityMatch[1] : null;
    if (entityName) {
      const registryPath = path.join(process.cwd(), '.claude', 'entity-registry.json');
      try {
        if (fs.existsSync(registryPath)) {
          const registry = fs.readFileSync(registryPath, 'utf8');
          if (!registry.toLowerCase().includes(entityName.toLowerCase())) {
            issues.push({ severity: 'WARN', type: 'scope-mismatch', message: `Extended Light scope requires entity "${entityName}" in registry, but not found. Reclassify as Full.` });
          }
        } else {
          issues.push({ severity: 'WARN', type: 'scope-mismatch', message: `Extended Light scope requires entity-registry.json, but file not found. Reclassify as Full.` });
        }
      } catch (_) { /* fail-open */ }
    }
  }

  console.log(JSON.stringify({ ok: issues.length === 0, issues }));
}

try {
  run();
} catch (err) {
  console.log(JSON.stringify({ ok: false, issues: [{ severity: 'ERROR', type: 'validator-crash', message: String(err) }] }));
  process.exit(1);
}
