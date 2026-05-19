import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type EmptyVariant = "info" | "warning" | "error";

const VARIANT_STYLES: Record<EmptyVariant, string> = {
  info: "border-border bg-card/30",
  warning: "border-amber-500/30 bg-amber-500/5",
  error: "border-destructive/40 bg-destructive/5",
};

const VARIANT_TITLE: Record<EmptyVariant, string> = {
  info: "text-foreground",
  warning: "text-amber-300",
  error: "text-destructive",
};

/**
 * Empty/info/error state card. Three variants:
 *   info     — neutral message ("nothing to show yet")
 *   warning  — amber ring, things-need-attention
 *   error    — destructive ring, something broke
 *
 * Right slot can host a retry button or action.
 */
export interface EmptyStateProps {
  title: string;
  description?: ReactNode;
  variant?: EmptyVariant;
  /** Right-aligned action (e.g. retry button). */
  right?: ReactNode;
  className?: string;
}

export function EmptyState({
  title,
  description,
  variant = "info",
  right,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        "border rounded-lg p-4 flex items-start gap-3 w-full",
        VARIANT_STYLES[variant],
        className,
      )}
    >
      <div className="flex-1 flex flex-col gap-1">
        <p className={cn("text-sm font-medium", VARIANT_TITLE[variant])}>{title}</p>
        {description && (
          <div className="text-[13px] text-muted-foreground leading-relaxed">
            {description}
          </div>
        )}
      </div>
      {right && <div className="shrink-0">{right}</div>}
    </div>
  );
}
