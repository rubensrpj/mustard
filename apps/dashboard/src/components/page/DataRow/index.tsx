// List row primitive for use inside a DataCard. Splits the row into four
// named slots — `lead` (icon/avatar/dot, shrink-0), `primary` (the main
// text, takes remaining space and truncates), `meta` (secondary text,
// shrinks but doesn't truncate aggressively), and `trailing` (actions or
// numerics, shrink-0). Stateless — interaction (click, hover) is the
// caller's job via the optional `onClick` prop.

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface DataRowProps {
  lead?: ReactNode;
  primary?: ReactNode;
  meta?: ReactNode;
  trailing?: ReactNode;
  onClick?: () => void;
  className?: string;
}

export function DataRow({
  lead,
  primary,
  meta,
  trailing,
  onClick,
  className,
}: DataRowProps) {
  const interactive = typeof onClick === "function";
  return (
    <div
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onClick={onClick}
      onKeyDown={
        interactive
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick?.();
              }
            }
          : undefined
      }
      className={cn(
        "flex items-center gap-3 px-3 py-2 border-b border-border last:border-b-0",
        interactive &&
          "cursor-pointer hover:bg-card focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/60",
        className,
      )}
    >
      {lead ? <span className="shrink-0 text-muted-foreground">{lead}</span> : null}
      {primary ? (
        <div className="min-w-0 flex-1 text-[13px] text-foreground truncate">
          {primary}
        </div>
      ) : null}
      {meta ? (
        <div className="shrink-0 text-[12px] text-muted-foreground">{meta}</div>
      ) : null}
      {trailing ? <div className="shrink-0">{trailing}</div> : null}
    </div>
  );
}
