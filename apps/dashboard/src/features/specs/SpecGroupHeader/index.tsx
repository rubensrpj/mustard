import { ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";

interface SpecGroupHeaderProps {
  label: string;
  count: number;
  expanded: boolean;
  onToggle: () => void;
}

/**
 * SpecGroupHeader — a `▾/▸ STAGE_LABEL COUNT` toggle row that heads each Stage
 * group on `/specs`. Empty groups collapse; the count is rendered in a muted
 * tabular figure so it never competes with the label.
 */
export function SpecGroupHeader({
  label,
  count,
  expanded,
  onToggle,
}: SpecGroupHeaderProps) {
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-expanded={expanded}
      className={cn(
        "flex items-center gap-1.5 h-7 px-2 rounded-md w-full text-left",
        "text-[11px] uppercase tracking-wide text-muted-foreground",
        "hover:bg-muted/30 hover:text-foreground transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--color-accent-mustard]",
      )}
    >
      <Chevron className="h-3.5 w-3.5 text-muted-foreground/50" aria-hidden />
      <span className="font-medium">{label}</span>
      <span
        className="tabular-nums text-muted-foreground/60"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {count}
      </span>
    </button>
  );
}
