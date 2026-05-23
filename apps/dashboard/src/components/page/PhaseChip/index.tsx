import { phaseColor } from "@/lib/phase-palette";
import { phaseTheme } from "@/lib/phaseTheme";
import { cn } from "@/lib/utils";

/**
 * Color-coded chip representing a Mustard pipeline phase
 * (ANALYZE / PLAN / EXECUTE / REVIEW / QA / CLOSE).
 *
 * Wave 4 (spec `2026-05-21-dashboard-spec-tabs-polish`): hue now comes from
 * the shared `phaseColor()` palette (the same one driving
 * `<PipelineTimeline>`), so every place that names a phase reads as the same
 * color. We still keep `phaseTheme()` around — only to source the
 * Portuguese tooltip text (`detail`) since the new palette is class-only.
 */
export interface PhaseChipProps {
  phase: string | null | undefined;
  size?: "default" | "sm";
  className?: string;
}

export function PhaseChip({ phase, size = "default", className }: PhaseChipProps) {
  if (!phase || phase === "—") {
    return (
      <span className={cn("inline-flex items-center text-[11px] text-muted-foreground/50", className)}>
        queued
      </span>
    );
  }
  const colors = phaseColor(phase);
  const tooltip = phaseTheme(phase).detail;
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-sm font-medium border tabular-nums",
        size === "sm" ? "px-1.5 py-0 text-[10px]" : "px-2 py-0.5 text-[11px]",
        colors.text,
        colors.bg,
        colors.border,
        className,
      )}
      title={tooltip}
    >
      {phase}
    </span>
  );
}
