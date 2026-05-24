// Horizontal bar used on the Economia page (and similar) to show a
// relative magnitude — cost per agent, tokens per spec, etc. Split into
// three pieces: `CostBar` is the labeled row (label + bar + numeric),
// `BarTrack` is the bar background, `BarFill` is the colored fill inside
// the track. Callers can compose them directly when the standard
// CostBar layout doesn't fit.

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type BarIntent = "primary" | "accent";

const BAR_INTENT: Record<BarIntent, string> = {
  primary: "bg-primary",
  accent: "bg-[--intent-success]",
};

export interface CostBarProps {
  label: ReactNode;
  /** Numeric in [0,1] — clamped before render. */
  value: number;
  /** Right-aligned numeric label (e.g. "1.2k tok", "$0.42"). */
  display?: ReactNode;
  intent?: BarIntent;
  className?: string;
}

export function CostBar({
  label,
  value,
  display,
  intent = "primary",
  className,
}: CostBarProps) {
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <div className="flex items-baseline justify-between gap-3">
        <div className="text-[12px] text-foreground truncate">{label}</div>
        {display !== undefined ? (
          <div className="font-mono tabular-nums text-[11px] text-muted-foreground shrink-0">
            {display}
          </div>
        ) : null}
      </div>
      <BarTrack>
        <BarFill value={value} intent={intent} />
      </BarTrack>
    </div>
  );
}

export interface BarTrackProps {
  children: ReactNode;
  className?: string;
}

export function BarTrack({ children, className }: BarTrackProps) {
  return (
    <div
      className={cn(
        "relative h-1.5 w-full overflow-hidden rounded-full bg-card border border-border",
        className,
      )}
    >
      {children}
    </div>
  );
}

export interface BarFillProps {
  /** Numeric in [0,1] — clamped before render. */
  value: number;
  intent?: BarIntent;
  className?: string;
}

export function BarFill({ value, intent = "primary", className }: BarFillProps) {
  const clamped = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
  const pct = `${(clamped * 100).toFixed(1)}%`;
  return (
    <div
      className={cn("h-full rounded-full", BAR_INTENT[intent], className)}
      style={{ width: pct }}
    />
  );
}
