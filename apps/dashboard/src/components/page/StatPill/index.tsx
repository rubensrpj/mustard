// Compact monospace pill for a single metric (e.g., "1.2k tok", "230 ms").
// `intent` colors the border only — the fill stays at --card
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
  // TF remap: --ds-surface-hover → --accent; hover surface = Binance accent swatch
  neutral: "border-[--accent]",
  // TF remap: --ds-intent-* → --intent-*; intent tokens renamed in Binance pack
  success: "border-[--intent-success]/40",
  warning: "border-[--intent-warning]/40",
  error:   "border-[--intent-error]/40",
  info:    "border-[--intent-info]/40",
};

const TEXT: Record<Intent, string> = {
  // TF remap: --ds-text-secondary → --muted-foreground; Binance #848e9c subdued text
  neutral: "text-[--muted-foreground]",
  // TF remap: --ds-intent-* → --intent-*
  success: "text-[--intent-success]",
  warning: "text-[--intent-warning]",
  error:   "text-[--intent-error]",
  info:    "text-[--intent-info]",
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
        // TF remap: --ds-surface-elevated → --card; card surface in Binance DESIGN.md
        "bg-[--card] font-mono text-[11px] leading-none",
        BORDER[intent],
        TEXT[intent],
        className,
      )}
    >
      <span className="tabular-nums">{value}</span>
      {/* TF remap: --ds-text-tertiary → --muted-foreground; no tertiary tier in Binance, maps to subdued */}
      {unit ? <span className="text-[--muted-foreground]">{unit}</span> : null}
    </span>
  );
}
