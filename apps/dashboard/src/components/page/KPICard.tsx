import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type KPIAccent = "emerald" | "amber" | "rose" | "indigo" | "violet" | "sky" | "zinc";

const ACCENT_STRIPE: Record<KPIAccent, string> = {
  emerald: "bg-emerald-500/40",
  amber: "bg-amber-500/40",
  rose: "bg-rose-500/40",
  indigo: "bg-primary/40",
  violet: "bg-violet-500/40",
  sky: "bg-sky-500/40",
  zinc: "bg-zinc-500/40",
};

const ACCENT_VALUE: Record<KPIAccent, string> = {
  emerald: "text-emerald-400",
  amber: "text-amber-400",
  rose: "text-rose-400",
  indigo: "text-primary",
  violet: "text-violet-400",
  sky: "text-sky-400",
  zinc: "text-foreground",
};

/**
 * Small stat card with a colored top stripe, big value, label, and hint.
 * Used in KPI ribbons across pages. Accent stripe ties the card to a
 * semantic color (emerald for good metrics, amber for caution, etc.).
 */
export interface KPICardProps {
  label: string;
  value: ReactNode;
  hint?: string;
  /** Color of the top accent stripe and the value text. */
  accent?: KPIAccent;
  /** Hover tooltip. */
  tooltip?: string;
  /** Override the value color (when the value itself encodes the state). */
  valueClassName?: string;
  className?: string;
}

export function KPICard({
  label,
  value,
  hint,
  accent = "zinc",
  tooltip,
  valueClassName,
  className,
}: KPICardProps) {
  return (
    <div
      className={cn(
        "border border-border rounded-lg p-4 flex flex-col gap-1 bg-card/30 relative overflow-hidden",
        className,
      )}
      title={tooltip}
    >
      <div className={cn("absolute top-0 left-0 right-0 h-0.5", ACCENT_STRIPE[accent])} />
      <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      <div
        className={cn(
          "text-2xl font-mono font-medium tabular-nums leading-tight",
          valueClassName ?? ACCENT_VALUE[accent],
        )}
      >
        {value}
      </div>
      {hint && (
        <div className="text-[11.5px] text-muted-foreground">{hint}</div>
      )}
    </div>
  );
}
