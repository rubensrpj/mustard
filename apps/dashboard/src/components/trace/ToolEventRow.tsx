// Wave 6 — payload renderer for `kind === "tool"` nodes.
//
// The component picks the right DS primitive based on `payload.tool_name`:
//
//   Edit / Write → <DiffViewer> (before/after)
//   Read         → <CodeBlock> with language inferred from path extension
//   Bash         → <CodeBlock> with the command on top and stdout below
//   *            → JSON fallback via <CodeBlock lang="json">
//
// Rendering only happens when the parent <details> is open (lazy by
// construction — React still renders us, but heavy payloads stay collapsed
// until expanded because <ExecutionTrace> mounts this inside <details>).

import { memo } from "react";
import { DiffViewer, CodeBlock, type CodeLang } from "@/components/ds";
import type { TraceNode } from "@/lib/types/trace";

interface ToolEventRowProps {
  node: TraceNode;
}

export const ToolEventRow = memo(function ToolEventRow({ node }: ToolEventRowProps) {
  const payload = (node.payload ?? {}) as Record<string, unknown>;
  const toolName = strField(payload, "tool_name") ?? strField(payload, "name") ?? "";

  // Edit / Write — render as a unified diff.
  if (toolName === "Edit" || toolName === "Write" || toolName === "MultiEdit") {
    const before =
      strField(payload, "before") ??
      strField(payload, "old_string") ??
      strField(payload, "original") ??
      "";
    const after =
      strField(payload, "after") ??
      strField(payload, "new_string") ??
      strField(payload, "content") ??
      "";
    if (before || after) {
      return (
        <DiffViewer before={before} after={after} mode="unified" maxLines={200} />
      );
    }
  }

  // Read — render the excerpt with language inferred from the path.
  if (toolName === "Read") {
    const content =
      strField(payload, "content") ??
      strField(payload, "content_excerpt") ??
      strField(payload, "tool_response") ??
      "";
    const path = strField(payload, "file_path") ?? strField(payload, "path") ?? "";
    if (content) {
      return (
        <CodeBlock
          code={truncate(content, 200)}
          lang={detectLang(path)}
          showLineNumbers
        />
      );
    }
  }

  // Bash — render the command followed by the stdout/stderr.
  if (toolName === "Bash") {
    const command = strField(payload, "command") ?? "";
    const stdout = strField(payload, "stdout") ?? strField(payload, "tool_response") ?? "";
    const code = stdout ? `$ ${command}\n---\n${truncate(stdout, 100)}` : `$ ${command}`;
    return <CodeBlock code={code} lang="plain" />;
  }

  // Fallback — pretty-print the payload as JSON.
  return (
    <CodeBlock
      code={truncate(JSON.stringify(payload, null, 2), 200)}
      lang="json"
    />
  );
});

// ── Helpers ────────────────────────────────────────────────────────────────

function strField(obj: Record<string, unknown>, key: string): string | undefined {
  const v = obj[key];
  return typeof v === "string" ? v : undefined;
}

/**
 * Truncate to `maxLines` lines so a 5k-line `Read` payload doesn't
 * blow up the row height. The DS `CodeBlock` already supports
 * `showLineNumbers`; truncation is the only thing we need to enforce here.
 */
function truncate(s: string, maxLines: number): string {
  const lines = s.split("\n");
  if (lines.length <= maxLines) return s;
  return `${lines.slice(0, maxLines).join("\n")}\n… (+${lines.length - maxLines} linhas)`;
}

const EXT_TO_LANG: Record<string, CodeLang> = {
  rs: "rust",
  ts: "ts",
  tsx: "tsx",
  json: "json",
  sql: "sql",
};

function detectLang(path: string): CodeLang {
  const ext = path.toLowerCase().split(".").pop() ?? "";
  return EXT_TO_LANG[ext] ?? "plain";
}
