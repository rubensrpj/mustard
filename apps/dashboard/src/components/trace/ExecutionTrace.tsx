// Wave 6 — recursive trace viewer.
//
// Renders a spec → wave → agent → tool tree as nested native <details>
// blocks so keyboard / a11y come for free and lazy rendering of `payload`
// happens via the browser instead of React state. `TraceNodeRow` is memoized
// to keep large trees performant — pure presentation, no fetch.

import { memo, type ReactNode } from "react";
import { Square, Layers, Cpu, Wrench, ChevronRight } from "lucide-react";
import { useSpecTrace } from "@/hooks/useSpecTrace";
import type { TraceKind, TraceNode, TokenBreakdown } from "@/lib/types/trace";
import { MetricsPill } from "@/components/ds";
import { formatTokens } from "@/lib/types/economy";
import { ToolEventRow } from "./ToolEventRow";
import { cn } from "@/lib/utils";

interface ExecutionTraceProps {
  projectPath: string | null;
  specName: string | null;
  className?: string;
}

/**
 * Top-level trace container. Renders the empty state when the query has no
 * inputs (no active spec, no workspace) and lets the recursive `TraceNodeRow`
 * handle the rest. The component is intentionally thin so other pages
 * (Workspace, Specs drill-down) can embed it without re-wrapping.
 */
export function ExecutionTrace({
  projectPath,
  specName,
  className,
}: ExecutionTraceProps) {
  const { data, isLoading, error } = useSpecTrace(projectPath, specName);

  if (!projectPath || !specName) {
    return (
      <div className={cn("text-[12px] text-[--ds-text-tertiary] px-2 py-3", className)}>
        Sem spec ativa para rastrear.
      </div>
    );
  }
  if (isLoading) {
    return (
      <div className={cn("flex flex-col gap-1 px-2 py-3", className)}>
        {[0, 1, 2].map((i) => (
          <div key={i} className="h-7 bg-[--ds-surface-hover] rounded animate-pulse" />
        ))}
      </div>
    );
  }
  if (error) {
    return (
      <div className={cn("text-[12px] text-[--ds-intent-error] px-2 py-3", className)}>
        Erro ao carregar trace: {error.message}
      </div>
    );
  }
  if (!data) {
    return (
      <div className={cn("text-[12px] text-[--ds-text-tertiary] px-2 py-3", className)}>
        Nenhum evento registrado para esta spec ainda.
      </div>
    );
  }

  return (
    <div className={cn("flex flex-col gap-0.5 font-sans text-[13px]", className)}>
      <TraceNodeRow node={data} depth={0} />
    </div>
  );
}

// ── Recursive row ──────────────────────────────────────────────────────────

interface TraceNodeRowProps {
  node: TraceNode;
  depth: number;
}

const KIND_ICON: Record<TraceKind, typeof Square> = {
  spec: Square,
  wave: Layers,
  agent: Cpu,
  tool: Wrench,
};

const KIND_TEXT: Record<TraceKind, string> = {
  spec:  "text-[--ds-accent-primary]",
  wave:  "text-[--ds-intent-info]",
  agent: "text-[--ds-text-primary]",
  tool:  "text-[--ds-text-secondary]",
};

const TraceNodeRow = memo(function TraceNodeRow({
  node,
  depth,
}: TraceNodeRowProps) {
  const Icon = KIND_ICON[node.kind];
  const hasChildren = node.children.length > 0;
  // Specs and waves stay open by default (top-of-tree context); agents and
  // tools collapse so the initial view doesn't drown the reader.
  const defaultOpen = node.kind === "spec" || node.kind === "wave";
  // Indentation is a flat margin per depth — keeps the tree shallow visually
  // even at depth=3 (tool nodes) while still preserving hierarchy.
  const indentClass = depth === 0 ? "" : "ml-3 pl-2 border-l border-dashed border-[--ds-surface-hover]";

  const summary: ReactNode = (
    <div
      className={cn(
        "flex items-center gap-2 py-1 px-1.5 rounded-[--ds-radius-sm] cursor-pointer select-none",
        "hover:bg-[--ds-surface-hover]",
      )}
    >
      {hasChildren ? (
        <ChevronRight
          size={12}
          className="text-[--ds-text-tertiary] transition-transform group-open:rotate-90 shrink-0"
        />
      ) : (
        <span className="inline-block w-3 shrink-0" />
      )}
      <Icon size={13} className={cn("shrink-0", KIND_TEXT[node.kind])} />
      <span className={cn("truncate flex-1 text-[13px]", KIND_TEXT[node.kind])}>
        {node.label}
      </span>
      {node.duration_ms != null ? (
        <MetricsPill value={formatDuration(node.duration_ms)} unit="" intent="neutral" />
      ) : null}
      {node.tokens ? <TokenPill tokens={node.tokens} /> : null}
    </div>
  );

  // Leaf (tool) — render via ToolEventRow which knows how to pivot payload.
  if (node.kind === "tool") {
    return (
      <div className={indentClass}>
        <details className="group">
          <summary className="list-none [&::-webkit-details-marker]:hidden">
            {summary}
          </summary>
          <div className="mt-1 ml-5">
            <ToolEventRow node={node} />
          </div>
        </details>
      </div>
    );
  }

  if (!hasChildren) {
    return <div className={indentClass}>{summary}</div>;
  }

  return (
    <details open={defaultOpen} className={cn("group", indentClass)}>
      <summary className="list-none [&::-webkit-details-marker]:hidden">
        {summary}
      </summary>
      <div className="mt-0.5">
        {node.children.map((child, idx) => (
          <TraceNodeRow
            key={`${child.kind}-${idx}-${child.label}`}
            node={child}
            depth={depth + 1}
          />
        ))}
      </div>
    </details>
  );
});

// ── Helpers ────────────────────────────────────────────────────────────────

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  const minutes = Math.floor(ms / 60_000);
  const seconds = Math.floor((ms % 60_000) / 1000);
  return `${minutes}m${seconds.toString().padStart(2, "0")}s`;
}

interface TokenPillProps {
  tokens: TokenBreakdown;
}

function TokenPill({ tokens }: TokenPillProps) {
  const total = tokens.input + tokens.output + tokens.cache_read + tokens.cache_creation;
  if (total <= 0) return null;
  const tooltip =
    `input ${tokens.input} · output ${tokens.output}` +
    (tokens.cache_read > 0 ? ` · cache_read ${tokens.cache_read}` : "") +
    (tokens.cache_creation > 0 ? ` · cache_creation ${tokens.cache_creation}` : "") +
    (tokens.cost_usd_micros != null
      ? ` · cost ${(tokens.cost_usd_micros / 1_000_000).toFixed(4)} USD`
      : "");
  return <MetricsPill value={formatTokens(total)} unit="tok" tooltip={tooltip} />;
}
