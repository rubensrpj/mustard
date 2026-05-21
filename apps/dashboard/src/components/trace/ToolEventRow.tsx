// Wave 6 + Followup-fix (2026-05-21, spec `2026-05-21-economia-moat-followup-fixes`).
//
// Payload renderer for `kind === "tool"` trace nodes. Each tool variant gets
// a card with a dedicated header (tool name + file path / command) and the
// matching DS primitive for the body:
//
//   Edit / Write / MultiEdit → <DiffViewer mode="split"> + file path subheader
//   Read                     → <CodeBlock> with lang inferred from path
//   Bash                     → <CodeBlock> for the command + stdout (+ stderr)
//   *                        → JSON fallback via <CodeBlock lang="json">
//
// We never `throw` — when expected payload fields are missing (backend has
// not populated `before`/`after`/`command`/etc. yet) we fall back to the
// JSON view so the user still sees *something*.

import { memo } from "react";
import { DiffViewer, CodeBlock, type CodeLang } from "@/components/ds";
import { cn } from "@/lib/utils";
import type { TraceNode } from "@/lib/types/trace";

interface ToolEventRowProps {
  node: TraceNode;
}

export const ToolEventRow = memo(function ToolEventRow({ node }: ToolEventRowProps) {
  const payload = (node.payload ?? {}) as Record<string, unknown>;
  const toolName =
    strField(payload, "tool_name") ?? strField(payload, "name") ?? "";

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
    const path = pathOf(payload);
    if (before || after) {
      return (
        <PayloadCard toolName={toolName} subheader={path}>
          <DiffViewer
            before={before}
            after={after}
            mode="split"
            maxLines={200}
          />
        </PayloadCard>
      );
    }
    return <FallbackJson toolName={toolName} subheader={path} payload={payload} />;
  }

  if (toolName === "Read") {
    const content =
      strField(payload, "content") ??
      strField(payload, "content_excerpt") ??
      strField(payload, "tool_response") ??
      "";
    const path = pathOf(payload);
    if (content) {
      return (
        <PayloadCard toolName="Read" subheader={path}>
          <CodeBlock
            code={truncate(content, 200)}
            lang={detectLang(path)}
            showLineNumbers
          />
        </PayloadCard>
      );
    }
    return <FallbackJson toolName="Read" subheader={path} payload={payload} />;
  }

  if (toolName === "Bash") {
    const command = strField(payload, "command") ?? "";
    const stdout =
      strField(payload, "stdout") ?? strField(payload, "tool_response") ?? "";
    const stderr = strField(payload, "stderr") ?? "";
    return (
      <PayloadCard
        toolName="Bash"
        subheader={command ? `$ ${command}` : undefined}
        subheaderMono
      >
        {stdout ? (
          <CodeBlock code={truncate(stdout, 200)} lang="plain" />
        ) : null}
        {stderr ? (
          <div
            className={cn(
              "mt-2 rounded-[--ds-radius-sm] overflow-hidden",
              "ring-1 ring-[--ds-intent-error]/30",
              "bg-[--ds-intent-error]/10",
            )}
          >
            <CodeBlock code={truncate(stderr, 200)} lang="plain" />
          </div>
        ) : null}
        {!stdout && !stderr && !command ? (
          <CodeBlock
            code={truncate(JSON.stringify(payload, null, 2), 200)}
            lang="json"
          />
        ) : null}
      </PayloadCard>
    );
  }

  // Fallback — pretty-print whatever payload arrived.
  return (
    <FallbackJson
      toolName={toolName || "tool.use"}
      subheader={pathOf(payload)}
      payload={payload}
    />
  );
});

// ── Card wrapper ───────────────────────────────────────────────────────────

interface PayloadCardProps {
  toolName: string;
  subheader?: string;
  subheaderMono?: boolean;
  children: React.ReactNode;
}

/** Shared card frame so every tool variant has the same visual rhythm:
 *  small uppercase tool-name pill on top, optional file-path / command
 *  subheader underneath, then the actual payload renderer. */
function PayloadCard({
  toolName,
  subheader,
  subheaderMono,
  children,
}: PayloadCardProps) {
  return (
    <div
      className={cn(
        "rounded-[--ds-radius-md] border border-[--ds-surface-hover]",
        "bg-[--ds-surface-base] overflow-hidden",
      )}
    >
      <div className="flex items-center gap-2 px-3 py-1.5 bg-[--ds-surface-sunken] border-b border-[--ds-surface-hover]">
        <span
          className={cn(
            "px-1.5 py-0.5 rounded-[--ds-radius-sm]",
            "text-[10px] font-medium tracking-wide uppercase",
            "bg-[--ds-accent-primary]/15 text-[--ds-accent-primary]",
          )}
        >
          {toolName}
        </span>
        {subheader ? (
          <span
            className={cn(
              "text-[11px] text-[--ds-text-tertiary] truncate flex-1 min-w-0",
              subheaderMono && "font-mono",
            )}
            title={subheader}
          >
            {subheader}
          </span>
        ) : null}
      </div>
      <div className="p-2">{children}</div>
    </div>
  );
}

interface FallbackJsonProps {
  toolName: string;
  subheader?: string;
  payload: Record<string, unknown>;
}

function FallbackJson({ toolName, subheader, payload }: FallbackJsonProps) {
  return (
    <PayloadCard toolName={toolName} subheader={subheader}>
      <CodeBlock
        code={truncate(JSON.stringify(payload, null, 2), 200)}
        lang="json"
      />
    </PayloadCard>
  );
}

// ── Helpers ────────────────────────────────────────────────────────────────

function strField(obj: Record<string, unknown>, key: string): string | undefined {
  const v = obj[key];
  return typeof v === "string" ? v : undefined;
}

function pathOf(payload: Record<string, unknown>): string | undefined {
  return (
    strField(payload, "file_path") ??
    strField(payload, "path") ??
    strField(payload, "filepath") ??
    undefined
  );
}

/** Truncate to `maxLines` lines so a 5k-line payload doesn't blow up the row. */
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

function detectLang(path: string | undefined): CodeLang {
  if (!path) return "plain";
  const ext = path.toLowerCase().split(".").pop() ?? "";
  return EXT_TO_LANG[ext] ?? "plain";
}
