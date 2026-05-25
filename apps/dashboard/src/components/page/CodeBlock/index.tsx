// Keyword-based syntax highlighter — no highlight.js / prism / shiki.
// We tokenise strings, line/block comments, numbers, and a small keyword
// set per language. Anything else falls back to the plain text color
// (`--foreground`). Good enough for the trace-viewer and Economia
// snippets; not a full parser.

import { useMemo } from "react";
import { cn } from "@/lib/utils";

export type CodeLang = "rust" | "ts" | "tsx" | "json" | "sql" | "plain";

export interface CodeBlockProps {
  code: string;
  lang?: CodeLang;
  showLineNumbers?: boolean;
  className?: string;
}

const KEYWORDS: Record<CodeLang, ReadonlyArray<string>> = {
  rust: [
    "fn", "let", "mut", "const", "static", "struct", "enum", "impl", "trait",
    "pub", "use", "mod", "match", "if", "else", "return", "for", "while",
    "loop", "in", "as", "self", "Self", "ref", "move", "async", "await",
  ],
  ts: [
    "const", "let", "var", "function", "return", "if", "else", "for", "while",
    "switch", "case", "break", "continue", "class", "interface", "type",
    "extends", "implements", "import", "from", "export", "default", "new",
    "typeof", "instanceof", "as", "async", "await", "true", "false", "null", "undefined",
  ],
  tsx: [
    "const", "let", "var", "function", "return", "if", "else", "for", "while",
    "switch", "case", "break", "continue", "class", "interface", "type",
    "extends", "implements", "import", "from", "export", "default", "new",
    "typeof", "instanceof", "as", "async", "await", "true", "false", "null", "undefined",
  ],
  json: ["true", "false", "null"],
  sql: [
    "SELECT", "FROM", "WHERE", "JOIN", "INNER", "LEFT", "RIGHT", "ON",
    "GROUP", "BY", "ORDER", "LIMIT", "INSERT", "INTO", "VALUES", "UPDATE",
    "SET", "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "AS", "AND", "OR", "NOT", "NULL",
  ],
  plain: [],
};

type Tok = { cls: string; text: string };

/** Tokenise one line. Strings/comments first, then keywords, then numbers. */
function tokenizeLine(line: string, lang: CodeLang): Tok[] {
  // TF remap: --ds-text-primary → --foreground
  if (lang === "plain") return [{ cls: "text-[--foreground]", text: line }];
  const kw = new Set(KEYWORDS[lang]);
  const out: Tok[] = [];
  // Pattern order matters — strings first (incl. /* */), then //-line-comments,
  // then identifiers and numbers. The regex is global; remaining text becomes plain.
  const re = /(\/\/[^\n]*)|(\/\*[\s\S]*?\*\/)|("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)|(\b\d+(?:\.\d+)?\b)|([A-Za-z_][A-Za-z0-9_]*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(line))) {
    if (m.index > last) {
      // TF remap: --ds-text-primary → --foreground
      out.push({ cls: "text-[--foreground]", text: line.slice(last, m.index) });
    }
    if (m[1] || m[2]) {
      // TF remap: --ds-text-tertiary → --muted-foreground; no tertiary tier in Binance
      out.push({ cls: "text-[--muted-foreground] italic", text: m[0] });
    } else if (m[3]) {
      // TF remap: --ds-intent-success → --intent-success
      out.push({ cls: "text-[--intent-success]", text: m[0] });
    } else if (m[4]) {
      // TF remap: --ds-intent-warning → --intent-warning
      out.push({ cls: "text-[--intent-warning]", text: m[0] });
    } else if (m[5]) {
      const word = m[5];
      const isKw = lang === "sql" ? kw.has(word.toUpperCase()) : kw.has(word);
      out.push({
        // TF remap: --ds-accent-primary → --primary (Mustard yellow brand); --ds-text-primary → --foreground
        cls: isKw ? "text-[--primary] font-medium" : "text-[--foreground]",
        text: word,
      });
    }
    last = m.index + m[0].length;
  }
  if (last < line.length) {
    // TF remap: --ds-text-primary → --foreground
    out.push({ cls: "text-[--foreground]", text: line.slice(last) });
  }
  if (out.length === 0) out.push({ cls: "text-[--foreground]", text: "" });
  return out;
}

export function CodeBlock({
  code,
  lang = "plain",
  showLineNumbers = false,
  className,
}: CodeBlockProps) {
  const lines = useMemo(() => code.split("\n"), [code]);
  const tokenized = useMemo(() => lines.map((l) => tokenizeLine(l, lang)), [lines, lang]);
  const gutterW = String(lines.length).length;

  return (
    <pre
      className={cn(
        // TF remap: --ds-radius-md → var(--radius-card); --ds-surface-hover → --accent; --ds-surface-sunken → --background (flat canvas)
        "rounded-[--radius-card] border border-[--accent] bg-[--background] overflow-auto",
        "font-mono text-[12px] leading-[1.55] py-2",
        className,
      )}
    >
      {tokenized.map((toks, idx) => (
        <div
          key={idx}
          className={cn(
            "px-3",
            showLineNumbers ? "grid grid-cols-[auto_1fr] gap-3" : "block",
          )}
        >
          {showLineNumbers ? (
            <span
              // TF remap: --ds-text-tertiary → --muted-foreground
              className="text-right text-[--muted-foreground] select-none"
              style={{ minWidth: `${gutterW}ch` }}
            >
              {idx + 1}
            </span>
          ) : null}
          <code className="whitespace-pre">
            {toks.map((t, i) => (
              <span key={i} className={t.cls}>{t.text}</span>
            ))}
          </code>
        </div>
      ))}
    </pre>
  );
}
