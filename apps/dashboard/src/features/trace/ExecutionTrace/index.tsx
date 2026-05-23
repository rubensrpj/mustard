// Wave 6 + Followup-fix (2026-05-21, spec `2026-05-21-economia-moat-followup-fixes`).
//
// Hierarchical trace viewer for `spec → wave → agent → tool`. Each node is
// rendered as a card (claude-devtools style): elevated background when open,
// flat sunken background when collapsed; large coloured icon per kind on the
// left; semantic badges (kind label, model, duration, tokens) on the right.
//
// Hierarchy is conveyed by a solid `border-l-2` connector + left padding,
// not by a tree-of-rows. Native `<details>` keeps a11y / keyboard for free
// and lets the browser handle lazy mounting of `payload` for tool nodes.
//
// Top-level toolbar exposes "Expand all" / "Collapse all" via a numeric
// generation counter — each click bumps a `forcedKey` that `<TraceNodeRow>`
// merges into its `<details>` `open` prop, so users can also collapse /
// expand individual sub-trees manually between bulk actions.

import { memo, useCallback, useState, type ReactNode } from "react";
import {
  Square,
  Layers,
  Cpu,
  Wrench,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
} from "lucide-react";
import { useSpecTrace } from "@/hooks/useSpecTrace";
import type { TraceKind, TraceNode, TokenBreakdown } from "@/lib/types/trace";
import { StatPill } from "@/components/page";
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
 * inputs and delegates the rest to the recursive `TraceNodeRow`.
 *
 * Wave 1 polish (spec `2026-05-21-dashboard-spec-tabs-polish`) — the per-node
 * `open` state was previously a `useState` inside the recursive `TraceNodeRow`,
 * which meant every TanStack Query refetch (5 s cadence) remounted the leaves
 * and silently reset their expansion. We now hold an `expanded: Set<string>`
 * at the top level and pass a path-keyed `isOpen` + `toggle` pair to each row.
 * Node ids are built from the recursion path (`${parentKey}/${kind}-${idx}`)
 * so the Set survives across refetches without colliding between siblings.
 */
export function ExecutionTrace({
  projectPath,
  specName,
  className,
}: ExecutionTraceProps) {
  const { data, isLoading, error } = useSpecTrace(projectPath, specName);
  // `forced` carries the latest bulk-expand/collapse intent. `null` means
  // "respect each node's default"; any number is an even/odd generation that
  // toggles every click so the same intent applied twice still re-renders.
  const [forced, setForced] = useState<{ open: boolean; gen: number } | null>(
    null,
  );
  // Path-keyed expansion state. Lives at the top so per-leaf re-mounts on
  // refetch (TanStack Query swaps the data object) can't wipe it.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const isOpenById = useCallback(
    (id: string, defaultOpen: boolean): boolean => {
      // When the user has explicitly touched this node, honour that. Default
      // (spec/wave open, agent/tool collapsed) only applies on first sight.
      if (expanded.has(`+${id}`)) return true;
      if (expanded.has(`-${id}`)) return false;
      return defaultOpen;
    },
    [expanded],
  );
  const toggleById = useCallback((id: string, defaultOpen: boolean) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      const currentlyOpen = next.has(`+${id}`)
        ? true
        : next.has(`-${id}`)
          ? false
          : defaultOpen;
      next.delete(`+${id}`);
      next.delete(`-${id}`);
      next.add(currentlyOpen ? `-${id}` : `+${id}`);
      return next;
    });
  }, []);

  if (!projectPath || !specName) {
    return (
      <div className={cn("text-[12px] text-[--ds-text-tertiary] px-2 py-3", className)}>
        Sem spec ativa para rastrear.
      </div>
    );
  }
  if (isLoading) {
    return (
      <div className={cn("flex flex-col gap-2 px-2 py-3", className)}>
        {[0, 1, 2].map((i) => (
          <div
            key={i}
            className="h-12 bg-[--ds-surface-hover] rounded-[--ds-radius-md] animate-pulse"
          />
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
    <div className={cn("flex flex-col gap-2 font-sans text-[13px]", className)}>
      <div className="flex items-center gap-1 self-end text-[11px] text-[--ds-text-tertiary]">
        <button
          type="button"
          onClick={() =>
            setForced({ open: true, gen: (forced?.gen ?? 0) + 1 })
          }
          className={cn(
            "inline-flex items-center gap-1 px-2 py-1 rounded-[--ds-radius-sm]",
            "hover:bg-[--ds-surface-hover] hover:text-[--ds-text-primary]",
            "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[--ds-accent-primary]/60",
          )}
          title="Expandir tudo"
        >
          <ChevronsUpDown size={12} aria-hidden />
          Expandir tudo
        </button>
        <button
          type="button"
          onClick={() =>
            setForced({ open: false, gen: (forced?.gen ?? 0) + 1 })
          }
          className={cn(
            "inline-flex items-center gap-1 px-2 py-1 rounded-[--ds-radius-sm]",
            "hover:bg-[--ds-surface-hover] hover:text-[--ds-text-primary]",
            "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[--ds-accent-primary]/60",
          )}
          title="Colapsar tudo"
        >
          <ChevronsDownUp size={12} aria-hidden />
          Colapsar tudo
        </button>
      </div>
      <TraceNodeRow
        node={data}
        depth={0}
        nodeId="root"
        forced={forced}
        isOpenById={isOpenById}
        toggleById={toggleById}
        setExpanded={setExpanded}
      />
    </div>
  );
}

// ── Recursive row ──────────────────────────────────────────────────────────

interface TraceNodeRowProps {
  node: TraceNode;
  depth: number;
  /** Path-keyed id of this node — built from the recursion ancestry. */
  nodeId: string;
  /** Latest bulk expand/collapse intent (see `ExecutionTrace`). */
  forced: { open: boolean; gen: number } | null;
  /** Top-level expansion lookup keyed by `nodeId`. */
  isOpenById: (id: string, defaultOpen: boolean) => boolean;
  /** Toggle helper keyed by `nodeId`. */
  toggleById: (id: string, defaultOpen: boolean) => void;
  /** Raw setter — used to swallow per-node state during bulk forced ops. */
  setExpanded: React.Dispatch<React.SetStateAction<Set<string>>>;
}

const KIND_ICON: Record<TraceKind, typeof Square> = {
  spec: Square,
  wave: Layers,
  agent: Cpu,
  tool: Wrench,
};

/** Icon colour per kind — see spec `2026-05-21-economia-moat-followup-fixes`
 *  (claude-devtools palette: indigo / blue / green / amber). */
const KIND_ICON_COLOR: Record<TraceKind, string> = {
  spec: "text-[--ds-accent-primary]",
  wave: "text-[--ds-intent-info]",
  agent: "text-[--ds-intent-success]",
  tool: "text-[--ds-status-draft]",
};

const KIND_LABEL: Record<TraceKind, string> = {
  spec: "SPEC",
  wave: "WAVE",
  agent: "AGENT",
  tool: "TOOL",
};

const TraceNodeRow = memo(function TraceNodeRow({
  node,
  depth,
  nodeId,
  forced,
  isOpenById,
  toggleById,
  setExpanded,
}: TraceNodeRowProps) {
  const Icon = KIND_ICON[node.kind];
  const iconColor = KIND_ICON_COLOR[node.kind];
  const hasChildren = node.children.length > 0;
  // Specs and waves stay open by default; agents/tools collapse so the
  // initial view doesn't drown the reader.
  const defaultOpen = node.kind === "spec" || node.kind === "wave";

  // Honour the top-level lookup. On bulk expand/collapse, an effect-free
  // generation guard rewrites the explicit override so this row reflects the
  // latest intent without losing other rows the user has manually touched.
  const open = isOpenById(nodeId, defaultOpen);
  const [lastGen, setLastGen] = useState<number>(0);
  if (forced && forced.gen !== lastGen) {
    setLastGen(forced.gen);
    setExpanded((prev) => {
      const next = new Set(prev);
      next.delete(`+${nodeId}`);
      next.delete(`-${nodeId}`);
      next.add(forced.open ? `+${nodeId}` : `-${nodeId}`);
      return next;
    });
  }

  // Indentation is owned by the parent's `children` container, so each row
  // only worries about its own card. Tool leaves get no expand chevron when
  // they have no payload.
  const expandable = hasChildren || node.kind === "tool";

  const header: ReactNode = (
    <div
      className={cn(
        "flex items-center gap-2.5 px-3 py-2 rounded-[--ds-radius-md]",
        "cursor-pointer select-none transition-colors",
        open
          ? "bg-[--ds-surface-elevated] border border-[--ds-surface-hover]"
          : "bg-[--ds-surface-base] border border-transparent hover:bg-[--ds-surface-hover]",
      )}
    >
      {expandable ? (
        <ChevronRight
          size={14}
          className={cn(
            "text-[--ds-text-tertiary] shrink-0 transition-transform",
            open && "rotate-90",
          )}
          aria-hidden
        />
      ) : (
        <span className="inline-block w-3.5 shrink-0" />
      )}
      <Icon size={18} className={cn("shrink-0", iconColor)} aria-hidden />
      <span className="font-medium text-[13px] text-[--ds-text-primary] truncate flex-1 min-w-0">
        {node.label}
      </span>
      <span
        className={cn(
          "shrink-0 px-1.5 py-0.5 rounded-[--ds-radius-sm]",
          "text-[10px] tracking-wide font-medium",
          "bg-[--ds-surface-hover] text-[--ds-text-secondary]",
        )}
        title={`kind: ${node.kind}`}
      >
        {KIND_LABEL[node.kind]}
      </span>
      {modelOf(node) ? (
        <span
          className={cn(
            "shrink-0 px-1.5 py-0.5 rounded-[--ds-radius-sm]",
            "text-[10px] font-mono text-[--ds-text-tertiary]",
            "bg-[--ds-surface-sunken]",
          )}
          title="model"
        >
          {modelOf(node)}
        </span>
      ) : null}
      {node.duration_ms != null ? (
        <StatPill
          value={formatDuration(node.duration_ms)}
          unit=""
          intent="neutral"
        />
      ) : null}
      {node.tokens ? <TokenPill tokens={node.tokens} /> : null}
    </div>
  );

  // Container that wraps children with the solid vertical connector. Depth
  // is consumed only to widen the connector colour on deeper trees; the
  // first level keeps the same accent for visual continuity.
  const childrenContainer = (
    <div
      className={cn(
        "mt-1.5 ml-4 pl-3 border-l-2 border-[--ds-surface-hover]",
        "flex flex-col gap-1.5",
      )}
    >
      {hasChildren
        ? node.children.map((child, idx) => {
            const childId = `${nodeId}/${child.kind}-${idx}`;
            return (
              <TraceNodeRow
                key={childId}
                node={child}
                depth={depth + 1}
                nodeId={childId}
                forced={forced}
                isOpenById={isOpenById}
                toggleById={toggleById}
                setExpanded={setExpanded}
              />
            );
          })
        : null}
      {node.kind === "tool" ? (
        <div className="rounded-[--ds-radius-md] overflow-hidden">
          <ToolEventRow node={node} />
        </div>
      ) : null}
    </div>
  );

  if (!expandable) {
    return <div>{header}</div>;
  }

  return (
    <div>
      <button
        type="button"
        onClick={() => toggleById(nodeId, defaultOpen)}
        aria-expanded={open}
        className={cn(
          "block w-full text-left rounded-[--ds-radius-md]",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[--ds-accent-primary]/60",
        )}
      >
        {header}
      </button>
      {open ? childrenContainer : null}
    </div>
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

/** Extract a model identifier from the node's payload (agent / tool nodes
 *  may carry `model` directly). Returns null when absent. */
function modelOf(node: TraceNode): string | null {
  const p = node.payload as Record<string, unknown> | null;
  if (!p) return null;
  const v = p["model"] ?? p["model_id"];
  return typeof v === "string" && v.length > 0 ? v : null;
}

interface TokenPillProps {
  tokens: TokenBreakdown;
}

function TokenPill({ tokens }: TokenPillProps) {
  const total =
    tokens.input + tokens.output + tokens.cache_read + tokens.cache_creation;
  if (total <= 0) return null;
  const tooltip =
    `input ${tokens.input} · output ${tokens.output}` +
    (tokens.cache_read > 0 ? ` · cache_read ${tokens.cache_read}` : "") +
    (tokens.cache_creation > 0
      ? ` · cache_creation ${tokens.cache_creation}`
      : "") +
    (tokens.cost_usd_micros != null
      ? ` · cost ${(tokens.cost_usd_micros / 1_000_000).toFixed(4)} USD`
      : "");
  return <StatPill value={formatTokens(total)} unit="tok" tooltip={tooltip} />;
}
