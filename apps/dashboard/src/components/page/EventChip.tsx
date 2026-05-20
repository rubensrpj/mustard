import { eventTheme } from "@/lib/phaseTheme";
import { cn } from "@/lib/utils";

/**
 * Color-coded chip for an event-type label (tool.use, agent.start, qa.result,
 * etc.). Different hue family from PhaseChip so the two categories of label
 * never blur visually.
 *
 * When `overall` is provided (only meaningful for qa.result), the chip
 * morphs into a pass/fail/skip badge instead of the generic "qa" label.
 */
export interface EventChipProps {
  eventType: string;
  /** For qa.result events: pass/fail/skip — overrides the chip color. */
  overall?: "pass" | "fail" | "skip" | null;
  size?: "default" | "sm";
  className?: string;
}

export function EventChip({ eventType, overall, size = "default", className }: EventChipProps) {
  const t = eventTheme(eventType);
  const label = t.label;

  // qa.result with explicit verdict — show that verdict instead of "qa"
  let displayLabel = label;
  let text = t.text;
  let bg = t.bg;
  let border = t.border;
  let title = t.detail;
  if (eventType === "qa.result" && overall) {
    displayLabel = overall === "pass" ? "qa ✓" : overall === "fail" ? "qa ✗" : "qa ⊘";
    text = overall === "pass" ? "text-[--color-ok]" : overall === "fail" ? "text-[--color-error]" : "text-[--color-accent-mustard]";
    bg = overall === "pass" ? "bg-[--color-ok]/10" : overall === "fail" ? "bg-[--color-error]/10" : "bg-[--color-accent-mustard]/10";
    border = overall === "pass" ? "border-[--color-ok]/25" : overall === "fail" ? "border-[--color-error]/25" : "border-[--color-accent-mustard]/25";
    title = `QA overall: ${overall}`;
  }

  return (
    <span
      className={cn(
        "inline-flex items-center rounded-sm font-mono border",
        size === "sm" ? "px-1.5 py-0 text-[10px]" : "px-2 py-0.5 text-[11px]",
        text,
        bg,
        border,
        className,
      )}
      title={title}
    >
      {displayLabel}
    </span>
  );
}
