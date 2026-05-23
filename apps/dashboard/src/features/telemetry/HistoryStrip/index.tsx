import { cn } from "@/lib/utils";
import type { HistoryEntry } from "@/lib/types/telemetry";

export interface HistoryStripProps {
  entries: HistoryEntry[];
  onSelect?: (spec: string) => void;
  className?: string;
}

function formatDuration(entry: HistoryEntry): string {
  if (!entry.started_at) return "—";
  const end = entry.completed_at ?? entry.started_at;
  const ms = Date.parse(end) - Date.parse(entry.started_at);
  if (!Number.isFinite(ms) || ms < 0) return "—";
  const totalMin = Math.floor(ms / 60_000);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  if (h === 0) return `${m}m`;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

function scopeFromEntry(entry: HistoryEntry): "full" | "light" | "touch" {
  const phases = Object.keys(entry.duration_per_phase ?? {}).map((p) =>
    p.toLowerCase(),
  );
  if (phases.includes("plan")) return "full";
  if (phases.includes("execute")) return "light";
  return "touch";
}

const SCOPE_LABEL: Record<"full" | "light" | "touch", string> = {
  full: "full",
  light: "light",
  touch: "touch",
};

function shortSpec(name: string): string {
  return name.replace(/^\d{4}-\d{2}-\d{2}-/, "");
}

export function HistoryStrip({ entries, onSelect, className }: HistoryStripProps) {
  if (entries.length === 0) {
    return (
      <p className={cn("text-[12px] text-muted-foreground/60 py-2", className)}>
        Nenhuma pipeline no período
      </p>
    );
  }

  return (
    <div
      className={cn(
        "flex gap-2 overflow-x-auto pb-1",
        "scrollbar-none [scrollbar-width:none] [-ms-overflow-style:none]",
        className,
      )}
    >
      {entries.map((e) => {
        const scope = scopeFromEntry(e);
        const dur = formatDuration(e);
        const acLabel = e.ac_total > 0 ? `${e.ac_passed}/${e.ac_total}` : "—";

        return (
          <button
            key={e.spec}
            type="button"
            onClick={() => onSelect?.(e.spec)}
            className={cn(
              "flex-shrink-0 flex flex-col gap-1 rounded-lg border border-border bg-card/40",
              "px-3 py-2 text-left transition-colors hover:bg-muted/60 min-w-[140px] max-w-[180px]",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
            )}
          >
            <span
              className="text-[12px] font-medium text-foreground truncate w-full"
              title={e.spec}
            >
              {shortSpec(e.spec)}
            </span>

            <div className="flex items-center gap-1.5 flex-wrap">
              {/* scope chip */}
              <span className="text-[10px] text-muted-foreground border border-border rounded px-1 py-px">
                {SCOPE_LABEL[scope]}
              </span>
              {/* duration */}
              <span
                className="text-[11px] text-muted-foreground"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                {dur}
              </span>
              {/* AC */}
              <span
                className="text-[11px] ml-auto"
                style={{ fontVariantNumeric: "tabular-nums" }}
                title={`AC: ${acLabel}`}
              >
                {acLabel}
              </span>
            </div>
          </button>
        );
      })}
    </div>
  );
}
