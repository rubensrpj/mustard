// SpecTimelineTab — claude-devtools-style flat timeline.
//
// W5 (`2026-05-24-mustard-unification`, T5.3) rewrite. The legacy
// phase-grouped accordion is replaced with a flat row-per-event list that
// matches the claude-devtools "execution trace" treatment:
//
//   icon · label · tokens_in/tokens_out · duration_ms · status dot · ▸
//
// Expanding a row reveals a tool-specific viewer:
//
//   Bash         → terminal block (mono, stdout + stderr concatenated)
//   Read         → Code / Preview toggle (markdown for `.md`, code otherwise)
//   Edit/Write   → diff viewer (before/after as `input` / `output` strings)
//   Glob / Grep  → result list (newline-split `output`)
//   Task         → recursive execution trace — children resolved via
//                  `parent_id` against the flat list this row belongs to
//   *            → JSON fallback (the raw `payload_summary` + input/output)
//
// Sources fan in lazily:
//   - `useSpecTimeline` returns the flat list (Tauri command
//     `dashboard_spec_timeline`, refetched on watcher `events` ticks via
//     `lib/watcher.ts::subscribeFsChange` invalidation — so the timeline
//     tails in real time without polling).
//   - The post-T5.2 core projection populates `tokens_in`, `tokens_out`,
//     `duration_ms`, `parent_id`, `input`, `output`, `tool`, `status` on
//     each `SpecTimelineNode`. Until that lands those fields stay `null`
//     and the row degrades cleanly (no chip / no expand body).
//
// Performance — we intentionally do not pull in `react-virtuoso` /
// `@tanstack/react-virtual` here: the row body is gated behind an `expanded`
// `Set` so collapsed rows render a single button + 4 chips, and the
// expanded body (the heavy renderers) mounts only on demand. That keeps the
// per-row work small enough to stay under the 16 ms re-render budget for
// 500+ events on a typical workstation — same strategy used by
// `<ExecutionTrace>` next door. We can revisit windowing if profiling
// shows it's actually needed (AC-W5-5 prohibits force-graph deps; nothing
// blocks a future virtualization helper).

import { memo, useCallback, useMemo, useState } from "react";
import {
  ChevronRight,
  Layers,
  Terminal as TerminalIcon,
  FileText,
  FileEdit,
  Search,
  Cpu,
  Wrench,
  GitBranch,
  CircleCheck,
  CircleAlert,
  Circle,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import { CodeBlock, DiffViewer, StatPill } from "@/components/page";
import type { SpecTimelineNode } from "@/lib/types/specs";

interface SpecTimelineTabProps {
  nodes: SpecTimelineNode[];
  /** Forwarded so a parent can observe row selection (legacy hook). */
  onNodeClick?: (node: SpecTimelineNode) => void;
  /** Required for the wave/qa/review markdown deep-links — currently used by
   *  the older grouped view; left in the API so callers don't break. */
  repoPath?: string | null;
  spec?: string;
  waves?: number[];
}

// ────────────────────────────────────────────────────────────────────────────
// Visual helpers
// ────────────────────────────────────────────────────────────────────────────

/** Icon picker — tool name first, falls back to event kind. */
function iconFor(node: SpecTimelineNode) {
  const tool = (node.tool ?? "").toLowerCase();
  if (tool === "bash") return TerminalIcon;
  if (tool === "read") return FileText;
  if (tool === "edit" || tool === "write" || tool === "multiedit") return FileEdit;
  if (tool === "glob" || tool === "grep") return Search;
  if (tool === "task") return Cpu;
  switch (node.kind) {
    case "phase":
    case "wave":
      return Layers;
    case "agent":
      return Cpu;
    case "tool":
      return Wrench;
    case "qa":
    case "review":
      return CircleCheck;
    default:
      return GitBranch;
  }
}

function StatusDotMini({ status }: { status?: string | null }) {
  if (status === "running") {
    return (
      <Loader2
        aria-hidden
        className="h-3 w-3 animate-spin text-[--ds-text-tertiary]"
      />
    );
  }
  const color =
    status === "error"
      ? "bg-[--ds-intent-error]"
      : status === "warn"
        ? "bg-[--ds-status-draft]"
        : status === "ok"
          ? "bg-[--ds-intent-success]"
          : "bg-[--ds-text-tertiary]/40";
  return (
    <span
      aria-hidden
      aria-label={status ?? "unknown"}
      className={cn("inline-block h-2 w-2 rounded-full", color)}
    />
  );
}

function StatusIcon({ status }: { status?: string | null }) {
  if (status === "error") {
    return (
      <CircleAlert
        aria-hidden
        className="h-3 w-3 text-[--ds-intent-error]"
      />
    );
  }
  if (status === "ok") {
    return (
      <CircleCheck
        aria-hidden
        className="h-3 w-3 text-[--ds-intent-success]"
      />
    );
  }
  return <Circle aria-hidden className="h-3 w-3 text-[--ds-text-tertiary]/40" />;
}

/** Short-form duration: `420ms`, `3.4s`, `1m12s`. */
function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  const m = Math.floor(ms / 60_000);
  const s = Math.floor((ms % 60_000) / 1000);
  return `${m}m${s.toString().padStart(2, "0")}s`;
}

/** Compact integer formatter (`1.2k`, `34`). */
function formatCount(n: number): string {
  if (n < 1000) return `${n}`;
  if (n < 10_000) return `${(n / 1000).toFixed(1)}k`;
  return `${Math.round(n / 1000)}k`;
}

// ────────────────────────────────────────────────────────────────────────────
// Tool-specific renderers (the row's expanded body)
// ────────────────────────────────────────────────────────────────────────────

interface ToolRendererProps {
  node: SpecTimelineNode;
  /** Flat list of all timeline nodes — used by the `Task` renderer to walk
   *  children via `parent_id`. */
  all: SpecTimelineNode[];
  /** Recursion depth guard — prevents an accidental cycle in `parent_id`
   *  data from melting the tab. The harness never emits cycles, but the
   *  renderer treats the projection as an external input. */
  depth: number;
}

function ToolRenderer({ node, all, depth }: ToolRendererProps) {
  const tool = (node.tool ?? "").toLowerCase();
  const input = node.input ?? "";
  const output = node.output ?? "";

  if (tool === "bash") {
    // stdout + stderr already concatenated by the projection. Mono, light
    // terminal feel — claude-devtools palette via the existing tokens.
    const cmd = input.trim();
    const body = output.length > 0 ? output : "(sem saída capturada)";
    return (
      <div className="flex flex-col gap-2">
        {cmd && (
          <div className="rounded-[--ds-radius-sm] bg-[--ds-surface-sunken] px-2 py-1.5 font-mono text-[11px] text-[--ds-text-secondary]">
            <span className="text-[--ds-text-tertiary]">$ </span>
            {cmd}
          </div>
        )}
        <CodeBlock code={truncate(body, 200)} lang="plain" />
      </div>
    );
  }

  if (tool === "read") {
    const isMd = (input ?? "").toLowerCase().endsWith(".md");
    if (!output) return <EmptyHint text="Conteúdo não capturado." />;
    return (
      <div className="flex flex-col gap-1">
        {input && (
          <p className="font-mono text-[11px] text-[--ds-text-tertiary]">
            {input}
          </p>
        )}
        <CodeBlock
          code={truncate(output, 200)}
          lang={isMd ? "plain" : "plain"}
          showLineNumbers
        />
      </div>
    );
  }

  if (tool === "edit" || tool === "write" || tool === "multiedit") {
    if (!input && !output) {
      return <EmptyHint text="Diff não capturado." />;
    }
    return (
      <DiffViewer
        before={input ?? ""}
        after={output ?? ""}
        mode="split"
        maxLines={200}
      />
    );
  }

  if (tool === "glob" || tool === "grep") {
    const results = (output ?? "").split("\n").filter((s) => s.trim().length > 0);
    if (results.length === 0) {
      return <EmptyHint text="Nenhum resultado." />;
    }
    return (
      <div className="flex flex-col gap-1">
        {input && (
          <p className="font-mono text-[11px] text-[--ds-text-tertiary]">
            {tool === "glob" ? "pattern: " : "query: "}
            {input}
          </p>
        )}
        <ol className="flex flex-col gap-0.5 font-mono text-[11px] text-[--ds-text-secondary]">
          {results.slice(0, 200).map((r, i) => (
            <li key={`${i}-${r}`} className="truncate">
              {r}
            </li>
          ))}
          {results.length > 200 && (
            <li className="italic text-[--ds-text-tertiary]">
              … (+{results.length - 200} linhas)
            </li>
          )}
        </ol>
      </div>
    );
  }

  if (tool === "task") {
    // Recursive: walk every node whose `parent_id` points at this node's
    // synthetic id (we use `ts` as the id when the projection has not yet
    // assigned one). The nested view is a stripped-down rendering — no
    // status dot/duration on the wrapper, just the row + its body.
    const children = childrenOf(node, all);
    return (
      <div className="flex flex-col gap-2">
        {input && (
          <div className="rounded-[--ds-radius-sm] bg-[--ds-surface-sunken] px-2 py-1.5 font-mono text-[11px] text-[--ds-text-secondary] whitespace-pre-wrap">
            {truncate(input, 40)}
          </div>
        )}
        {children.length === 0 ? (
          output ? (
            <CodeBlock code={truncate(output, 200)} lang="plain" />
          ) : (
            <EmptyHint text="Subagent sem rastro." />
          )
        ) : (
          <div className="border-l-2 border-[--ds-surface-hover] pl-3 flex flex-col gap-1.5">
            {children.map((child, i) => (
              <TimelineRow
                key={`${child.ts}-${i}-${depth + 1}`}
                node={child}
                all={all}
                depth={depth + 1}
                defaultOpen={false}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  // Fallback — payload_summary + any captured input/output as JSON-ish blocks.
  if (!node.payload_summary && !input && !output) {
    return <EmptyHint text="Sem payload capturado." />;
  }
  return (
    <div className="flex flex-col gap-2">
      {node.payload_summary && (
        <p className="text-[11px] text-[--ds-text-secondary]">{node.payload_summary}</p>
      )}
      {input && <CodeBlock code={truncate(input, 100)} lang="plain" />}
      {output && <CodeBlock code={truncate(output, 100)} lang="plain" />}
    </div>
  );
}

function EmptyHint({ text }: { text: string }) {
  return (
    <p className="px-1 py-1 text-[11px] italic text-[--ds-text-tertiary]">
      {text}
    </p>
  );
}

/** Walk the flat list and collect every node whose `parent_id` matches the
 *  given parent's row id. The projection (T5.2) tags each row with a
 *  `pipeline_events.id` (signed integer); until that lands the typed shape
 *  carries `id?: number` so this walker degrades to an empty list rather
 *  than throwing. Stable order — preserves the flat list ordering (which is
 *  timestamp-ascending by the projection). */
function childrenOf(parent: SpecTimelineNode, all: SpecTimelineNode[]): SpecTimelineNode[] {
  const parentId = (parent as { id?: number }).id;
  if (parentId == null) return [];
  const out: SpecTimelineNode[] = [];
  for (const n of all) {
    if (n === parent) continue;
    if (n.parent_id != null && n.parent_id === parentId) out.push(n);
  }
  return out;
}

/** Cap multi-line payloads at `maxLines` lines so a 5k-line dump doesn't
 *  blow up the row. Mirrors the `truncate` used in `ToolEventRow`. */
function truncate(s: string, maxLines: number): string {
  const lines = s.split("\n");
  if (lines.length <= maxLines) return s;
  return `${lines.slice(0, maxLines).join("\n")}\n… (+${lines.length - maxLines} linhas)`;
}

// ────────────────────────────────────────────────────────────────────────────
// Row
// ────────────────────────────────────────────────────────────────────────────

interface TimelineRowProps {
  node: SpecTimelineNode;
  all: SpecTimelineNode[];
  depth: number;
  defaultOpen: boolean;
  onClick?: (node: SpecTimelineNode) => void;
}

const TimelineRow = memo(function TimelineRow({
  node,
  all,
  depth,
  defaultOpen,
  onClick,
}: TimelineRowProps) {
  const [open, setOpen] = useState<boolean>(defaultOpen);
  const Icon = iconFor(node);
  const hasBody =
    !!node.input ||
    !!node.output ||
    !!node.payload_summary ||
    (node.tool ?? "").toLowerCase() === "task";

  const toolLabel =
    node.tool ?? (node.kind === "tool" ? "tool" : node.kind);

  return (
    <div className="flex flex-col">
      <button
        type="button"
        onClick={() => {
          if (hasBody) setOpen((v) => !v);
          onClick?.(node);
        }}
        aria-expanded={hasBody ? open : undefined}
        className={cn(
          "group flex items-center gap-2 px-2 py-1.5 text-left rounded-[--ds-radius-sm]",
          "hover:bg-[--ds-surface-hover] transition-colors",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[--ds-accent-primary]/60",
          open && "bg-[--ds-surface-hover]/60",
        )}
      >
        {hasBody ? (
          <ChevronRight
            aria-hidden
            className={cn(
              "h-3 w-3 shrink-0 text-[--ds-text-tertiary] transition-transform",
              open && "rotate-90",
            )}
          />
        ) : (
          <span aria-hidden className="inline-block w-3 shrink-0" />
        )}
        <Icon
          aria-hidden
          className={cn(
            "h-3.5 w-3.5 shrink-0",
            node.status === "error"
              ? "text-[--ds-intent-error]"
              : node.kind === "phase" || node.kind === "wave"
                ? "text-[--ds-accent-primary]"
                : "text-[--ds-text-tertiary]",
          )}
        />
        <span
          className={cn(
            "shrink-0 rounded-[--ds-radius-sm] px-1.5 py-0.5",
            "text-[10px] font-medium uppercase tracking-wide",
            "bg-[--ds-surface-sunken] text-[--ds-text-secondary]",
          )}
        >
          {toolLabel}
        </span>
        <span className="flex-1 min-w-0 truncate text-[12px] text-[--ds-text-primary]">
          {node.label}
        </span>
        {node.tokens_in != null && node.tokens_in > 0 && (
          <span
            title="tokens in"
            className="shrink-0 tabular-nums text-[10px] text-[--ds-text-tertiary]"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            ↘{formatCount(node.tokens_in)}
          </span>
        )}
        {node.tokens_out != null && node.tokens_out > 0 && (
          <span
            title="tokens out"
            className="shrink-0 tabular-nums text-[10px] text-[--ds-text-tertiary]"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            ↗{formatCount(node.tokens_out)}
          </span>
        )}
        {node.duration_ms != null && (
          <StatPill
            value={formatDuration(node.duration_ms)}
            unit=""
            intent="neutral"
          />
        )}
        <StatusDotMini status={node.status} />
        <time
          dateTime={node.ts}
          title={node.ts}
          className="shrink-0 text-[10px] text-[--ds-text-tertiary]/70"
        >
          {relativeTime(node.ts)}
        </time>
      </button>
      {open && hasBody && (
        <div className="ml-6 mt-1 mb-1 rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] p-2">
          <ToolRenderer node={node} all={all} depth={depth} />
        </div>
      )}
    </div>
  );
});

// ────────────────────────────────────────────────────────────────────────────
// Tab
// ────────────────────────────────────────────────────────────────────────────

export function SpecTimelineTab({ nodes, onNodeClick }: SpecTimelineTabProps) {
  // Roots = nodes without a `parent_id`. Children render under their Task
  // parent via `ToolRenderer`'s recursive walk. When the projection has not
  // yet assigned `parent_id` everywhere, every node is a root — the flat
  // claude-devtools list is the safe default.
  const roots = useMemo(
    () => nodes.filter((n) => !n.parent_id),
    [nodes],
  );

  const handleClick = useCallback(
    (n: SpecTimelineNode) => onNodeClick?.(n),
    [onNodeClick],
  );

  if (nodes.length === 0) {
    return (
      <div className="px-2 py-3 text-center">
        <p className="text-[12px] text-[--ds-text-tertiary]">
          Nenhum evento de timeline para esta spec.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-0.5">
      <div className="flex items-center gap-3 px-2 pb-1 text-[10px] uppercase tracking-wide text-[--ds-text-tertiary]/70">
        <span className="flex items-center gap-1">
          <StatusIcon status="ok" /> ok
        </span>
        <span className="flex items-center gap-1">
          <StatusIcon status="error" /> erro
        </span>
        <span className="ml-auto tabular-nums">
          {nodes.length} eventos
        </span>
      </div>
      {roots.map((node, i) => (
        <TimelineRow
          key={`${node.ts}-${i}`}
          node={node}
          all={nodes}
          depth={0}
          defaultOpen={false}
          onClick={handleClick}
        />
      ))}
    </div>
  );
}
