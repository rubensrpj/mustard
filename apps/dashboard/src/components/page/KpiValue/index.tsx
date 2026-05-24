// Numeric primitive used inside KPICards (or anywhere a big monospace
// number with an uppercase label/hint stack is needed). Splits the trio
// (value/label/hint) into separate slot components so callers can compose
// freely — e.g. an inline KpiValue + KpiLabel side-by-side, or stacked
// with a KpiHint underneath. All numerics use `font-mono tabular-nums` so
// columns of digits stay aligned across rows.

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface KpiValueProps {
  children: ReactNode;
  className?: string;
}

export function KpiValue({ children, className }: KpiValueProps) {
  return (
    <div
      className={cn(
        "text-2xl font-mono font-medium tabular-nums leading-tight text-foreground",
        className,
      )}
    >
      {children}
    </div>
  );
}

export interface KpiLabelProps {
  children: ReactNode;
  className?: string;
}

export function KpiLabel({ children, className }: KpiLabelProps) {
  return (
    <div
      className={cn(
        "text-[10px] uppercase tracking-wider text-muted-foreground",
        className,
      )}
    >
      {children}
    </div>
  );
}

export interface KpiHintProps {
  children: ReactNode;
  className?: string;
}

export function KpiHint({ children, className }: KpiHintProps) {
  return (
    <div className={cn("text-[11.5px] text-muted-foreground", className)}>
      {children}
    </div>
  );
}
