import { useMemo } from "react";
import { useNavigate } from "react-router";
import { Activity } from "lucide-react";
import { cn } from "@/lib/utils";
import { DataCard, EmptyState, SectionHeader } from "@/components/page";
import { PhaseChip } from "@/components/page/PhaseChip";
import { MetricsPill } from "@/components/ds";
import { useTranslate } from "@/lib/i18n";
import { relativeTime } from "@/lib/time";
import type { SpecTrack, WorkspaceSummary } from "@/lib/types/specs";

interface WorkspaceHeroProps {
  summary: WorkspaceSummary | undefined;
}

/** Filter terms that mean "this spec is parked / done". Mirrors the same
 *  bucket used by `WorkspaceSpecsByStatus`. */
const TERMINAL = new Set(["completed", "closed", "cancelled", "no-events"]);

/**
 * Active-pipeline list rendering one row per ongoing spec. Replaces the
 * single-pipeline `<WorkspaceStatusBar>` + `<PipelineTimeline>` pair so the
 * Visão Geral hero stays useful when 2+ pipelines run in parallel.
 *
 * Rendering rules:
 *   - Source = `summary.spec_tracks` filtered to non-terminal status, sorted
 *     by `last_event_at` desc. We iterate with `.map` (AC-4).
 *   - Each row shows: status dot · spec name · `<PhaseChip>` · agents-active
 *     pill · relative last-activity. Tokens consumed per spec would need a
 *     telemetry round-trip — agents-active is the closest live signal already
 *     in the workspace payload.
 *   - Clicking a row routes to `/specs#<name>` so the operator can drill in.
 */
export function WorkspaceHero({ summary }: WorkspaceHeroProps) {
  const t = useTranslate();
  const navigate = useNavigate();

  const active: SpecTrack[] = useMemo(() => {
    const tracks = summary?.spec_tracks ?? [];
    return tracks
      .filter((track) => !TERMINAL.has(track.status.toLowerCase()))
      .slice()
      .sort((a, b) => (b.last_event_at ?? "").localeCompare(a.last_event_at ?? ""));
  }, [summary?.spec_tracks]);

  if (active.length === 0) {
    return (
      <DataCard padded>
        <SectionHeader title={t("workspace.activePipelines")} />
        <EmptyState
          className="mt-3"
          title={t("hero.empty")}
          description={t("hero.emptyHint")}
        />
      </DataCard>
    );
  }

  return (
    <DataCard padded>
      <SectionHeader
        title={t("workspace.activePipelines")}
        right={
          <span
            className="text-[11px] text-muted-foreground tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {active.length}
          </span>
        }
      />
      <ul className="mt-3 flex flex-col divide-y divide-border/40">
        {active.map((track) => (
          <li key={track.spec}>
            <button
              type="button"
              onClick={() => navigate(`/specs#${track.spec}`)}
              className={cn(
                "w-full flex items-center gap-3 px-2 py-2 text-left",
                "hover:bg-muted/40 rounded transition-colors",
                "focus-visible:outline-none focus-visible:ring-2",
                "focus-visible:ring-[--ds-accent-primary]/60",
              )}
              aria-label={`${track.spec} — ${track.current_phase || "queued"}`}
            >
              <Activity
                className="h-3.5 w-3.5 shrink-0 text-[--ds-text-tertiary]"
                aria-hidden
              />
              <span
                className="font-mono text-[12.5px] text-foreground/90 truncate flex-1 min-w-0"
                title={track.spec}
              >
                {track.spec}
              </span>
              <PhaseChip phase={track.current_phase || null} size="sm" />
              {track.agents_active > 0 ? (
                <MetricsPill
                  value={track.agents_active}
                  unit="ag"
                  intent="info"
                  tooltip={`${track.agents_active} active agents`}
                />
              ) : null}
              <span
                className="text-[11px] text-muted-foreground/70 shrink-0 min-w-[60px] text-right tabular-nums"
                style={{ fontVariantNumeric: "tabular-nums" }}
                title={track.last_event_at ?? ""}
              >
                {track.last_event_at ? relativeTime(track.last_event_at) : "—"}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </DataCard>
  );
}
