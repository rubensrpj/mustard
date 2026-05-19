import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Mid-page section divider. Use for delimiting major sections within a page
 * (KPI ribbon, data tables, breakdowns). Smaller than PageHeader, larger
 * than a plain label.
 */
export interface SectionHeaderProps {
  /** Section title — rendered uppercase + tracked, label-style. */
  title: string;
  /** Optional descriptive paragraph below the title. */
  description?: ReactNode;
  /** Right-aligned slot — typically a count, filter, or "ver mais" link. */
  right?: ReactNode;
  className?: string;
}

export function SectionHeader({
  title,
  description,
  right,
  className,
}: SectionHeaderProps) {
  return (
    <header className={cn("flex flex-col gap-1 w-full", className)}>
      <div className="flex items-baseline justify-between gap-3 flex-wrap">
        <h2 className="text-[11px] uppercase tracking-[0.08em] font-medium text-muted-foreground">
          {title}
        </h2>
        {right && <div className="text-[11px] text-muted-foreground/70">{right}</div>}
      </div>
      {description && (
        <p className="text-[12.5px] text-muted-foreground/80 leading-relaxed">
          {description}
        </p>
      )}
    </header>
  );
}
