import { useMemo, useState } from "react";
import { ChevronRight, FileText } from "lucide-react";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import { SpecMarkdownViewer, type SpecMarkdownKind } from "../SpecMarkdownViewer";
import type { SpecTimelineNode } from "@/lib/types/specs";

interface SpecTimelineTabProps {
  nodes: SpecTimelineNode[];
  /** When a node is clicked, emit it so SpecDrillDown can filter SpecEventsTab. */
  onNodeClick?: (node: SpecTimelineNode) => void;
  /** Repo + spec are required to surface "ver markdown" deep links per node. */
  repoPath?: string | null;
  spec?: string;
  /** Wave numbers known for this spec (drives the wave tabs in the viewer). */
  waves?: number[];
}

const KIND_ACCENT: Record<string, string> = {
  phase:  "text-[--primary]",
  wave:   "text-foreground/80",
  qa:     "text-[--intent-success]",
  review: "text-muted-foreground",
  agent:  "text-muted-foreground",
  tool:   "text-muted-foreground/60",
  other:  "text-muted-foreground/40",
};

const KIND_BADGE_CLS: Record<string, string> = {
  phase:  "bg-[--primary]/15 text-[--primary]",
  wave:   "bg-muted text-foreground/80",
  qa:     "bg-[--intent-success]/15 text-[--intent-success]",
  review: "bg-muted text-muted-foreground",
  agent:  "bg-muted text-muted-foreground",
  tool:   "bg-muted/60 text-muted-foreground/70",
  other:  "bg-muted/40 text-muted-foreground/60",
};

const KIND_LABEL: Record<string, string> = {
  phase:  "fase",
  wave:   "onda",
  qa:     "QA",
  review: "review",
  agent:  "agente",
  tool:   "tool",
  other:  "outro",
};

const PHASE_ORDER = ["analyze", "plan", "execute", "qa", "close"];
const NO_PHASE = "__no_phase__";

function phaseLabel(phase: string): string {
  if (phase === NO_PHASE) return "Sem fase";
  return phase.charAt(0).toUpperCase() + phase.slice(1);
}

function groupByPhase(
  nodes: SpecTimelineNode[],
): { phase: string; nodes: SpecTimelineNode[] }[] {
  const map = new Map<string, SpecTimelineNode[]>();
  for (const n of nodes) {
    const key = n.phase && n.phase.length > 0 ? n.phase.toLowerCase() : NO_PHASE;
    const list = map.get(key) ?? [];
    list.push(n);
    map.set(key, list);
  }
  // Stable order: known phases first by canonical sequence, unknown phases by
  // first appearance, "no phase" bucket last.
  const ordered: string[] = [];
  for (const p of PHASE_ORDER) if (map.has(p)) ordered.push(p);
  for (const key of map.keys()) {
    if (!PHASE_ORDER.includes(key) && key !== NO_PHASE) ordered.push(key);
  }
  if (map.has(NO_PHASE)) ordered.push(NO_PHASE);
  return ordered.map((phase) => ({ phase, nodes: map.get(phase) ?? [] }));
}

/** True when this node corresponds to an artifact the markdown viewer can open. */
function markdownTargetFor(
  node: SpecTimelineNode,
): { kind: SpecMarkdownKind; wave?: number } | null {
  if (node.kind === "qa") return { kind: "qa" };
  if (node.kind === "review") return { kind: "review" };
  if (node.kind === "wave" && node.wave != null) {
    return { kind: "wave", wave: node.wave };
  }
  return null;
}

export function SpecTimelineTab({
  nodes,
  onNodeClick,
  repoPath,
  spec,
  waves,
}: SpecTimelineTabProps) {
  const groups = useMemo(() => groupByPhase(nodes), [nodes]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [viewer, setViewer] = useState<
    { kind: SpecMarkdownKind; wave?: number } | null
  >(null);

  if (nodes.length === 0) {
    return (
      <p className="text-[13px] text-muted-foreground py-4 text-center">
        Nenhum evento de timeline para esta spec.
      </p>
    );
  }

  function togglePhase(phase: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(phase)) next.delete(phase);
      else next.add(phase);
      return next;
    });
  }

  return (
    <div className="flex flex-col gap-3">
      {groups.map(({ phase, nodes: group }) => {
        const isCollapsed = collapsed.has(phase);
        return (
          <section key={phase} className="flex flex-col gap-1">
            <button
              type="button"
              onClick={() => togglePhase(phase)}
              aria-expanded={!isCollapsed}
              className={cn(
                "flex items-center gap-1 text-[11px] uppercase tracking-wide font-medium",
                "text-muted-foreground hover:text-foreground transition-colors",
                "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary] rounded-sm",
              )}
            >
              <ChevronRight
                className={cn(
                  "h-3 w-3 transition-transform",
                  isCollapsed ? "rotate-0" : "rotate-90",
                )}
                aria-hidden
              />
              <span>{phaseLabel(phase)}</span>
              <span
                className="text-muted-foreground/60 tabular-nums ml-1"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                {group.length}
              </span>
            </button>

            {!isCollapsed && (
              <ol className="relative flex flex-col gap-0 pl-4 border-l border-border/40 ml-1">
                {group.map((node, i) => {
                  const target = markdownTargetFor(node);
                  const canOpenMd = !!target && !!repoPath && !!spec;
                  return (
                    <li key={`${node.ts}-${i}`} className="relative">
                      {/* Connector dot */}
                      <span
                        className={cn(
                          "absolute -left-[5px] top-2.5 w-2 h-2 rounded-full border bg-background",
                          node.kind === "phase"
                            ? "border-[--primary]"
                            : "border-border",
                        )}
                        aria-hidden
                      />

                      <div className="flex items-start gap-2 pl-3 py-2 group">
                        <button
                          type="button"
                          onClick={() => onNodeClick?.(node)}
                          className={cn(
                            "flex-1 text-left flex flex-col gap-0.5 rounded-r-md min-w-0",
                            "hover:bg-muted/30 focus-visible:outline-none focus-visible:ring-2",
                            "focus-visible:ring-[--primary] transition-colors",
                            onNodeClick ? "cursor-pointer" : "cursor-default",
                          )}
                          aria-label={`${node.label}${node.phase ? `, fase ${node.phase}` : ""}${node.wave != null ? `, onda ${node.wave}` : ""}, ${relativeTime(node.ts)}`}
                        >
                          <div className="flex items-center gap-2 flex-wrap">
                            <span
                              className={cn(
                                "text-[10px] uppercase tracking-wide font-medium shrink-0 px-1.5 py-0.5 rounded",
                                KIND_BADGE_CLS[node.kind] ?? KIND_BADGE_CLS.other,
                              )}
                            >
                              {KIND_LABEL[node.kind] ?? node.kind}
                            </span>
                            {node.wave != null && (
                              <span
                                className={cn(
                                  "text-[10px] tabular-nums shrink-0",
                                  KIND_ACCENT.wave,
                                )}
                                style={{ fontVariantNumeric: "tabular-nums" }}
                              >
                                #{node.wave}
                              </span>
                            )}
                            <span className="text-[11px] text-muted-foreground/50 ml-auto shrink-0">
                              {relativeTime(node.ts)}
                            </span>
                          </div>
                          <p className="text-[12px] text-foreground/80 leading-snug">
                            {node.label}
                          </p>
                        </button>

                        {canOpenMd && (
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              setViewer(target);
                            }}
                            aria-label="Ver markdown"
                            title="Ver markdown"
                            className={cn(
                              "shrink-0 h-6 w-6 mt-0.5 flex items-center justify-center rounded",
                              "text-muted-foreground/60 hover:text-foreground hover:bg-muted/60",
                              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
                              "transition-colors opacity-0 group-hover:opacity-100 focus-visible:opacity-100",
                            )}
                          >
                            <FileText className="h-3.5 w-3.5" aria-hidden />
                          </button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ol>
            )}
          </section>
        );
      })}

      {viewer && repoPath && spec && (
        <SpecMarkdownViewer
          open={!!viewer}
          onOpenChange={(o) => {
            if (!o) setViewer(null);
          }}
          repoPath={repoPath}
          spec={spec}
          waves={waves}
          initialKind={viewer.kind}
          initialWave={viewer.wave}
        />
      )}
    </div>
  );
}
