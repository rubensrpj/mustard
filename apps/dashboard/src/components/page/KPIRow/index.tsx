// Grid wrapper for a row of KPICards. Defines the gap and responsive
// column count so every page renders KPI ribbons identically (1 col on
// narrow widths, 2 cols on small screens, then `cols` columns from md
// upwards). Callers just drop KPICards in as children — no manual grid
// wiring per page.

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface KPIRowProps {
  children: ReactNode;
  /** Column count from the md breakpoint upwards. Default 4. */
  cols?: 2 | 3 | 4 | 5 | 6;
  className?: string;
}

const COLS: Record<NonNullable<KPIRowProps["cols"]>, string> = {
  2: "md:grid-cols-2",
  3: "md:grid-cols-3",
  4: "md:grid-cols-4",
  5: "md:grid-cols-5",
  6: "md:grid-cols-6",
};

export function KPIRow({ children, cols = 4, className }: KPIRowProps) {
  return (
    <div
      className={cn(
        "grid grid-cols-1 sm:grid-cols-2 gap-3",
        COLS[cols],
        className,
      )}
    >
      {children}
    </div>
  );
}
