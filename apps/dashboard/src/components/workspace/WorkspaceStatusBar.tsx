import { cn } from "@/lib/utils";
import type { WorkspaceSummary } from "@/lib/types/specs";

interface WorkspaceStatusBarProps {
  summary: WorkspaceSummary | undefined;
  className?: string;
}

/** Animated pulse dot — signals live activity. */
function LiveDot() {
  return (
    <span className="relative flex h-2 w-2 shrink-0" aria-hidden>
      <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[--color-accent-mustard] opacity-75" />
      <span className="relative inline-flex rounded-full h-2 w-2 bg-[--color-accent-mustard]" />
    </span>
  );
}

export function WorkspaceStatusBar({ summary, className }: WorkspaceStatusBarProps) {
  const epm = summary?.events_per_minute ?? 0;
  const active = summary?.specs_active_count ?? 0;
  const saved = summary?.tokens_saved_today ?? 0;

  const formattedSaved =
    saved >= 1_000_000
      ? `${(saved / 1_000_000).toFixed(1)}M`
      : saved >= 1_000
        ? `${(saved / 1_000).toFixed(1)}k`
        : String(saved);

  return (
    <div
      className={cn(
        "flex items-center gap-6 flex-wrap px-4 py-2.5 rounded-lg",
        "border border-border bg-card/30",
        className,
      )}
    >
      {/* Live events/min pulse */}
      <div className="flex items-center gap-2 min-w-0">
        <LiveDot />
        <span
          className="text-sm text-foreground/80 tabular-nums"
          style={{ fontVariantNumeric: "tabular-nums" }}
          aria-label={`${epm.toFixed(1)} eventos por minuto`}
        >
          <span className="font-medium">{epm.toFixed(1)}</span>
          <span className="text-muted-foreground text-[12px]"> eventos/min</span>
        </span>
      </div>

      {/* Active specs */}
      <div className="flex items-center gap-1.5 min-w-0">
        <span
          className="text-sm tabular-nums"
          style={{ fontVariantNumeric: "tabular-nums" }}
          aria-label={`${active} specs ativas`}
        >
          <span className="font-medium">{active}</span>
          <span className="text-muted-foreground text-[12px]"> specs ativas</span>
        </span>
      </div>

      {/* Hero: tokens saved today */}
      <div className="flex items-center gap-1.5 ml-auto min-w-0">
        <span className="text-[11px] text-muted-foreground uppercase tracking-wide">
          economizados hoje
        </span>
        <span
          className="text-lg font-bold text-[--color-accent-mustard] tabular-nums"
          style={{ fontVariantNumeric: "tabular-nums" }}
          aria-label={`${formattedSaved} tokens economizados hoje`}
        >
          {formattedSaved}
        </span>
        <span className="text-[11px] text-muted-foreground">tokens</span>
      </div>
    </div>
  );
}
