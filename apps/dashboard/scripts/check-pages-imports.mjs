#!/usr/bin/env node
// Validates that dashboard pages do not import from deleted/forbidden barrels.
// Used by spec 2026-05-23-dashboard-design-system as AC-6.
// Usage: node apps/dashboard/scripts/check-pages-imports.mjs [pagesDir]
//   default pagesDir = <dashboard>/src/pages (relative to this script)

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const DASHBOARD_ROOT = resolve(dirname(__filename), "..");

const FORBIDDEN = [
  { name: "@/components/ds", re: /from\s+['"]@\/components\/ds['"]/ },
  { name: "../components/ds", re: /from\s+['"]\.\.\/components\/ds['"]/ },
  { name: "./components/ds", re: /from\s+['"]\.\/components\/ds['"]/ },
  { name: "@/components/Markdown (raiz)", re: /from\s+['"]@\/components\/Markdown['"]/ },
  { name: "@/components/StatusDot (raiz)", re: /from\s+['"]@\/components\/StatusDot['"]/ },
];

const pagesDir = resolve(process.argv[2] ?? join(DASHBOARD_ROOT, "src", "pages"));

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) out.push(...walk(full));
    else if (entry.endsWith(".tsx") || entry.endsWith(".ts")) out.push(full);
  }
  return out;
}

const violations = [];
for (const file of walk(pagesDir)) {
  const src = readFileSync(file, "utf8");
  for (const { name, re } of FORBIDDEN) {
    if (re.test(src)) violations.push({ file, import: name });
  }
}

if (violations.length) {
  console.error(`[check-pages-imports] ${violations.length} violation(s):`);
  for (const v of violations) console.error(`  ${v.file} -> ${v.import}`);
  process.exit(1);
}

console.log(`[check-pages-imports] ok — ${pagesDir}`);
