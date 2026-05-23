import { Maximize2, ArrowUpRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { PipelineTimeline } from "@/features/telemetry/PipelineTimeline";
import { SpecActionMenu } from "../SpecActionMenu";
import { StatusPill } from "../_shared/spec-status";
import type { SpecCard as SpecCardData } from "@/lib/types/specs";

interface SpecCardProps {
  data: SpecCardData;
  repoPath: string | null;
  /** When true, render the expanded drill-down area instead. */
  expanded?: boolean;
  /**
   * Wave-4 (2026-05-20, spec mustard-wave-network-standard): when the spec is
   * a wave-plan parent, this is the count of child wave specs. The card then
   * renders a `+N waves` badge that links to the Network tab of the drill-
   * down. Undefined / 0 → no badge (back-compatible default).
   */
  childWaves?: number;
  /** Optional Network-tab href. Falls back to the spec's drill-down URL. */
  networkHref?: string;
  /**
   * Wave-3 (spec `2026-05-20-tactical-fix-via-sub-spec`): optional Sub-specs
   * tab href used by the `+N sub-specs` badge. Falls back to a plain
   * `#<spec>` deep-link so the row at least expands when clicked.
   */
  subSpecsHref?: string;
  /**
   * Wave-1 (spec `2026-05-21-dashboard-spec-tabs`): when provided, the card
   * renders a "Detalhes" button that asks the parent route to open this
   * spec in a new tab. Optional so callers without a tab system (e.g. the
   * Network drill-down preview) keep rendering without the action.
   */
  onOpenSpec?: (slug: string) => void;
  className?: string;
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
 * Wave 1 (spec `2026-05-21-dashboard-i18n-and-phase-unify`): the local
 * `MiniTimeline` was deleted in favour of `<PipelineTimeline>`. The
 * "no events yet" fallback is rendered inline below so the card height
 * stays stable across both states.
 *
 * Tactical-fix `2026-05-21-tf-speccard-polish`: the previous size-mode prop
 * on `<PipelineTimeline>` was removed; list and detail render identically.
 */
const PHASES = ["analyze", "plan", "execute", "qa", "close"] as const;

function CardTimeline({ card }: { card: SpecCardData }) {
  const phase = card.phase ?? "";
  if (!phase || phase === "no-events") {
    return (
      <div
        aria-label="Pipeline ainda sem eventos"
        className="-mt-1 h-7 flex items-center gap-1 text-muted-foreground/40 text-[10px]"
      >
        <span className="inline-block h-px flex-1 border-t border-dashed border-current" />
        <span className="px-1.5">sem eventos</span>
        <span className="inline-block h-px flex-1 border-t border-dashed border-current" />
      </div>
    );
  }

  const completedPhases: string[] = [];
  const currentIdx = PHASES.indexOf(phase.toLowerCase() as (typeof PHASES)[number]);
  PHASES.forEach((p, i) => {
    if (i < currentIdx) completedPhases.push(p);
  });

  return (
    <PipelineTimeline
      pipeline={{
        spec: card.spec,
        currentPhase: phase,
        phasesCompleted: completedPhases,
      }}
    />
  );
}

export function SpecCard({
  data,
  repoPath,
  childWaves,
  networkHref,
  subSpecsHref,
  onOpenSpec,
  className,
}: SpecCardProps) {
  // Wave-4: a spec is a "parent" when it has any child waves. Render the
  // `+N waves` badge that takes the user to the Network tab of the drill-
  // down. We use an anchor (not router) so callers without a Router context
  // (e.g. Storybook) still render — `networkHref` may be absent in tests.
  const hasChildren = typeof childWaves === "number" && childWaves > 0;

  // Wave-3 (spec `2026-05-20-tactical-fix-via-sub-spec`): badge counts how
  // many sub-specs are linked via `spec.link` events. The card used to fan
  // out one per-row child-list query (N+1 invokes for a long list); spec
  // `2026-05-21-speccard-use-children-count` moves the count onto the
  // `SpecCard` payload itself (populated by `spec_card_v2` on the Rust
  // side). The drill-down / children-tab views keep using the dedicated
  // hook — they need the full row list, not just the count.
  const subSpecCount = data.children_count ?? 0;
  const hasSubSpecs = subSpecCount > 0;

  return (
    <div
      className={cn(
        "group/speccard relative flex flex-col gap-3 rounded-lg border border-border",
        "bg-card/20 p-3 w-full transition-colors hover:border-border/80",
        className,
      )}
    >
      {/* Header row */}
      <div className="flex items-start gap-2 min-w-0">
        {/* Spec name — truncate at end, never cut the prefix */}
        <span
          className="font-mono text-[13px] font-medium truncate flex-1 min-w-0"
          title={data.spec}
        >
          {data.spec}
        </span>

        <div className="flex items-center gap-2 shrink-0">
          {/* Wave 4 (spec `2026-05-21-dashboard-spec-tabs-polish`): the
              +N waves badge now reads in mustard accent (not muted) so the
              eye picks it up as a "parent has children to drill into" cue.
              The canonical className="bg-[--primary]/15 ..."
              applied below is what keeps AC-W4-4 honest. */}
          {hasChildren && (
            networkHref ? (
              <a
                href={networkHref}
                onClick={(e) => e.stopPropagation()}
                title={`${childWaves} waves — abrir aba Network`}
                className="text-[10px] font-mono font-medium px-1.5 py-0.5 rounded uppercase tracking-wide bg-[--primary]/15 text-[--primary] hover:bg-[--primary]/25 transition-colors"
              >
                +{childWaves} waves
              </a>
            ) : (
              <span
                title={`${childWaves} waves`}
                className="text-[10px] font-mono font-medium px-1.5 py-0.5 rounded uppercase tracking-wide bg-[--primary]/15 text-[--primary]"
              >
                +{childWaves} waves
              </span>
            )
          )}
          {hasSubSpecs && (
            <a
              href={subSpecsHref ?? `#${data.spec}`}
              onClick={(e) => e.stopPropagation()}
              title={`${subSpecCount} sub-specs — abrir aba Sub-specs`}
              className="text-xs font-medium px-1.5 py-0.5 rounded bg-cyan-500/15 text-cyan-400 tabular-nums hover:bg-cyan-500/25 transition-colors"
              style={{ fontVariantNumeric: "tabular-nums" }}
            >
              +{subSpecCount} sub-specs
            </a>
          )}
          <StatusPill status={data.status} />

          {/* Tactical-fix `2026-05-21-tf-speccard-polish`:
              - PhaseChip removed (PipelineTimeline below already shows the
                current phase — chip was redundant).
              - Duration moved to the bottom-right of the quantitative row.
              - The "—" separator between badges and the Detalhes button was
                removed; gap on the parent cluster carries the spacing.
              - The Detalhes button is now a bordered chip (Maximize2 + label
                + ArrowUpRight) so it reads as the primary card action. */}
          {onOpenSpec && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onOpenSpec(data.spec);
              }}
              aria-label="Abrir detalhes em nova aba"
              title="Abrir em nova aba"
              className="inline-flex items-center gap-1 h-7 px-2.5 rounded-md bg-card border border-border hover:bg-muted/60 hover:border-foreground/20 transition-colors text-[12px] font-medium"
            >
              <Maximize2 className="h-3.5 w-3.5" aria-hidden />
              Detalhes
              <ArrowUpRight className="h-3 w-3 text-muted-foreground" aria-hidden />
            </button>
          )}

          {/* Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`): the
              inline markdown-viewer trigger that lived here was removed
              from the card — the markdown access path is now exclusively
              the Ondas tab (Onda #0 row opens the parent markdown via the
              wave drawer). The kebab action menu and "Detalhes" button
              remain. */}

          {/* Kebab action menu — visible on hover/focus */}
          <SpecActionMenu repoPath={repoPath} spec={data.spec} status={data.status} />
        </div>
      </div>

      {/* Wave 1: compact pipeline timeline (was MiniTimeline). */}
      <CardTimeline card={data} />

      {/* Tactical-fix `2026-05-21-tf-speccard-polish` (item 5): the
          quantitative footer now ALWAYS renders every metric (ondas / ACs /
          arquivos / tools / modelo) with a `—` fallback so missing values
          stay visible instead of silently collapsing. Duration is the last
          element and carries the `ml-auto` (was on `model` before). */}
      <div className="flex items-center gap-4 flex-wrap text-[11px] text-muted-foreground tabular-nums"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        <span title="Ondas">
          <span className="text-muted-foreground/60">ondas</span>{" "}
          <span className="text-foreground/70 font-medium">
            {data.current_wave ?? "—"}/{data.total_waves ?? "—"}
          </span>
        </span>
        <span title="Critérios de aceitação">
          <span className="text-muted-foreground/60">ACs</span>{" "}
          <span className="text-foreground/70 font-medium">
            {data.ac_passed}/{data.ac_total}
          </span>
        </span>
        <span title="Arquivos tocados">
          <span className="text-muted-foreground/60">arquivos</span>{" "}
          <span className="text-foreground/70 font-medium">{data.files_touched}</span>
        </span>
        <span title="Ferramentas usadas">
          <span className="text-muted-foreground/60">tools</span>{" "}
          <span className="text-foreground/70 font-medium">{data.tools_used}</span>
        </span>
        <span title="Modelo" className="truncate max-w-[140px]">
          <span className="text-muted-foreground/60">modelo</span>{" "}
          <span className="text-foreground/70 font-medium font-mono">
            {data.model ?? "—"}
          </span>
        </span>
        <span
          className="ml-auto text-[11px] text-muted-foreground tabular-nums"
          style={{ fontVariantNumeric: "tabular-nums" }}
          title="Duração"
        >
          <span className="text-muted-foreground/60">duração</span>{" "}
          <span className="text-foreground/70 font-medium">
            {formatDuration(data.duration_ms)}
          </span>
        </span>
      </div>
    </div>
  );
}
