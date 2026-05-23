import { cn } from "@/lib/utils";
import type { AgentDispatch } from "@/lib/types/telemetry";

export interface AgentRosterProps {
  agents: AgentDispatch[];
  /** Max rows to display. Default: 8 */
  topN?: number;
  className?: string;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)}s`;
  return `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`;
}

function relativeTime(ts: string | null): string {
  if (!ts) return "";
  const ms = Date.now() - Date.parse(ts);
  if (!Number.isFinite(ms)) return "";
  const m = Math.floor(ms / 60_000);
  if (m < 1) return "agora";
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

export function AgentRoster({ agents, topN = 8, className }: AgentRosterProps) {
  const rows = agents.slice(0, topN);

  if (rows.length === 0) {
    return (
      <p className={cn("text-[12px] text-muted-foreground/60 py-2", className)}>
        Nenhum agente no período
      </p>
    );
  }

  return (
    <div className={cn("flex flex-col gap-0 w-[280px]", className)}>
      {/* header */}
      <div className="grid grid-cols-[1fr_auto_auto_auto] gap-2 px-2 pb-1 border-b border-border">
        <span className="text-[10px] text-muted-foreground/60">tipo</span>
        <span className="text-[10px] text-muted-foreground/60 text-right w-10">desp.</span>
        <span className="text-[10px] text-muted-foreground/60 text-right w-12">duração</span>
        <span className="text-[10px] text-muted-foreground/60 text-right w-8">último</span>
      </div>

      {rows.map((a) => (
        <div
          key={a.subagent_type}
          className="grid grid-cols-[1fr_auto_auto_auto] gap-2 px-2 py-1.5 hover:bg-muted/40 rounded transition-colors"
        >
          {/* subagent type */}
          <span className="text-[12px] text-foreground truncate" title={a.subagent_type}>
            {a.subagent_type}
          </span>

          {/* dispatches */}
          <span
            className="text-[12px] text-muted-foreground text-right w-10 tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {a.count}
          </span>

          {/* avg duration */}
          <span
            className="text-[12px] text-muted-foreground text-right w-12 tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {formatDuration(a.avg_duration_ms)}
          </span>

          {/* last dispatched */}
          <span
            className="text-[11px] text-muted-foreground/60 text-right w-8"
          >
            {relativeTime(a.last_dispatched_at)}
          </span>
        </div>
      ))}

      {/* error summary row — only when any agent has errors */}
      {rows.some((a) => a.error_count > 0) && (
        <div className="mt-1 pt-1 border-t border-border px-2 flex flex-col gap-0.5">
          {rows
            .filter((a) => a.error_count > 0)
            .map((a) => (
              <div key={`err-${a.subagent_type}`} className="flex items-center gap-1.5">
                <span
                  className="inline-block w-1.5 h-1.5 rounded-full bg-[--color-error] flex-shrink-0"
                  aria-hidden="true"
                />
                <span className="text-[11px] text-[--color-error] tabular-nums">
                  {a.subagent_type} · {a.error_count} erro{a.error_count !== 1 ? "s" : ""}
                </span>
              </div>
            ))}
        </div>
      )}
    </div>
  );
}
