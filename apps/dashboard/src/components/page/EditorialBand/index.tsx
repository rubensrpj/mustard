// Editorial hero band rendered at the top of major pages. Provides a
// consistent ~80px tall area for an uppercase eyebrow, a large title, a
// muted subtitle, and an optional actions slot on the trailing side. The
// component is purely compositional — callers choose which slots to fill,
// so the band can render anything from a single title to a full
// eyebrow+title+subtitle+actions cluster. Voltage typography follows
// DESIGN.md (uppercase tracking-wider eyebrow, 2xl/3xl title, muted
// subtitle).

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface EditorialBandProps {
  eyebrow?: ReactNode;
  title?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  className?: string;
}

export function EditorialBand({
  eyebrow,
  title,
  subtitle,
  actions,
  className,
}: EditorialBandProps) {
  return (
    <div
      className={cn(
        "flex items-start justify-between gap-6 min-h-20 py-4",
        className,
      )}
    >
      <div className="min-w-0 flex flex-col gap-1">
        {eyebrow ? <EditorialEyebrow>{eyebrow}</EditorialEyebrow> : null}
        {title ? <EditorialTitle>{title}</EditorialTitle> : null}
        {subtitle ? <EditorialSubtitle>{subtitle}</EditorialSubtitle> : null}
      </div>
      {actions ? (
        <div className="shrink-0 flex items-center gap-2">{actions}</div>
      ) : null}
    </div>
  );
}

export interface EditorialEyebrowProps {
  children: ReactNode;
  className?: string;
}

export function EditorialEyebrow({ children, className }: EditorialEyebrowProps) {
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

export interface EditorialTitleProps {
  children: ReactNode;
  className?: string;
}

export function EditorialTitle({ children, className }: EditorialTitleProps) {
  return (
    <h1
      className={cn(
        "text-2xl md:text-3xl font-semibold leading-tight text-foreground truncate",
        className,
      )}
    >
      {children}
    </h1>
  );
}

export interface EditorialSubtitleProps {
  children: ReactNode;
  className?: string;
}

export function EditorialSubtitle({ children, className }: EditorialSubtitleProps) {
  return (
    <p
      className={cn("text-[13px] text-muted-foreground leading-snug", className)}
    >
      {children}
    </p>
  );
}
