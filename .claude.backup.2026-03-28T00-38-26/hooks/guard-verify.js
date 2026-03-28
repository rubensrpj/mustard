#!/usr/bin/env node
/**
 * GUARD-VERIFY: PostToolUse hook for Write|Edit
 *
 * Verifies production file edits against critical architectural rules.
 * Critical violations → block. All other checks are handled by review agents.
 *
 * @version 4.0.0
 */

const path = require("path");

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
    } else {
      process.stdout.write(JSON.stringify({ decision: "approve" }));
    }
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
      const repoMatches = [...content.matchAll(/\b(\w+)Repository\b/g)];
      const moduleBase = currentModule.toLowerCase().replace(/s$/, "");
      let hasCrossModule = false;
      for (const [, repoName] of repoMatches) {
        const repoBase = repoName.toLowerCase().replace(/^i/, "");
        if (repoBase.startsWith(moduleBase) || moduleBase.startsWith(repoBase.replace(/repository$/, ""))) continue;
        hasCrossModule = true;
        break;
      }
      if (!hasCrossModule) continue;
    }

    violations.push(`${rule.msg} (in ${relPath})`);
  }
  return violations;
}

function analyzeImports(relPath, content) {
  if (!relPath.endsWith(".cs")) return [];
  const currentModule = relPath.match(/Modules[/\\]v\d+[/\\](\w+)/)?.[1];
  if (!currentModule) return [];

  const violations = [];
  const isService = /Services?[/\\]/.test(relPath);
  const isRepository = /Repositor(y|ies)[/\\]/.test(relPath);
  const usings = [...content.matchAll(/using\s+[\w.]+\.Modules\.v\d+\.(\w+)\.([\w.]*)/g)];

  for (const [, importModule, importPath] of usings) {
    if (isService && importModule !== currentModule && /Repositor/i.test(importPath)) {
      violations.push(`L8: importing ${importModule}.${importPath} from ${currentModule} Service — use Service instead`);
    }
    if (!isRepository && /DbContext/i.test(importPath)) {
      violations.push(`L7: DbContext import in non-Repository file (${relPath})`);
    }
  }
  return violations;
}
