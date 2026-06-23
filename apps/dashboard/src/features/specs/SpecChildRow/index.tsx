import { cn } from "@/lib/utils";
import { StageBullet } from "../StageBullet";
import { StatusPill } from "../_shared/spec-status";
import { useT } from "@/lib/i18n";
import type { Stage, Outcome, Flags, SpecState } from "@/lib/types/specs";

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
 * Derive the StageBullet `outcome` + `flags` from a wave/AC `status` string when
 * the row carries no explicit `state` (sub-specs pass their own `state`; waves
 * and ACs only carry a flat status). Without this the bullet was hard-coded to
 * `outcome="active"`, so a finished/passed child always read as an empty ring
 * instead of the terminal "done" glyph.
 *
 * A terminal status (`completed`/`pass`) lifts to `outcome="completed"` so the
 * bullet paints the full green ring + check. A failed status (`failed`/`fail`)
 * stays `active` but raises the `wave_failed` flag so the alert adornment shows
 * on top of the in-flight ring. Everything else (queued/in-progress/skip/
 * pending/unknown) stays `active` and lets the staged ring read progress.
 */
function outcomeFromChildStatus(status: string): {
  outcome: Outcome;
  flags: Partial<Flags>;
} {
  switch (status) {
    case "completed":
    case "pass":
      return { outcome: "completed", flags: {} };
    case "failed":
    case "fail":
      return { outcome: "active", flags: { wave_failed: true } };
    default:
      return { outcome: "active", flags: {} };
  }
}

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

  // Resolve the bullet outcome/flags. An explicit `state` (sub-specs) wins; for
  // waves/ACs derive from the flat `status` so a completed wave or passed AC
  // paints the terminal "done" glyph instead of an empty active ring.
  const derived = status ? outcomeFromChildStatus(status) : null;
  const resolvedOutcome: Outcome = state?.outcome ?? derived?.outcome ?? "active";
  const resolvedFlags: Partial<Flags> | undefined =
    state?.flags ?? derived?.flags;

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
        outcome={resolvedOutcome}
        flags={resolvedFlags}
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
      {/* Running marker for the live wave — makes the executing wave obvious
          in the inline list expand, mirroring the Ondas tab badge. Uses the
          mustard accent so it stands out from the neutral status pill. */}
      {kind === "wave" && (status === "in_progress" || status === "in-progress") ? (
        <span
          className="shrink-0 text-[9px] font-semibold px-1 py-0.5 rounded uppercase tracking-wide bg-[--primary] text-[--primary-foreground] animate-pulse"
          title={t("specWaves.row.runningBadgeTitle")}
        >
          {t("specWaves.row.runningBadge")}
        </span>
      ) : null}
      {/* Per-row status indicator the detail tabs' columns expect: pass/fail
          for ACs, completed/in-progress/queued/failed for waves. Sub-specs
          already carry their lifecycle in the StageBullet, so the pill is
          skipped for them to keep the row uncluttered. */}
      {kind !== "sub-spec" && status ? (
        <span className="shrink-0">
          <StatusPill status={status} />
        </span>
      ) : null}
    </div>
  );
}
