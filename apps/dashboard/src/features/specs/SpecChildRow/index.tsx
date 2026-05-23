import { cn } from "@/lib/utils";
import { StageBullet } from "../StageBullet";
import { useT } from "@/lib/i18n";
import type { Stage, SpecState } from "@/lib/types/specs";

export type ChildKind = "wave" | "ac" | "sub-spec";

interface SpecChildRowProps {
  kind: ChildKind;
  /** Primary label — wave "N · role", AC id, or sub-spec slug. */
  label: string;
  /** Optional secondary text (AC description, sub-spec reason). */
  detail?: string | null;
  /**
   * Canonical lifecycle state for the bullet — passed for sub-specs (the
   * child's own `state`). When absent, the bullet stage is derived from
   * `status` so waves/ACs still read done/active/future.
   */
  state?: SpecState;
  /** Kebab/lowercase status string for waves (`in-progress`…) / ACs (`pass`…). */
  status?: string;
  /** Click handler — opens the parent drill-down (children are not routable). */
  onClick?: () => void;
}

// Map a wave status onto a Stage so the bullet shows a sensible ramp. Waves
// don't have stages of their own; we colour by progress instead.
const WAVE_STATUS_STAGE: Record<string, Stage> = {
  queued: "plan",
  "in-progress": "execute",
  in_progress: "execute",
  completed: "close",
  failed: "execute",
};

/**
 * SpecChildRow — an indented child row under an expanded `SpecRow`. Renders a
 * 12px `StageBullet`, a fixed-width kind tag, the label and an optional detail.
 * Clicking drills into the parent spec (children carry no route of their own).
 */
export function SpecChildRow({
  kind,
  label,
  detail,
  state,
  status,
  onClick,
}: SpecChildRowProps) {
  const t = useT();
  const kindLabel = t(`route.specs.child.${kind.replace("-", "_")}`, kind);

  // Resolve the bullet stage: an explicit `state` (sub-specs) wins; otherwise
  // colour by progress derived from the wave/AC status string.
  const resolvedStage: Stage =
    state?.stage ??
    (kind === "wave" && status
      ? WAVE_STATUS_STAGE[status] ?? "execute"
      : "plan");

  return (
    <div
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onClick={onClick}
      onKeyDown={
        onClick
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick();
              }
            }
          : undefined
      }
      className={cn(
        "flex items-center gap-2 h-8 pl-12 pr-4 rounded-md text-[11px]",
        onClick &&
          "cursor-pointer transition-colors hover:bg-muted/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
      )}
    >
      <StageBullet
        size={12}
        stage={resolvedStage}
        outcome={state?.outcome ?? "active"}
        flags={state?.flags}
      />
      <span className="shrink-0 w-20 text-muted-foreground/60 uppercase tracking-wide">
        {kindLabel}
      </span>
      <span
        className={cn(
          "truncate min-w-0",
          kind === "sub-spec" ? "font-mono text-foreground/80" : "text-foreground/80",
        )}
        title={label}
      >
        {label}
      </span>
      {detail ? (
        <span
          className="truncate min-w-0 flex-1 text-muted-foreground/70"
          title={detail}
        >
          {detail}
        </span>
      ) : (
        <span className="flex-1" aria-hidden />
      )}
    </div>
  );
}
