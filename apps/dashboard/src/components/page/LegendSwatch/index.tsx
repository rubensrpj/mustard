// Tiny color-swatch + label combo used in chart legends and anywhere a
// caller wants to attach a semantic color to a piece of text. Intents
// reference semantic CSS tokens (no raw Tailwind color classes) so the
// dashboard's theming stays consistent across legends, bars, deltas,
// etc.

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type LegendIntent =
  | "primary"
  | "success"
  | "error"
  | "warning"
  | "neutral";

const SWATCH: Record<LegendIntent, string> = {
  primary: "bg-primary",
  success: "bg-[--intent-success]",
  error: "bg-[--intent-error]",
  warning: "bg-[--intent-warning]",
  neutral: "bg-muted-foreground",
};

export interface LegendSwatchProps {
  intent: LegendIntent;
  label: ReactNode;
  className?: string;
}

export function LegendSwatch({ intent, label, className }: LegendSwatchProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 text-[12px] text-muted-foreground",
        className,
      )}
    >
      <span
        aria-hidden="true"
        className={cn("inline-block size-2 rounded-sm", SWATCH[intent])}
      />
      <span>{label}</span>
    </span>
  );
}
