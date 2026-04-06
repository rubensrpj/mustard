#!/usr/bin/env node
'use strict';
/**
 * GUARD-VERIFY: PostToolUse hook for Write|Edit
 *
 * Verifies production file edits against critical architectural rules.
 * Critical violations → block. All other checks are handled by review agents.
 *
 * @version 4.0.0
 */

const path = require("path");
const fs = require("fs");
const { shouldRun } = require('./_lib/hook-env.js');

const ROOT = process.env.CLAUDE_PROJECT_DIR || process.cwd();

const SKIP_PATTERNS = [
  /node_modules/, /\.next[/\\]/, /[/\\]bin[/\\]/, /[/\\]obj[/\\]/,
  /[/\\]dist[/\\]/, /[/\\]_backup[/\\]/, /\.claude[/\\]/, /\.git[/\\]/,
  /migrations[/\\]/,
];

const CRITICAL_RULES = [
  {
    pattern: /\bDbContext\b/i,
    scope: /Services?[/\\]/,
    exclude: /Repositor/,
    msg: "L7: DbContext proibido em Services — use Repository",
  },
  {
    pattern: /\b\w+Repository\b/,
    scope: /Services?[/\\]/,
    crossModule: true,
    msg: "L8: cross-module SEMPRE via Service, NUNCA Repository",
  },
  {
    pattern: /new\s+\w+(Service|Repository)\(/,
    scope: /\.cs$/,
    msg: "DIP: inject interface, NEVER concrete class",
  },
  {
    pattern: /\b(uint|int)\s+\w*[Ii]d\b/,
    scope: /\.cs$/,
    msg: "IDs must be Guid (UUIDv7), never int/uint",
  },
  {
    pattern: /directClient/,
    scope: /app[/\\]api[/\\]/,
    msg: "API routes NUNCA usam directClient — use backend-client.ts",
  },
];

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("end", () => {
  try {
    if (!shouldRun('guard-verify')) { process.exit(0); }
    const data = JSON.parse(input);
    const tool = data.tool_name || "";

    if (!["Write", "Edit"].includes(tool)) {
      process.stdout.write(JSON.stringify({ decision: "approve" }));
      return;
    }

    const filePath = data.tool_input?.file_path || "";
    if (!filePath) {
      process.stdout.write(JSON.stringify({ decision: "approve" }));
      return;
    }

    const relPath = path.relative(ROOT, filePath).replace(/\\/g, "/");
    if (SKIP_PATTERNS.some((p) => p.test(relPath))) {
      process.stdout.write(JSON.stringify({ decision: "approve" }));
      return;
    }

    const newContent = data.tool_input?.new_string || data.tool_input?.content || "";
    if (!newContent) {
      process.stdout.write(JSON.stringify({ decision: "approve" }));
      return;
    }

    const violations = checkCriticalRules(newContent, relPath);
    violations.push(...analyzeImports(relPath, newContent));

    if (violations.length > 0) {
      const msgs = violations.map((v) => `CRITICAL: ${v}`).join("\n");
      process.stdout.write(
        JSON.stringify({
          decision: "block",
          reason: `Guard Enforcement BLOCKED:\n${msgs}\n\nFix these violations before proceeding.`,
        })
      );
      return;
    }

    const boundaryWarning = checkBoundaries(filePath, ROOT);
    if (boundaryWarning) {
      process.stdout.write(
        JSON.stringify({
          decision: "approve",
          reason: `[BOUNDARY WARNING] ${boundaryWarning}`,
        })
      );
      return;
    }

    process.stdout.write(JSON.stringify({ decision: "approve" }));
  } catch {
    process.stdout.write(JSON.stringify({ decision: "approve" }));
  }
});

function checkCriticalRules(content, relPath) {
  const violations = [];
  for (const rule of CRITICAL_RULES) {
    if (!rule.scope.test(relPath)) continue;
    if (rule.exclude && rule.exclude.test(relPath)) continue;
    if (!rule.pattern.test(content)) continue;

    if (rule.crossModule) {
      const currentModule = relPath.match(/Modules[/\\]v\d+[/\\](\w+)/)?.[1];
      if (!currentModule) continue;
      const moduleBase = currentModule.toLowerCase().replace(/s$/, "");
      let hasCrossModule = false;
      // Only match PascalCase type references (I?[A-Z]...Repository), skip camelCase variable names
      const repoRegex = /\bI?([A-Z]\w+)Repository\b/g;
      let repoMatch;
      while ((repoMatch = repoRegex.exec(content)) !== null) {
        const repoName = repoMatch[1].toLowerCase();
        // Same-module if repo name contains the module base (e.g. NotificationDefinition contains "notification")
        if (repoName.includes(moduleBase) || moduleBase.includes(repoName)) continue;
        hasCrossModule = true;
        break;
      }
      if (!hasCrossModule) continue;
    }

    violations.push(`${rule.msg} (in ${relPath})`);
  }
  return violations;
}

/**
 * Scans active specs for a ## Boundaries section and checks whether the
 * edited file falls outside the declared scope. Advisory only — never blocks.
 *
 * @param {string} filePath  Absolute path of the file being edited
 * @param {string} cwd       Project root (CLAUDE_PROJECT_DIR or cwd)
 * @returns {string|null}    Warning message, or null if no boundary mismatch
 */
function checkBoundaries(filePath, cwd) {
  try {
    const specRoot = path.join(cwd, ".claude", "spec", "active");
    if (!fs.existsSync(specRoot)) return null;

    const normalizedEdit = filePath.replace(/\\/g, "/");

    const specDirs = fs.readdirSync(specRoot, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => d.name);

    for (const dir of specDirs) {
      const specFile = path.join(specRoot, dir, "spec.md");
      if (!fs.existsSync(specFile)) continue;

      const content = fs.readFileSync(specFile, "utf8");
      const boundaryMatch = content.match(/##\s+Boundaries\s*\n([\s\S]*?)(?:\n##\s|\n---\s*\n|$)/);
      if (!boundaryMatch) continue;

      const boundaryBlock = boundaryMatch[1];
      const lines = boundaryBlock.split("\n")
        .map((l) => l.replace(/^[-*]\s+`?/, "").replace(/`.*/, "").trim())
        .filter(Boolean);

      if (lines.length === 0) continue;

      // Check if the edited file matches any declared boundary
      for (const rawPattern of lines) {
        const pattern = rawPattern.replace(/\\/g, "/");
        if (!pattern) continue;

        // Directory scope: ends with /
        if (pattern.endsWith("/")) {
          const dir = pattern.endsWith("/") ? pattern : pattern + "/";
          if (normalizedEdit.includes(dir) || normalizedEdit.startsWith(dir)) return null;
          continue;
        }

        // Glob pattern: contains * or ?
        if (pattern.includes("*") || pattern.includes("?")) {
          const regexStr = pattern
            .replace(/[.+^${}()|[\]\\]/g, "\\$&")
            .replace(/\*\*/g, "(.+)")
            .replace(/\*/g, "([^/]+)")
            .replace(/\?/g, "([^/])");
          if (new RegExp(regexStr).test(normalizedEdit)) return null;
          continue;
        }

        // Exact file match (may be relative or absolute)
        const normalizedPattern = pattern.replace(/\\/g, "/");
        if (normalizedEdit.endsWith(normalizedPattern) || normalizedEdit === normalizedPattern) return null;
      }

      // File was not matched by any boundary in this spec
      const relEdited = path.relative(cwd, filePath).replace(/\\/g, "/");
      return `"${relEdited}" is outside the boundaries declared in spec "${dir}". Declared: ${lines.join(", ")}. Verify this edit is intentional.`;
    }

    return null;
  } catch {
    return null;
  }
}

function analyzeImports(relPath, content) {
  if (!relPath.endsWith(".cs")) return [];
  const currentModule = relPath.match(/Modules[/\\]v\d+[/\\](\w+)/)?.[1];
  if (!currentModule) return [];

  const violations = [];
  const isService = /Services?[/\\]/.test(relPath);
  const isRepository = /Repositor(y|ies)[/\\]/.test(relPath);
  const usingRegex = /using\s+[\w.]+\.Modules\.v\d+\.(\w+)\.([\w.]*)/g;
  let usingMatch;
  while ((usingMatch = usingRegex.exec(content)) !== null) {
    const [, importModule, importPath] = usingMatch;
    if (isService && importModule !== currentModule && /Repositor/i.test(importPath)) {
      violations.push(`L8: importing ${importModule}.${importPath} from ${currentModule} Service — use Service instead`);
    }
    if (!isRepository && /DbContext/i.test(importPath)) {
      violations.push(`L7: DbContext import in non-Repository file (${relPath})`);
    }
  }
  return violations;
}
