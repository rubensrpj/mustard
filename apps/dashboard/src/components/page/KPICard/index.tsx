import type { ReactNode } from "react";
import { Info } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

export type KPIAccent = "emerald" | "amber" | "rose" | "indigo" | "violet" | "sky" | "zinc";

const ACCENT_STRIPE: Record<KPIAccent, string> = {
  emerald: "bg-[--color-ok]/40",
  amber: "bg-[--color-accent-mustard]/40",
  rose: "bg-[--color-error]/40",
  indigo: "bg-primary/40",
  violet: "bg-primary/40",
  sky: "bg-primary/20",
  zinc: "bg-zinc-500/40",
};

const ACCENT_VALUE: Record<KPIAccent, string> = {
  emerald: "text-[--color-ok]",
  amber: "text-[--color-accent-mustard]",
  rose: "text-[--color-error]",
  indigo: "text-primary",
  violet: "text-primary",
  sky: "text-primary",
  zinc: "text-foreground",
};

/**
 * Small stat card with a colored top stripe, big value, label, and hint.
 * Used in KPI ribbons across pages. Accent stripe ties the card to a
 * semantic color (emerald for good metrics, amber for caution, etc.).
 */
export interface KPICardProps {
  label: string;
  value: ReactNode;
  hint?: string;
  /**
   * Smaller didactic line that explains what the value means and why it
   * matters. Lives inside the card, below the hint, in a quieter style.
   * Accepts JSX so callers can compose badges (e.g. a freshness StatusDot)
   * with prose.
   */
  caption?: ReactNode;
  /** Color of the top accent stripe and the value text. */
  accent?: KPIAccent;
  /** Hover tooltip. */
  tooltip?: string;
  /** Override the value color (when the value itself encodes the state). */
  valueClassName?: string;
  className?: string;
}

export function KPICard({
  label,
  value,
  hint,
  caption,
  accent = "zinc",
  tooltip,
  valueClassName,
  className,
}: KPICardProps) {
  const { t } = useTranslation();
  return (
    <div
      className={cn(
        "border border-border rounded-lg p-4 flex flex-col gap-1 bg-card/30 relative overflow-hidden h-full",
        className,
      )}
      title={tooltip}
    >
      <div className={cn("absolute top-0 left-0 right-0 h-0.5", ACCENT_STRIPE[accent])} />
      <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      <div
        className={cn(
          "text-2xl font-mono font-medium tabular-nums leading-tight",
          valueClassName ?? ACCENT_VALUE[accent],
        )}
      >
        {value}
      </div>
      {hint && (
        <div className="text-[11.5px] text-muted-foreground">{hint}</div>
      )}
      {caption && (
        <div className="mt-auto pt-3">
          <div className="relative pl-3 border-l-2 border-primary/40">
            <div className="absolute -left-[7px] top-0 flex h-3 w-3 items-center justify-center rounded-full bg-card border border-primary/40">
              <Info className="h-2 w-2 text-primary/70" strokeWidth={2.5} />
            </div>
            <div className="text-[10.5px] uppercase tracking-[0.14em] font-medium text-primary/60 mb-1">
              {t("kpi.captionLabel")}
            </div>
            <div className="flex flex-col gap-1 text-[11px] leading-snug text-muted-foreground [&_span]:block">
              {caption}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
