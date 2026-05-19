import { Fragment, type ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Top-of-page header used across all routes. Combines breadcrumb, title, an
 * optional appended subtitle (separated by em-dash), and a paragraph
 * description. Right slot can host a live indicator, refresh button, or any
 * other secondary action.
 *
 * Width: always full-width (no max-w constraints) — the consumer is
 * responsible for letting the page take the available space. Description text
 * uses `leading-relaxed` so wide lines stay legible without artificial caps.
 */
export interface PageHeaderProps {
  /** Breadcrumb segments rendered with `/` separators. Last is bolded. */
  breadcrumb: ReadonlyArray<string | { label: string; mono?: boolean }>;
  /** Main page title (h1). */
  title: string;
  /** Optional subtitle appended after the title with an em-dash separator. */
  subtitle?: string;
  /** Paragraph description shown under the title. Optional. */
  description?: ReactNode;
  /** Right-aligned slot for secondary actions or status indicators. */
  right?: ReactNode;
  className?: string;
}

export function PageHeader({
  breadcrumb,
  title,
  subtitle,
  description,
  right,
  className,
}: PageHeaderProps) {
  return (
    <header className={cn("flex flex-col gap-1.5 w-full", className)}>
      <div className="flex items-start justify-between gap-3 flex-wrap">
        <nav className="text-[12px] text-muted-foreground flex items-center gap-1.5 flex-wrap">
          {breadcrumb.map((segment, i) => {
            const label = typeof segment === "string" ? segment : segment.label;
            const mono = typeof segment === "object" && segment.mono;
            const isLast = i === breadcrumb.length - 1;
            return (
              <Fragment key={i}>
                {i > 0 && <span className="opacity-50">/</span>}
                <span
                  className={cn(
                    mono && "font-mono",
                    isLast ? "text-foreground" : "text-muted-foreground",
                  )}
                >
                  {label}
                </span>
              </Fragment>
            );
          })}
        </nav>
        {right && <div className="shrink-0">{right}</div>}
      </div>
      <h1 className="text-xl font-medium tracking-tight">
        {title}
        {subtitle && (
          <span className="text-muted-foreground font-normal"> — {subtitle}</span>
        )}
      </h1>
      {description && (
        <p className="text-[13px] text-muted-foreground leading-relaxed">
          {description}
        </p>
      )}
    </header>
  );
}
