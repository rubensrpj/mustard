import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/**
 * Wave-6 (2026-05-21, spec `spec-lifecycle-unification/wave-6-observability`) —
 * unified hygiene badge rendered inside `SpecRow` right of the spec name.
 *
 * Variants mirror the five hygiene signal categories:
 *   auto-closed  — dark-green  — spec was closed by the hygiene hook
 *   blocked      — amber       — pipeline paused, waiting for intervention
 *   wave-failed  — pink/rose   — at least one wave failed twice
 *   suspect      — slate/muted — hygiene.detected flagged this spec recently
 *   followup     — blue        — spec is in the 24h follow-up window after CLOSE
 *
 * Sizing: 11px text, 4px top/bottom × 6px left/right padding, rounded-sm. The
 * component is intentionally minimal — no hover states, no interactions.
 */
export type SpecBadgeVariant =
  | "auto-closed"
  | "blocked"
  | "wave-failed"
  | "suspect"
  | "followup";

const VARIANT_CLASSES: Record<SpecBadgeVariant, string> = {
  "auto-closed": "bg-emerald-950/60 text-emerald-400 border border-emerald-800/50",
  blocked: "bg-amber-950/60 text-amber-400 border border-amber-800/50",
  "wave-failed": "bg-rose-950/60 text-rose-400 border border-rose-800/50",
  suspect: "bg-slate-800/60 text-slate-400 border border-slate-700/50",
  followup: "bg-blue-950/60 text-blue-400 border border-blue-800/50",
};

const VARIANT_I18N_KEY: Record<SpecBadgeVariant, string> = {
  "auto-closed": "specs.badge.auto_closed",
  blocked: "specs.badge.blocked",
  "wave-failed": "specs.badge.wave_failed",
  suspect: "specs.badge.suspect",
  followup: "specs.badge.followup",
};

interface SpecBadgeProps {
  variant: SpecBadgeVariant;
  className?: string;
}

export function SpecBadge({ variant, className }: SpecBadgeProps) {
  const t = useT();
  return (
    <span
      className={cn(
        "inline-flex items-center shrink-0 rounded-sm",
        "text-[11px] font-medium leading-none",
        "px-1.5 py-[3px]",
        VARIANT_CLASSES[variant],
        className,
      )}
      aria-label={t(VARIANT_I18N_KEY[variant], variant)}
    >
      {t(VARIANT_I18N_KEY[variant], variant)}
    </span>
  );
}
