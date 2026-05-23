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
      <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[--primary] opacity-75" />
      <span className="relative inline-flex rounded-full h-2 w-2 bg-[--primary]" />
    </span>
  );
}

/**
 * Wave 8 (2026-05-21, spec
 * `2026-05-20-economia-moat-unification/wave-8-visao-geral-revamp`): the
 * token-savings hero block was removed from this status bar so the Visão
 * Geral hero stops competing with the dedicated savings card. Token savings
 * now live in `<WorkspaceTokenSummary>` (and the `/economia` page from Wave
 * 7). This component is kept as a thin live-rate strip in case a future page
 * wants the same surface; the Wave-8 layout (`Workspace.tsx`) replaced the
 * StatusBar+PipelineTimeline pair with `<WorkspaceHero>` and no longer mounts
 * this component on the Overview page.
 */
export function WorkspaceStatusBar({ summary, className }: WorkspaceStatusBarProps) {
  const epm = summary?.events_per_minute ?? 0;
  const active = summary?.specs_active_count ?? 0;

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
          aria-label={`${epm.toFixed(1)} events per minute`}
        >
          <span className="font-medium">{epm.toFixed(1)}</span>
          <span className="text-muted-foreground text-[12px]"> events/min</span>
        </span>
      </div>

      {/* Active specs */}
      <div className="flex items-center gap-1.5 min-w-0">
        <span
          className="text-sm tabular-nums"
          style={{ fontVariantNumeric: "tabular-nums" }}
          aria-label={`${active} active specs`}
        >
          <span className="font-medium">{active}</span>
          <span className="text-muted-foreground text-[12px]"> active specs</span>
        </span>
      </div>
    </div>
  );
}
