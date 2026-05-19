import { phaseTheme } from "@/lib/phaseTheme";
import { cn } from "@/lib/utils";

/**
 * Color-coded chip representing a Mustard pipeline phase
 * (ANALYZE / PLAN / EXECUTE / QA / CLOSE). Hue is drawn from the shared
 * `phaseTheme` so every page reads the same phase as the same color.
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
  const t = phaseTheme(phase);
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-sm font-medium border tabular-nums",
        size === "sm" ? "px-1.5 py-0 text-[10px]" : "px-2 py-0.5 text-[11px]",
        t.text,
        t.bg,
        t.border,
        className,
      )}
      title={t.detail}
    >
      {phase}
    </span>
  );
}
