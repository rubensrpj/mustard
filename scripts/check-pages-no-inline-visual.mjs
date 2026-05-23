#!/usr/bin/env node
// scripts/check-pages-no-inline-visual.mjs
//
// AC-10 of spec 2026-05-23-dashboard-design-system.
// Walks every .tsx under apps/dashboard/src/pages and
// fails when a page declares visual styles inline. Pages
// must compose primitives from @/components/page; only
// structural layout classes (grid/flex/spacing/sizing)
// are allowed.
//
// Usage:
//   node scripts/check-pages-no-inline-visual.mjs [pagesDir]
//
// Exit codes:
//   0 - no violations
//   1 - violations found (printed to stderr)
//   2 - parse or I/O error

import { readdirSync, readFileSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { join, relative, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const REPO_ROOT = resolve(dirname(__filename), "..");

// @typescript-eslint/typescript-estree is a transitive
// devDep via eslint. We resolve from repo root and the
// dashboard so future hoisting changes don't break us.
const require = createRequire(import.meta.url);
const ESTREE_PATH = require.resolve("@typescript-eslint/typescript-estree", {
  paths: [REPO_ROOT, join(REPO_ROOT, "apps", "dashboard")],
});
const estreeMod = await import(toFileUrl(ESTREE_PATH));
const { parse: tsParse } = estreeMod;

function toFileUrl(p) {
  return "file://" + (p.startsWith("/") ? p : "/" + p.replace(/\\/g, "/"));
}

// Inline-style object keys that mean "the page is painting
// itself". Layout keys like width/height/padding/margin/flex
// are allowed because they're not visual decisions.
const FORBIDDEN_STYLE_KEYS = new Set([
  "color", "background", "backgroundColor",
  "border", "borderColor", "borderTop", "borderBottom",
  "borderLeft", "borderRight", "borderRadius",
  "boxShadow", "outline", "outlineColor",
]);

// Tokenized className whitelist. Every token must come from
// the design system OR be a structural layout primitive.
// Visual tokens not on this list trigger a fail.
const TOKEN_WHITELIST = new Set([
  // foreground/background semantic tokens
  "text-foreground", "text-muted-foreground", "text-card-foreground",
  "text-primary", "text-primary-foreground",
  "text-secondary", "text-secondary-foreground",
  "text-accent", "text-accent-foreground",
  "text-destructive", "text-destructive-foreground",
  "text-sidebar-foreground", "text-sidebar-primary",
  "bg-background", "bg-card", "bg-sidebar", "bg-primary",
  "bg-secondary", "bg-accent", "bg-muted", "bg-destructive",
  "bg-transparent", "bg-popover",
  "border-border", "border-sidebar-border", "border-input",
  "border-transparent",
  "ring-primary", "ring-ring", "ring-offset-background",
  "fill-current", "stroke-current",
]);

// Patterns that are always OK (structural layout, sizing,
// spacing, typography weight, opacity, transitions). Tested
// with String.prototype.match for full-string semantics.
const STRUCTURAL = [
  /^(grid|flex|inline-flex|inline-grid|block|inline-block|inline|hidden|contents)$/,
  /^(grid-cols|grid-rows|col-span|col-start|col-end|row-span|row-start|row-end|gap|gap-x|gap-y|place|justify|items|self|content|order)-/,
  /^(flex-1|flex-auto|flex-initial|flex-none|flex-row|flex-row-reverse|flex-col|flex-col-reverse|flex-wrap|flex-nowrap|flex-wrap-reverse|flex-grow|flex-shrink|basis)/,
  /^(w|h|min-w|min-h|max-w|max-h|size)-/,
  /^(p|px|py|pt|pr|pb|pl|m|mx|my|mt|mr|mb|ml|space-x|space-y)-/,
  /^(text)-(xs|sm|base|lg|xl|2xl|3xl|4xl|5xl|6xl|left|right|center|justify|start|end|balance|pretty|nowrap|wrap|ellipsis|clip)$/,
  /^text-\[\d+(?:\.\d+)?(?:px|rem|em)\]$/,
  /^(text)-(muted-foreground|card-foreground)(?:\/\d+)?$/,
  /^border(?:-(?:0|2|4|8|t|r|b|l|x|y|none|solid|dashed|dotted))?$/,
  /^outline-none$/,
  /^underline|^no-underline|^line-through|^uppercase|^lowercase|^capitalize|^normal-case$/,
  /^rounded-\[--[\w-]+(?:-(?:sm|md|lg|xl))?\]$/,
  /^(focus|focus-visible|hover|active|disabled):/,
  /^(font)-(thin|extralight|light|normal|medium|semibold|bold|extrabold|black|sans|serif|mono)$/,
  /^(leading|tracking|line-clamp|truncate|whitespace|break|hyphens|indent)/,
  /^(opacity|cursor|select|pointer-events|resize|scroll|snap|will-change|transform|origin|scale|rotate|translate|skew)/,
  /^(transition|duration|ease|delay|animate)/,
  /^(overflow|overscroll)/,
  /^(z|inset|top|right|bottom|left|static|fixed|absolute|relative|sticky)/,
  /^(rounded(?:-(?:none|sm|md|lg|xl|2xl|3xl|full|t|r|b|l|tl|tr|bl|br))?$)/,
  /^(shrink|grow)(?:-0)?$/,
  /^(tabular-nums|ordinal|slashed-zero|lining-nums|oldstyle-nums)$/,
  /^(aspect)-/,
  /^(divide)-/,
  /^(container|prose)$/,
  /^(items-start|items-center|items-end|items-baseline|items-stretch)$/,
  /^(justify-start|justify-center|justify-end|justify-between|justify-around|justify-evenly)$/,
];

function isStructural(token) {
  if (!token) return true;
  if (TOKEN_WHITELIST.has(token)) return true;
  // Drop arbitrary-value brackets for the structural test.
  // e.g. `w-[200px]`, `mt-[3px]` are layout, OK.
  for (const r of STRUCTURAL) if (r.test(token)) return true;
  // CSS variable tokens like `text-[--intent-error]` or
  // `bg-[--primary]/15` are design-system bridges — allowed.
  if (/^(text|bg|border|ring|fill|stroke|outline)-\[--[a-z][\w-]*\](?:\/\d+)?$/.test(token)) return true;
  // Pseudo / responsive / state prefixes — recurse on the
  // remainder after the colon. e.g. `hover:text-foreground`.
  const colon = token.indexOf(":");
  if (colon > 0) return isStructural(token.slice(colon + 1));
  return false;
}

// A token like `text-red-500` MUST fail. Detect "visual"
// tokens that need a design system check.
function isVisualButForbidden(token) {
  // Strip pseudo/state prefixes.
  const colon = token.lastIndexOf(":");
  const t = colon > 0 ? token.slice(colon + 1) : token;
  // Raw palette suffix like text-red-500, bg-zinc-900, border-emerald-300.
  if (/^(text|bg|border|ring|fill|stroke|outline|from|to|via)-(red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|slate|gray|zinc|neutral|stone|white|black)(?:-(?:50|100|200|300|400|500|600|700|800|900|950))?(?:\/\d+)?$/.test(t)) {
    return true;
  }
  return false;
}

// Hex color literal — never allowed in JSX.
const HEX_RE = /#[0-9a-fA-F]{3,8}\b/;

// Collect string-literal tokens from a className attribute
// value. Accepts: string literals, template literals with
// no interpolations, and `cn(...)` / `clsx(...)` call args
// that are themselves literals or arrays of literals.
function collectClassNameTokens(node) {
  const tokens = [];
  function visit(n) {
    if (!n) return;
    if (n.type === "Literal" && typeof n.value === "string") {
      for (const t of n.value.split(/\s+/)) if (t) tokens.push({ token: t, loc: n.loc });
    } else if (n.type === "TemplateLiteral") {
      for (const q of n.quasis) {
        for (const t of q.value.cooked.split(/\s+/)) if (t) tokens.push({ token: t, loc: n.loc });
      }
    } else if (n.type === "CallExpression") {
      // cn(...) / clsx(...) — descend into arguments.
      for (const a of n.arguments) visit(a);
    } else if (n.type === "ArrayExpression") {
      for (const el of n.elements) visit(el);
    } else if (n.type === "LogicalExpression") {
      visit(n.left); visit(n.right);
    } else if (n.type === "ConditionalExpression") {
      visit(n.consequent); visit(n.alternate);
    }
  }
  visit(node);
  return tokens;
}

function walk(node, visitor) {
  if (!node || typeof node !== "object") return;
  visitor(node);
  for (const k of Object.keys(node)) {
    if (k === "loc" || k === "range" || k === "parent") continue;
    const v = node[k];
    if (Array.isArray(v)) for (const c of v) walk(c, visitor);
    else if (v && typeof v === "object" && typeof v.type === "string") walk(v, visitor);
  }
}

function check(file) {
  const src = readFileSync(file, "utf8");
  let ast;
  try {
    ast = tsParse(src, {
      jsx: true,
      loc: true,
      range: false,
      errorOnUnknownASTType: false,
      // .tsx parsing — typescript-estree handles ts + jsx without
      // a separate plugin.
      filePath: file,
    });
  } catch (err) {
    console.error(`[parse] ${file}: ${err.message}`);
    return [{ file, kind: "parse-error", line: 0, column: 0, snippet: err.message }];
  }
  const violations = [];

  walk(ast, (n) => {
    if (n.type !== "JSXAttribute") return;
    const attrName = n.name?.name;
    if (attrName === "style" && n.value?.type === "JSXExpressionContainer") {
      const obj = n.value.expression;
      if (obj?.type === "ObjectExpression") {
        for (const prop of obj.properties) {
          if (prop.type !== "Property") continue;
          const key = prop.key?.name ?? prop.key?.value;
          if (FORBIDDEN_STYLE_KEYS.has(key)) {
            violations.push({
              file,
              kind: "inline-style",
              line: prop.loc?.start.line ?? 0,
              column: prop.loc?.start.column ?? 0,
              snippet: `style={{ ${key}: ... }}`,
            });
          }
        }
      }
    }
    if (attrName === "className") {
      const tokens = collectClassNameTokens(
        n.value?.type === "JSXExpressionContainer" ? n.value.expression : n.value,
      );
      for (const { token, loc } of tokens) {
        if (HEX_RE.test(token)) {
          violations.push({
            file, kind: "hex-color",
            line: loc?.start.line ?? 0,
            column: loc?.start.column ?? 0,
            snippet: token,
          });
          continue;
        }
        if (isVisualButForbidden(token)) {
          violations.push({
            file, kind: "raw-palette",
            line: loc?.start.line ?? 0,
            column: loc?.start.column ?? 0,
            snippet: token,
          });
          continue;
        }
        if (!isStructural(token)) {
          violations.push({
            file, kind: "non-whitelisted",
            line: loc?.start.line ?? 0,
            column: loc?.start.column ?? 0,
            snippet: token,
          });
        }
      }
    }
  });

  // Also flag bare hex literals anywhere (e.g., inline
  // string used in a conditional class).
  walk(ast, (n) => {
    if (n.type !== "Literal" || typeof n.value !== "string") return;
    if (!HEX_RE.test(n.value)) return;
    // Skip if already reported via className.
    violations.push({
      file, kind: "hex-literal",
      line: n.loc?.start.line ?? 0,
      column: n.loc?.start.column ?? 0,
      snippet: n.value.slice(0, 80),
    });
  });

  return violations;
}

function listPages(dir) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, e.name);
    if (e.isDirectory()) out.push(...listPages(full));
    else if (e.isFile() && full.endsWith(".tsx")) out.push(full);
  }
  return out;
}

const pagesDir = resolve(process.argv[2] ?? join(REPO_ROOT, "apps", "dashboard", "src", "pages"));
try { statSync(pagesDir); }
catch { console.error(`[check] not a directory: ${pagesDir}`); process.exit(2); }

const allViolations = [];
for (const file of listPages(pagesDir)) {
  for (const v of check(file)) allViolations.push(v);
}

if (allViolations.length) {
  console.error(`[check-pages-no-inline-visual] ${allViolations.length} violation(s) under ${pagesDir}:`);
  // Group per-file for readability.
  const byFile = new Map();
  for (const v of allViolations) {
    const arr = byFile.get(v.file) ?? [];
    arr.push(v);
    byFile.set(v.file, arr);
  }
  for (const [file, arr] of byFile) {
    console.error(`  ${relative(REPO_ROOT, file)} (${arr.length})`);
    for (const v of arr.slice(0, 8)) {
      console.error(`    ${v.line}:${v.column}  ${v.kind}  ${v.snippet}`);
    }
    if (arr.length > 8) console.error(`    ... +${arr.length - 8} more`);
  }
  process.exit(1);
}

console.log(`[check-pages-no-inline-visual] ok — ${pagesDir}`);
