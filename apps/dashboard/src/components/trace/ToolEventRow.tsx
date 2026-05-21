// Wave 6 + Followup-fix-2 (2026-05-21, spec
// `2026-05-21-economia-followup-2-trace-rich`).
//
// Payload renderer for `kind === "tool"` trace nodes. The real `tool.use`
// shape (emitted by the rt hook) is
//   { tool, target: { command?, file_path?, description? }, phase?,
//     tool_use_id?, result?: ToolResultPayload }
// where `result` is spliced in by the Rust pairing step
// (`telemetry.rs::pair_tool_results`) for tools whose `tool.result` was
// captured by the post-tool hook (stdout/stderr for Bash, before/after
// snapshots for Edit/Write/MultiEdit, content excerpt for Read).
//
// Each variant gets a card with a dedicated header (tool name + file path
// or command) and the matching DS primitive for the body:
//
//   Edit / Write / MultiEdit → <DiffViewer mode="split"> + file path subheader
//                              (or "diff não capturado" hint when the result
//                              event hasn't been recorded yet)
//   Read                     → <ReactMarkdown> for .md, <CodeBlock> otherwise
//   Bash                     → <CodeBlock> for stdout (+ error-tinted stderr)
//   *                        → JSON fallback via <CodeBlock lang="json">
//
// We never `throw` — when the result payload hasn't landed (only `tool.use`
// is in the DB) we still render the header + a short hint so the user sees
// something meaningful.

import { memo, useState } from "react";
import ReactMarkdown from "react-markdown";
import { ChevronDown, ChevronRight } from "lucide-react";
import { DiffViewer, CodeBlock, type CodeLang } from "@/components/ds";
import { cn } from "@/lib/utils";
import type {
  TraceNode,
  ToolUsePayload,
  ToolResultPayload,
  ToolUseTarget,
} from "@/lib/types/trace";

interface ToolEventRowProps {
  node: TraceNode;
}

export const ToolEventRow = memo(function ToolEventRow({ node }: ToolEventRowProps) {
  // Cast to the real shape — legacy fields land in `payload` as extras
  // (we treat anything missing as `undefined`, never throw).
  const payload = (node.payload ?? {}) as ToolUsePayload & Record<string, unknown>;
  const toolName =
    payload.tool ??
    strField(payload, "tool_name") ??
    strField(payload, "name") ??
    "";
  const target: ToolUseTarget = payload.target ?? {};
  const result: ToolResultPayload | undefined = payload.result;
  const filePath =
    target.file_path ??
    target.file ??
    result?.file_path ??
    strField(payload, "file_path") ??
    strField(payload, "path");

  if (toolName === "Edit" || toolName === "Write" || toolName === "MultiEdit") {
    if (result?.file_before != null && result?.file_after != null) {
      return (
        <PayloadCard toolName={toolName} subheader={filePath} payload={payload}>
          <DiffViewer
            before={result.file_before}
            after={result.file_after}
            mode="split"
            maxLines={200}
          />
        </PayloadCard>
      );
    }
    return (
      <PayloadCard toolName={toolName} subheader={filePath} payload={payload}>
        <DiffPending description={target.description} />
      </PayloadCard>
    );
  }

  if (toolName === "Read") {
    const content = result?.content_excerpt ?? "";
    if (content) {
      const isMarkdown = (filePath ?? "").toLowerCase().endsWith(".md");
      return (
        <PayloadCard toolName="Read" subheader={filePath} payload={payload}>
          {isMarkdown ? (
            <MarkdownBlock source={truncate(content, 200)} />
          ) : (
            <CodeBlock
              code={truncate(content, 200)}
              lang={detectLang(filePath)}
              showLineNumbers
            />
          )}
        </PayloadCard>
      );
    }
    return (
      <PayloadCard toolName="Read" subheader={filePath} payload={payload}>
        <EmptyHint text="Conteúdo não capturado (tool_result pendente)." />
      </PayloadCard>
    );
  }

  if (toolName === "Bash") {
    const command = target.command ?? "";
    const stdout = result?.stdout_excerpt ?? "";
    const stderr = result?.stderr_excerpt ?? "";
    const exitCode = result?.exit_code;
    return (
      <PayloadCard
        toolName="Bash"
        subheader={command ? `$ ${command}` : undefined}
        subheaderMono
        payload={payload}
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
        {exitCode != null && exitCode !== 0 ? (
          <p className="mt-2 text-[11px] text-[--ds-intent-error]">
            exit {exitCode}
          </p>
        ) : null}
        {!stdout && !stderr && !command ? (
          <EmptyHint text="Bash sem resultado capturado." />
        ) : !stdout && !stderr ? (
          <EmptyHint text="Sem stdout/stderr capturado (tool_result pendente)." />
        ) : null}
      </PayloadCard>
    );
  }

  // Fallback — pretty-print whatever payload arrived.
  return (
    <PayloadCard toolName={toolName || "tool.use"} subheader={filePath} payload={payload}>
      <CodeBlock
        code={truncate(JSON.stringify(payload, null, 2), 200)}
        lang="json"
      />
    </PayloadCard>
  );
});

// ── Card wrapper ───────────────────────────────────────────────────────────

interface PayloadCardProps {
  toolName: string;
  subheader?: string;
  subheaderMono?: boolean;
  children: React.ReactNode;
  /** When provided, a small "Ver payload bruto" toggle appears in the
   *  header. Useful for debugging shape drift without leaving the row. */
  payload?: Record<string, unknown>;
}

/** Shared card frame so every tool variant has the same visual rhythm:
 *  small uppercase tool-name pill on top, optional file-path / command
 *  subheader underneath, then the actual payload renderer. */
function PayloadCard({
  toolName,
  subheader,
  subheaderMono,
  children,
  payload,
}: PayloadCardProps) {
  const [showRaw, setShowRaw] = useState(false);
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
        ) : (
          <span className="flex-1" />
        )}
        {payload ? (
          <button
            type="button"
            onClick={() => setShowRaw((v) => !v)}
            className={cn(
              "shrink-0 inline-flex items-center gap-1",
              "text-[10px] text-[--ds-text-tertiary] hover:text-[--ds-text-secondary]",
              "px-1 py-0.5 rounded-[--ds-radius-sm]",
            )}
            aria-expanded={showRaw}
          >
            {showRaw ? (
              <ChevronDown className="h-3 w-3" aria-hidden />
            ) : (
              <ChevronRight className="h-3 w-3" aria-hidden />
            )}
            payload
          </button>
        ) : null}
      </div>
      <div className="p-2">{children}</div>
      {payload && showRaw ? (
        <div className="px-2 pb-2">
          <CodeBlock
            code={truncate(JSON.stringify(payload, null, 2), 200)}
            lang="json"
          />
        </div>
      ) : null}
    </div>
  );
}

// ── Small helpers ──────────────────────────────────────────────────────────

function DiffPending({ description }: { description?: string }) {
  return (
    <EmptyHint
      text={
        description
          ? `Diff não capturado · ${description}`
          : "Diff não capturado (tool_result pendente)."
      }
    />
  );
}

function EmptyHint({ text }: { text: string }) {
  return (
    <p className="text-[11px] italic text-[--ds-text-tertiary] px-1 py-1">
      {text}
    </p>
  );
}

/**
 * Minimal react-markdown wrapper for `.md` file Read previews. We deliberately
 * lean on the v10 defaults (no custom renderers) — the prose is read-only and
 * we already truncate to 200 lines upstream, so the page doesn't need our own
 * `code`/`pre` overrides here.
 */
function MarkdownBlock({ source }: { source: string }) {
  return (
    <div
      className={cn(
        "prose prose-sm max-w-none",
        "text-[--ds-text-secondary]",
        // Keep the rendered markdown visually contained within the card.
        "[&_pre]:bg-[--ds-surface-sunken] [&_pre]:rounded [&_pre]:p-2",
        "[&_code]:bg-[--ds-surface-sunken] [&_code]:px-1 [&_code]:rounded",
      )}
    >
      <ReactMarkdown>{source}</ReactMarkdown>
    </div>
  );
}

// ── Field helpers ──────────────────────────────────────────────────────────

function strField(obj: Record<string, unknown>, key: string): string | undefined {
  const v = obj[key];
  return typeof v === "string" ? v : undefined;
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
