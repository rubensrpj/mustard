import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import type { SpecTimelineNode } from "@/lib/types/specs";

interface SpecTimelineTabProps {
  nodes: SpecTimelineNode[];
  /** When a node is clicked, emit it so SpecDrillDown can filter SpecEventsTab. */
  onNodeClick?: (node: SpecTimelineNode) => void;
}

const KIND_ACCENT: Record<string, string> = {
  phase:  "text-[--color-accent-mustard]",
  wave:   "text-foreground/80",
  qa:     "text-[--color-ok]",
  review: "text-muted-foreground",
  agent:  "text-muted-foreground",
  tool:   "text-muted-foreground/60",
  other:  "text-muted-foreground/40",
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

export function SpecTimelineTab({ nodes, onNodeClick }: SpecTimelineTabProps) {
  if (nodes.length === 0) {
    return (
      <p className="text-[13px] text-muted-foreground py-4 text-center">
        Nenhum evento de timeline para esta spec.
      </p>
    );
  }

  return (
    <ol className="relative flex flex-col gap-0 pl-4 border-l border-border/40">
      {nodes.map((node, i) => (
        <li key={`${node.ts}-${i}`} className="relative">
          {/* Connector dot */}
          <span
            className={cn(
              "absolute -left-[5px] top-2.5 w-2 h-2 rounded-full border bg-background",
              node.kind === "phase"
                ? "border-[--color-accent-mustard]"
                : "border-border",
            )}
            aria-hidden
          />

          <button
            type="button"
            onClick={() => onNodeClick?.(node)}
            className={cn(
              "w-full text-left flex flex-col gap-0.5 pl-3 py-2 rounded-r-md",
              "hover:bg-muted/30 focus-visible:outline-none focus-visible:ring-2",
              "focus-visible:ring-[--color-accent-mustard] transition-colors",
              onNodeClick ? "cursor-pointer" : "cursor-default",
            )}
            aria-label={`${node.label}${node.phase ? `, fase ${node.phase}` : ""}${node.wave != null ? `, onda ${node.wave}` : ""}, ${relativeTime(node.ts)}`}
          >
            <div className="flex items-center gap-2 flex-wrap">
              <span
                className={cn(
                  "text-[10px] uppercase tracking-wide font-medium shrink-0",
                  KIND_ACCENT[node.kind] ?? KIND_ACCENT.other,
                )}
              >
                {KIND_LABEL[node.kind] ?? node.kind}
              </span>
              {node.wave != null && (
                <span className="text-[10px] text-muted-foreground/60 tabular-nums shrink-0"
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
        </li>
      ))}
    </ol>
  );
}
