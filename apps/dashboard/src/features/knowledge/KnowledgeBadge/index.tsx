import { cn } from "@/lib/utils";

/**
 * Typed knowledge badge — one of four `kind`s. Visual mapping is centralised
 * here (and re-exported as `KIND_BADGE`) so pages don't repeat the colour
 * lookup. Mustard yellow theme: avoid raw indigo/violet/sky/emerald/amber/rose
 * Tailwind classes; everything goes through CSS variables defined in theme.
 */
export type KnowledgeKind = "pattern" | "decision" | "lesson" | "friction";

/** Centralised colour lookup. Pages key into this by `row.type` (mapped) or
 *  by a hardcoded kind for legacy sources. */
export const KIND_BADGE: Record<KnowledgeKind, string> = {
  pattern: "bg-muted text-muted-foreground border border-border/60",
  decision:
    "bg-[--color-accent-mustard]/10 text-[--color-accent-mustard] border border-[--color-accent-mustard]/40",
  lesson: "bg-[--color-ok]/10 text-[--color-ok] border border-[--color-ok]/40",
  friction:
    "bg-[--color-error]/10 text-[--color-error] border border-[--color-error]/40",
};

const KIND_LABEL: Record<KnowledgeKind, string> = {
  pattern: "Padrão",
  decision: "Decisão",
  lesson: "Lição",
  friction: "Atrito",
};

export interface KnowledgeBadgeProps {
  kind: KnowledgeKind;
  /** Override label (defaults to a humanised version of `kind`). */
  label?: string;
  className?: string;
}

export function KnowledgeBadge({ kind, label, className }: KnowledgeBadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded px-1.5 py-0 text-[11px] font-medium",
        KIND_BADGE[kind],
        className,
      )}
    >
      {label ?? KIND_LABEL[kind]}
    </span>
  );
}

/** Maps a `KnowledgeBrowseRow.type` (free-form backend string) into one of the
 *  four canonical kinds. Anything unknown falls back to "pattern" (neutral).
 *  Centralised so callers don't re-derive the mapping. */
export function kindFromType(type: string): KnowledgeKind {
  switch (type) {
    case "decision":
      return "decision";
    case "lesson":
      return "lesson";
    case "pattern":
    case "naming-pattern":
    case "entity-cluster":
    case "convention":
    case "recipe":
      return "pattern";
    default:
      return "pattern";
  }
}
