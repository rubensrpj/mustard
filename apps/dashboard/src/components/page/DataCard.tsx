import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Wrapper for tabular/list data. Provides consistent border, background,
 * and rounded corners so tables across pages look like they belong to the
 * same product. Use as parent for `<table>` or `<ul>` content.
 *
 * Set `padded` when the children don't have their own internal padding
 * (e.g. a list with no rows yet, or free-form content). For tables, leave
 * padded=false — the table cells provide their own padding.
 */
export interface DataCardProps {
  children: ReactNode;
  /** Add internal padding (default false — tables handle their own). */
  padded?: boolean;
  className?: string;
}

export function DataCard({ children, padded = false, className }: DataCardProps) {
  return (
    <div
      className={cn(
        "border border-border rounded-lg overflow-hidden bg-card/20 w-full",
        padded && "p-4",
        className,
      )}
    >
      {children}
    </div>
  );
}
