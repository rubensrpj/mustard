// Real syntax highlighter (replaces the old ~5-language keyword tokenizer).
//
// We use `PrismAsyncLight` from react-syntax-highlighter: it ships only the
// core Prism runtime and loads each grammar lazily via `registerLanguage`, so
// the bundle grows per-language-actually-used rather than shipping the whole
// Prism corpus. The theme is `vscDarkPlus` (VS Code "Dark+") — a fixed dark
// palette so code blocks read like a Notion/VS Code code block regardless of
// the app's light/dark mode (the surrounding chrome still follows our tokens).
//
// API is backward-compatible: the old `CodeLang` ids (`rust|ts|tsx|json|sql|
// plain`) still work, and `lang` now ALSO accepts any extension/alias/Prism id
// (`cs`, `py`, `go`, `yaml`, …). Unknown or `plain` → rendered without
// highlight (plain text), never throwing.

import { useMemo } from "react";
import { PrismAsyncLight as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import { cn } from "@/lib/utils";

// ── Grammar registry ────────────────────────────────────────────────────────
// Register a broad set covering "any code". Each import is a Prism language
// refractor module; PrismAsyncLight code-splits them so they load on demand.
import bash from "react-syntax-highlighter/dist/esm/languages/prism/bash";
import c from "react-syntax-highlighter/dist/esm/languages/prism/c";
import cpp from "react-syntax-highlighter/dist/esm/languages/prism/cpp";
import csharp from "react-syntax-highlighter/dist/esm/languages/prism/csharp";
import css from "react-syntax-highlighter/dist/esm/languages/prism/css";
import dart from "react-syntax-highlighter/dist/esm/languages/prism/dart";
import diff from "react-syntax-highlighter/dist/esm/languages/prism/diff";
import docker from "react-syntax-highlighter/dist/esm/languages/prism/docker";
import go from "react-syntax-highlighter/dist/esm/languages/prism/go";
import graphql from "react-syntax-highlighter/dist/esm/languages/prism/graphql";
import ini from "react-syntax-highlighter/dist/esm/languages/prism/ini";
import java from "react-syntax-highlighter/dist/esm/languages/prism/java";
import javascript from "react-syntax-highlighter/dist/esm/languages/prism/javascript";
import json from "react-syntax-highlighter/dist/esm/languages/prism/json";
import jsx from "react-syntax-highlighter/dist/esm/languages/prism/jsx";
import kotlin from "react-syntax-highlighter/dist/esm/languages/prism/kotlin";
import less from "react-syntax-highlighter/dist/esm/languages/prism/less";
import markdown from "react-syntax-highlighter/dist/esm/languages/prism/markdown";
import markup from "react-syntax-highlighter/dist/esm/languages/prism/markup";
import php from "react-syntax-highlighter/dist/esm/languages/prism/php";
import python from "react-syntax-highlighter/dist/esm/languages/prism/python";
import ruby from "react-syntax-highlighter/dist/esm/languages/prism/ruby";
import rust from "react-syntax-highlighter/dist/esm/languages/prism/rust";
import scss from "react-syntax-highlighter/dist/esm/languages/prism/scss";
import sql from "react-syntax-highlighter/dist/esm/languages/prism/sql";
import swift from "react-syntax-highlighter/dist/esm/languages/prism/swift";
import toml from "react-syntax-highlighter/dist/esm/languages/prism/toml";
import typescript from "react-syntax-highlighter/dist/esm/languages/prism/typescript";
import tsx from "react-syntax-highlighter/dist/esm/languages/prism/tsx";
import yaml from "react-syntax-highlighter/dist/esm/languages/prism/yaml";

// Register each grammar under its canonical Prism id once at module load.
const GRAMMARS: Record<string, Parameters<typeof SyntaxHighlighter.registerLanguage>[1]> = {
  bash,
  c,
  cpp,
  csharp,
  css,
  dart,
  diff,
  docker,
  go,
  graphql,
  ini,
  java,
  javascript,
  json,
  jsx,
  kotlin,
  less,
  markdown,
  markup,
  php,
  python,
  ruby,
  rust,
  scss,
  sql,
  swift,
  toml,
  typescript,
  tsx,
  yaml,
};
for (const [id, grammar] of Object.entries(GRAMMARS)) {
  SyntaxHighlighter.registerLanguage(id, grammar);
}

/**
 * Legacy id union — kept exported so existing callers (`ToolEventRow`,
 * `SpecTimelineTab`, Economia) that import `CodeLang` keep compiling. New code
 * may pass any string (extension / alias / Prism id); see `LANG_ALIASES`.
 */
export type CodeLang = "rust" | "ts" | "tsx" | "json" | "sql" | "plain";

/**
 * Map extensions, common aliases and the legacy `CodeLang` ids onto registered
 * Prism language ids. Anything not found here (and not a registered id) falls
 * back to plain text — never throws.
 */
const LANG_ALIASES: Record<string, string> = {
  // legacy CodeLang ids
  rs: "rust",
  ts: "typescript",
  typescript: "typescript",
  tsx: "tsx",
  js: "javascript",
  jsx: "jsx",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  jsonc: "json",
  sql: "sql",
  // dotnet / jvm
  cs: "csharp",
  csharp: "csharp",
  kt: "kotlin",
  kts: "kotlin",
  java: "java",
  // scripting
  py: "python",
  python: "python",
  rb: "ruby",
  ruby: "ruby",
  php: "php",
  sh: "bash",
  zsh: "bash",
  bash: "bash",
  shell: "bash",
  // systems
  go: "go",
  golang: "go",
  rust: "rust",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  swift: "swift",
  dart: "dart",
  // data / config
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  ini: "ini",
  cfg: "ini",
  conf: "ini",
  // web / markup
  css: "css",
  scss: "scss",
  less: "less",
  html: "markup",
  htm: "markup",
  xml: "markup",
  svg: "markup",
  markup: "markup",
  graphql: "graphql",
  gql: "graphql",
  // docs / misc
  md: "markdown",
  markdown: "markdown",
  mdx: "markdown",
  diff: "diff",
  patch: "diff",
  dockerfile: "docker",
  docker: "docker",
};

/** Registered Prism ids — used to accept a raw id even if it's not aliased. */
const REGISTERED = new Set(Object.keys(GRAMMARS));

/**
 * Resolve a free-form `lang` (extension / alias / Prism id / legacy CodeLang)
 * to a registered Prism id, or `null` for plain text (no highlight).
 */
function resolveLanguage(lang: string | undefined): string | null {
  if (!lang) return null;
  const key = lang.toLowerCase();
  if (key === "plain" || key === "text" || key === "txt") return null;
  if (LANG_ALIASES[key]) return LANG_ALIASES[key];
  if (REGISTERED.has(key)) return key;
  return null;
}

export interface CodeBlockProps {
  code: string;
  /**
   * Language hint: a legacy `CodeLang` id, a file extension (`cs`, `py`, …),
   * an alias, or a registered Prism id. Unknown / `plain` → no highlight.
   */
  lang?: CodeLang | string;
  showLineNumbers?: boolean;
  className?: string;
}

export function CodeBlock({
  code,
  lang = "plain",
  showLineNumbers = false,
  className,
}: CodeBlockProps) {
  const language = useMemo(() => resolveLanguage(lang), [lang]);

  // Shared chrome: rounded dark surface (the prism theme owns the *inner*
  // background; we add the frame, mono font, comfortable padding and a
  // horizontal scroll so long code lines never wrap/break).
  const frame = cn(
    "rounded-[--radius-card] border border-[--border] overflow-hidden",
    "text-[12px] leading-[1.55]",
    className,
  );

  // `customStyle` resets the library's default margin and lets our frame own
  // the radius/border; we keep the theme's dark background so the block reads
  // like a Notion/VS Code code block. `tabular-nums` keeps the gutter aligned.
  const customStyle: React.CSSProperties = {
    margin: 0,
    padding: "8px 12px",
    background: "transparent",
    fontSize: "12px",
    lineHeight: "1.55",
  };

  const lineNumberStyle: React.CSSProperties = {
    minWidth: "2.25em",
    paddingRight: "1em",
    textAlign: "right",
    opacity: 0.45,
    fontVariantNumeric: "tabular-nums",
    userSelect: "none",
  };

  // Plain text path — no grammar, render in the same frame without highlight.
  if (!language) {
    return (
      <div className={frame} style={{ background: "#1e1e1e" }}>
        <SyntaxHighlighter
          language="text"
          style={vscDarkPlus}
          showLineNumbers={showLineNumbers}
          customStyle={customStyle}
          lineNumberStyle={lineNumberStyle}
          codeTagProps={{ style: { fontFamily: "var(--font-mono, monospace)", whiteSpace: "pre" } }}
          wrapLongLines={false}
        >
          {code}
        </SyntaxHighlighter>
      </div>
    );
  }

  return (
    <div className={frame} style={{ background: "#1e1e1e" }}>
      <SyntaxHighlighter
        language={language}
        style={vscDarkPlus}
        showLineNumbers={showLineNumbers}
        customStyle={customStyle}
        lineNumberStyle={lineNumberStyle}
        codeTagProps={{ style: { fontFamily: "var(--font-mono, monospace)", whiteSpace: "pre" } }}
        wrapLongLines={false}
      >
        {code}
      </SyntaxHighlighter>
    </div>
  );
}
