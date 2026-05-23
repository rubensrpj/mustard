import { useEffect, useRef } from "react";
import {
  Search,
  ClipboardList,
  Zap,
  CheckSquare,
  Archive,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

export type PhaseStationState = "future" | "active" | "completed";

/**
 * Wave 4 (spec `2026-05-21-dashboard-spec-tabs-polish`): per-phase color
 * bag injected by the parent `<PipelineTimeline>`. Each phase now reads as
 * its own hue (analyze=sky, plan=violet, execute=mustard, qa=emerald,
 * close=slate) — see `@/lib/phase-palette`. When the field is omitted we
 * fall back to the legacy mustard-only treatment so any external caller
 * still renders something sane.
 */
export interface PhaseStationColors {
  bg: string;
  text: string;
  border: string;
  ring: string;
}

export interface PhaseStationProps {
  phase: "analyze" | "plan" | "execute" | "qa" | "close";
  state: PhaseStationState;
  eventsCount?: number;
  durationMs?: number;
  /** Optional per-phase color override (Wave 4). */
  colors?: PhaseStationColors;
  className?: string;
}

// Tactical-fix `2026-05-21-tf-speccard-polish`: labels were hard-coded EN;
// they now resolve via `useT()` against the shared `phase.*` keys (already
// populated by Wave 2 of `2026-05-21-dashboard-i18n-and-phase-unify`).
const PHASE_META: Record<
  PhaseStationProps["phase"],
  { i18nKey: string; Icon: React.ComponentType<{ className?: string }> }
> = {
  analyze:  { i18nKey: "phase.analyze",  Icon: Search },
  plan:     { i18nKey: "phase.plan",     Icon: ClipboardList },
  execute:  { i18nKey: "phase.execute",  Icon: Zap },
  qa:       { i18nKey: "phase.qa",       Icon: CheckSquare },
  close:    { i18nKey: "phase.close",    Icon: Archive },
};

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return rem > 0 ? `${m}m ${rem}s` : `${m}m`;
}

export function PhaseStation({
  phase,
  state,
  eventsCount,
  durationMs,
  colors,
  className,
}: PhaseStationProps) {
  const t = useT();
  const { i18nKey, Icon } = PHASE_META[phase];
  const label = t(i18nKey);
  const dotRef = useRef<HTMLDivElement>(null);
  // Tactical-fix `2026-05-21-tf-speccard-polish`: single-size component.
  // The previous size-mode branching was removed — both call sites (SpecCard
  // list + SpecDetailDashboard header) render identical now.
  const circleSize = "h-8 w-8";
  const iconSize = "h-4 w-4";
  const labelSize = "text-[12px] font-medium";
  const activeRing = "ring-2";
  const minWidth = "min-w-[52px]";

  // wave-glow fires once when state becomes active
  useEffect(() => {
    if (state === "active" && dotRef.current) {
      dotRef.current.classList.remove("animate-wave-glow");
      // force reflow to restart animation
      void dotRef.current.offsetWidth;
      dotRef.current.classList.add("animate-wave-glow");
    }
  }, [state]);

  const ariaLabel = `${label}${eventsCount != null ? `, ${eventsCount} eventos` : ""}${durationMs != null ? `, ${Math.round(durationMs / 1000)}s` : ""} — ${state}`;

  return (
    <div
      tabIndex={0}
      role="img"
      aria-label={ariaLabel}
      className={cn(
        "flex flex-col items-center gap-1",
        minWidth,
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--color-accent-mustard] focus-visible:rounded-sm",
        className,
      )}
      title={`${label}${eventsCount != null ? ` — ${eventsCount} eventos` : ""}`}
    >
      {/* glyph circle — per-phase coloring (Wave 4). When `colors` is omitted
          we keep the legacy mustard-only treatment for back-compat. Active
          phase gets a `motion-safe:animate-pulse` plus a colored ring so it
          stands out at a glance; `motion-safe:` honors `prefers-reduced-motion`. */}
      <div
        ref={dotRef}
        className={cn(
          circleSize,
          "rounded-full flex items-center justify-center border transition-colors",
          colors
            ? cn(
                state === "future" && cn("border-dashed opacity-40", colors.border, colors.text),
                state === "active" &&
                  cn(
                    colors.border,
                    colors.text,
                    colors.bg,
                    activeRing,
                    colors.ring,
                    "motion-safe:animate-pulse",
                  ),
                state === "completed" && cn("border-transparent text-foreground", colors.bg),
              )
            : cn(
                state === "future" && "border-border text-muted-foreground/50 bg-transparent",
                state === "active" &&
                  cn(
                    "border-[--color-accent-mustard] text-[--color-accent-mustard] bg-[--color-accent-mustard]/10 motion-safe:animate-pulse",
                    activeRing,
                  ),
                state === "completed" && "border-transparent text-foreground bg-muted",
              ),
        )}
      >
        <Icon className={iconSize} />
      </div>

      {/* label */}
      <span
        className={cn(
          labelSize,
          "leading-none",
          colors
            ? cn(
                state === "future" && "text-muted-foreground/50",
                state === "active" && colors.text,
                state === "completed" && "text-foreground",
              )
            : cn(
                state === "future" && "text-muted-foreground/50",
                state === "active" && "text-[--color-accent-mustard]",
                state === "completed" && "text-foreground",
              ),
        )}
      >
        {label}
      </span>

      {/* duration + event count */}
      {(durationMs != null || eventsCount != null) && (
        <span
          className="text-[10px] text-muted-foreground tabular-nums leading-none"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {durationMs != null && formatDuration(durationMs)}
          {durationMs != null && eventsCount != null && " · "}
          {eventsCount != null && `${eventsCount}`}
        </span>
      )}
    </div>
  );
}
