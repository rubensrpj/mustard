#!/usr/bin/env bun

/**
 * wave-dependency.js
 *
 * Builds a dependency DAG from a list of files (via imports/require/from parsing)
 * and groups files into waves using topological level assignment.
 *
 * Input (stdin):
 *   {
 *     files: ["src/schema/user.ts", "src/api/users.ts", ...],
 *     projectRoot: "."               // absolute or relative to cwd (optional, defaults to cwd)
 *   }
 *
 * Output (stdout):
 *   Success:
 *   {
 *     waves: [
 *       { wave: 1, files: [...], roles: [...], dependsOn: [] },
 *       { wave: 2, files: [...], roles: [...], dependsOn: [1] },
 *       ...
 *     ],
 *     metadata: { totalWaves: N, totalFiles: M, widestWave: W }
 *   }
 *
 *   Error:
 *   { error: "cyclic-dependency" | "empty-input" | "error-fallback", cycle?: [...] }
 *
 * Fail-open: on unrecoverable error, emits { error: "error-fallback" } and exits 0.
 */

"use strict";

const fs = require("fs");
const path = require("path");
const { detectRole } = require("./_lib/wave-lib");

const RESOLVABLE_EXTENSIONS = [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".vue", ".svelte", ".py", ".go", ".cs"];
const INDEX_BASENAMES = ["index.ts", "index.tsx", "index.js", "index.jsx", "index.mjs", "__init__.py"];

function readStdin() {
  return new Promise((resolve) => {
    let data = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => {
      data += chunk;
    });
    process.stdin.on("end", () => resolve(data));
    process.stdin.on("error", () => resolve(""));
  });
}

function extractImports(content) {
  const imports = new Set();
  const patterns = [
    /import\s+[\s\S]*?from\s+['"]([^'"]+)['"]/g,
    /import\s+['"]([^'"]+)['"]/g,
    /require\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
    /from\s+([.\w]+)\s+import\s+/g,
  ];
  for (const re of patterns) {
    let m;
    while ((m = re.exec(content)) !== null) {
      imports.add(m[1]);
    }
  }
  return [...imports];
}

function resolveImport(importPath, currentFileAbs, projectRoot, candidateSet) {
  if (!importPath.startsWith(".") && !importPath.startsWith("/")) {
    return null;
  }
  const baseDir = path.dirname(currentFileAbs);
  const absTarget = path.resolve(baseDir, importPath);

  if (candidateSet.has(absTarget)) return absTarget;

  for (const ext of RESOLVABLE_EXTENSIONS) {
    const withExt = absTarget + ext;
    if (candidateSet.has(withExt)) return withExt;
  }

  const lastDot = absTarget.lastIndexOf(".");
  if (lastDot > absTarget.lastIndexOf(path.sep)) {
    const stripped = absTarget.slice(0, lastDot);
    if (candidateSet.has(stripped)) return stripped;
    for (const ext of RESOLVABLE_EXTENSIONS) {
      const swapped = stripped + ext;
      if (candidateSet.has(swapped)) return swapped;
    }
  }

  for (const basename of INDEX_BASENAMES) {
    const indexPath = path.join(absTarget, basename);
    if (candidateSet.has(indexPath)) return indexPath;
  }

  return null;
}

function buildGraph(files, projectRoot) {
  const absFiles = files.map((f) => path.isAbsolute(f) ? f : path.resolve(projectRoot, f));
  const candidateSet = new Set(absFiles);
  const graph = new Map();

  for (const absFile of absFiles) {
    graph.set(absFile, new Set());
    let content;
    try {
      content = fs.readFileSync(absFile, "utf8");
    } catch {
      continue;
    }
    const imports = extractImports(content);
    for (const imp of imports) {
      const resolved = resolveImport(imp, absFile, projectRoot, candidateSet);
      if (resolved && resolved !== absFile) {
        graph.get(absFile).add(resolved);
      }
    }
  }

  return { graph, absFiles };
}

function topologicalWaves(graph) {
  const indegree = new Map();
  const dependents = new Map();

  for (const node of graph.keys()) {
    indegree.set(node, 0);
    dependents.set(node, new Set());
  }

  for (const [node, deps] of graph.entries()) {
    for (const dep of deps) {
      if (graph.has(dep)) {
        indegree.set(node, (indegree.get(node) || 0) + 1);
        dependents.get(dep).add(node);
      }
    }
  }

  const waves = [];
  const visited = new Set();
  let currentWave = [...indegree.entries()].filter(([, deg]) => deg === 0).map(([node]) => node);

  while (currentWave.length > 0) {
    waves.push(currentWave);
    for (const node of currentWave) visited.add(node);
    const nextWave = [];
    for (const node of currentWave) {
      for (const dependent of dependents.get(node) || []) {
        const newDeg = indegree.get(dependent) - 1;
        indegree.set(dependent, newDeg);
        if (newDeg === 0 && !visited.has(dependent)) {
          nextWave.push(dependent);
        }
      }
    }
    currentWave = nextWave;
  }

  if (visited.size < graph.size) {
    const stuck = [...graph.keys()].filter((n) => !visited.has(n));
    return { error: "cyclic-dependency", cycle: stuck };
  }

  return { waves };
}

function toRelative(abs, projectRoot) {
  try {
    return path.relative(projectRoot, abs).replace(/\\/g, "/");
  } catch {
    return abs;
  }
}

async function main() {
  try {
    const raw = await readStdin();
    if (!raw.trim()) {
      process.stdout.write(JSON.stringify({ error: "empty-input" }));
      return;
    }
    const { files, projectRoot = process.cwd() } = JSON.parse(raw);

    if (!Array.isArray(files) || files.length === 0) {
      process.stdout.write(JSON.stringify({ error: "empty-input" }));
      return;
    }

    const rootAbs = path.isAbsolute(projectRoot) ? projectRoot : path.resolve(projectRoot);
    const { graph } = buildGraph(files, rootAbs);
    const result = topologicalWaves(graph);

    if (result.error) {
      const cycle = (result.cycle || []).map((f) => toRelative(f, rootAbs));
      process.stdout.write(JSON.stringify({ error: result.error, cycle }));
      return;
    }

    const waves = result.waves.map((waveFiles, idx) => {
      const relFiles = waveFiles.map((f) => toRelative(f, rootAbs));
      const roles = [...new Set(relFiles.map(detectRole))];
      return {
        wave: idx + 1,
        files: relFiles,
        roles,
        dependsOn: idx === 0 ? [] : [idx],
      };
    });

    const widestWave = waves.reduce((max, w) => Math.max(max, w.files.length), 0);

    process.stdout.write(JSON.stringify({
      waves,
      metadata: {
        totalWaves: waves.length,
        totalFiles: files.length,
        widestWave,
      },
    }));
  } catch (_err) {
    process.stdout.write(JSON.stringify({ error: "error-fallback" }));
  }
}

main();
