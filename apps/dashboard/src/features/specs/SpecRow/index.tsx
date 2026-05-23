import { ChevronRight, ChevronDown, ArrowRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { StageBullet } from "../StageBullet";
import { SpecBadge } from "../SpecBadge";
import { stateFromStatus } from "../_shared/stage-from-status";
import type { SpecCard } from "@/lib/types/specs";

interface SpecRowProps {
  data: SpecCard;
  /** Whether the expandable children tree is open for this spec. */
  expanded: boolean;
  /** Toggle the children tree (chevron click). */
  onToggle: (slug: string) => void;
  /** Open the spec in a new tab / drill-down (row click). */
  onOpen: (slug: string) => void;
  /**
   * Wave-6: set of spec slugs flagged as suspects by the hygiene hook
   * (`hygiene.detected` in the last 7 days, still active). When provided,
   * matching rows get a `suspect` badge. Passed down from `Specs.tsx` which
   * holds the `workspace_health` query result.
   */
  suspectSpecs?: ReadonlySet<string>;
  /**
   * Wave-6: set of spec slugs that were auto-closed today (`hygiene.autoclose`
   * in the last 24h). Used to render the `auto-closed` badge in the
   * "Encerradas" bucket.
   */
  autoClosedSpecs?: ReadonlySet<string>;
}

function formatDuration(ms: number | null): string {
  if (ms == null) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return rem > 0 ? `${m}m ${rem}s` : `${m}m`;
}

/**
 * SpecRow — a dense (32px) Linear-style list row replacing the ~150px
 * `SpecCard`. The leading cluster pairs an expand chevron with a `StageBullet`;
 * the row body shows the spec name (mono), model, wave + AC counters and the
 * duration. Clicking the row (anywhere but the chevron) opens the drill-down;
 * clicking the chevron toggles the inline children tree.
 */
export function SpecRow({
  data,
  expanded,
  onToggle,
  onOpen,
  suspectSpecs,
  autoClosedSpecs,
}: SpecRowProps) {
  const state = stateFromStatus(data.status);
  const Chevron = expanded ? ChevronDown : ChevronRight;

  // Compute which badges to render for this row (right of the name).
  // Order: blocked → wave-failed → followup → suspect → auto-closed.
  const badges: Array<"blocked" | "wave-failed" | "followup" | "suspect" | "auto-closed"> = [];
  if (state.flags.blocked) badges.push("blocked");
  if (state.flags.wave_failed) badges.push("wave-failed");
  if (state.flags.followup_open) badges.push("followup");
  if (suspectSpecs?.has(data.spec)) badges.push("suspect");
  if (autoClosedSpecs?.has(data.spec)) badges.push("auto-closed");

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onOpen(data.spec)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen(data.spec);
        }
      }}
      className={cn(
        "group/specrow flex items-center gap-2 h-8 px-4 rounded-md",
        "cursor-pointer transition-colors hover:bg-muted/30",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
      )}
    >
      {/* Leading: chevron + stage bullet. */}
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onToggle(data.spec);
        }}
        aria-label={expanded ? "Recolher" : "Expandir"}
        aria-expanded={expanded}
        className="shrink-0 grid place-items-center h-5 w-5 rounded text-muted-foreground/50 hover:text-muted-foreground hover:bg-muted/40 transition-colors"
      >
        <Chevron className="h-3.5 w-3.5" aria-hidden />
      </button>
      <StageBullet
        stage={state.stage}
        outcome={state.outcome}
        flags={state.flags}
      />

      {/* Spec name — mono, truncates at the end. */}
      <span
        className="font-mono text-[12px] text-foreground/90 truncate min-w-0"
        style={{ flex: "1 1 0%" }}
        title={data.spec}
      >
        {data.spec}
      </span>

      {/* Wave-6 hygiene badges — right of the name, before metric columns. */}
      {badges.length > 0 && (
        <div className="hidden sm:flex items-center gap-1 shrink-0">
          {badges.map((variant) => (
            <SpecBadge key={variant} variant={variant} />
          ))}
        </div>
      )}

      {/* Quantitative columns. Hidden on the narrowest widths so the name keeps
          priority; tabular-nums so counters align column-to-column. */}
      <div
        className="hidden sm:flex items-center gap-4 shrink-0 text-[11px] text-muted-foreground tabular-nums"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        <span
          className="hidden md:inline font-mono text-foreground/60 truncate max-w-[110px]"
          title={data.model ?? "modelo desconhecido"}
        >
          {data.model ?? "—"}
        </span>
        <span title="Ondas" className="w-12 text-right">
          {data.current_wave ?? "—"}/{data.total_waves ?? "—"}
        </span>
        <span title="Critérios de aceitação" className="w-12 text-right">
          {data.ac_passed}/{data.ac_total}
        </span>
        <span title="Duração" className="w-12 text-right">
          {formatDuration(data.duration_ms)}
        </span>
      </div>

      {/* Trailing affordance — appears on hover/focus. */}
      <ArrowRight
        className="shrink-0 h-3.5 w-3.5 text-muted-foreground/40 opacity-0 transition-opacity group-hover/specrow:opacity-100"
        aria-hidden
      />
    </div>
  );
}
