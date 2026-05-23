// Compact monospace pill for a single metric (e.g., "1.2k tok", "230 ms").
// `intent` colors the border only — the fill stays at --ds-surface-elevated
// so the pill is calm in dense lists. `tooltip` is rendered as a native
// title for now; the trace-viewer in W6 can swap to a richer floating panel.

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type Intent = "success" | "warning" | "error" | "info" | "neutral";

export interface StatPillProps {
  value: string | number;
  unit?: string;
  intent?: Intent;
  tooltip?: ReactNode;
  className?: string;
}

const BORDER: Record<Intent, string> = {
  neutral: "border-[--ds-surface-hover]",
  success: "border-[--ds-intent-success]/40",
  warning: "border-[--ds-intent-warning]/40",
  error:   "border-[--ds-intent-error]/40",
  info:    "border-[--ds-intent-info]/40",
};

const TEXT: Record<Intent, string> = {
  neutral: "text-[--ds-text-secondary]",
  success: "text-[--ds-intent-success]",
  warning: "text-[--ds-intent-warning]",
  error:   "text-[--ds-intent-error]",
  info:    "text-[--ds-intent-info]",
};

export function StatPill({
  value,
  unit,
  intent = "neutral",
  tooltip,
  className,
}: StatPillProps) {
  const title = typeof tooltip === "string" ? tooltip : undefined;
  return (
    <span
      title={title}
      className={cn(
        "inline-flex items-baseline gap-1 rounded-full border px-2 py-0.5",
        "bg-[--ds-surface-elevated] font-mono text-[11px] leading-none",
        BORDER[intent],
        TEXT[intent],
        className,
      )}
    >
      <span className="tabular-nums">{value}</span>
      {unit ? <span className="text-[--ds-text-tertiary]">{unit}</span> : null}
    </span>
  );
}
