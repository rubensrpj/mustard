import { cn } from "@/lib/utils";
import { PhaseChip } from "@/components/page";
import { PipelineTimeline } from "@/components/telemetry/PipelineTimeline";
import { SpecActionMenu } from "./SpecActionMenu";
import type { SpecCard as SpecCardData } from "@/lib/types/specs";

interface SpecCardProps {
  data: SpecCardData;
  repoPath: string | null;
  /** When true, render the expanded drill-down area instead. */
  expanded?: boolean;
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

// Map a typed `mustard-specsdb::SpecStatus` (serialized as kebab-case) to a
// short human-readable label. Renders honest empty state ("—") when the spec
// has no harness events yet, instead of the old grey "UNKNOWN" badge.
const STATUS_LABELS: Record<string, string> = {
  "no-events":       "—",
  planning:          "planejamento",
  implementing:      "ativa",
  reviewing:         "review",
  qa:                "QA",
  "closed-followup": "follow-up",
  completed:         "concluída",
  cancelled:         "cancelada",
  blocked:           "bloqueada",
  "wave-failed":     "wave falhou",
  // Legacy strings from the pre-Wave-4 SQL fallback — kept so an old DB row
  // does not crash the render. New code emits the kebab-case forms above.
  active:            "ativa",
  closed:            "concluída",
};

function StatusPill({ status }: { status: string }) {
  const colorMap: Record<string, string> = {
    "no-events":       "bg-muted/40 text-muted-foreground/60",
    planning:          "bg-muted text-muted-foreground",
    implementing:      "bg-[--color-accent-mustard]/15 text-[--color-accent-mustard]",
    reviewing:         "bg-[--color-accent-mustard]/15 text-[--color-accent-mustard]",
    qa:                "bg-[--color-accent-mustard]/15 text-[--color-accent-mustard]",
    "closed-followup": "bg-muted text-muted-foreground",
    completed:         "bg-[--color-ok]/15 text-[--color-ok]",
    cancelled:         "bg-muted text-muted-foreground/50",
    blocked:           "bg-[--color-error]/15 text-[--color-error]",
    "wave-failed":     "bg-[--color-error]/15 text-[--color-error]",
    // Pre-Wave-4 fallback.
    active:            "bg-[--color-accent-mustard]/15 text-[--color-accent-mustard]",
    closed:            "bg-muted text-muted-foreground",
  };
  const cls = colorMap[status] ?? "bg-muted text-muted-foreground";
  const label = STATUS_LABELS[status] ?? status;
  return (
    <span
      className={cn(
        "text-[10px] font-medium px-1.5 py-0.5 rounded tracking-wide",
        // Render the empty state label in lowercase (the em-dash already
        // signals "no data" — UPPERCASE would shout it).
        status === "no-events" ? "" : "uppercase",
        cls,
      )}
      title={status}
    >
      {label}
    </span>
  );
}

/** Compact 5-station mini-timeline — reuses PipelineTimeline at reduced scale.
 *
 *  Wave 5 fix (2026-05-20): when the spec has no harness events yet
 *  (`phase === ""` after the specsdb adapter maps `Phase: None`) we render a
 *  dotted placeholder instead of five identical grey stations — same height
 *  so the card layout doesn't jump, but the visual reads as "no data" at a
 *  glance.
 */
function MiniTimeline({ card }: { card: SpecCardData }) {
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

  // Build the prop shape PipelineTimeline expects
  const completedPhases: string[] = [];
  const PHASES = ["analyze", "plan", "execute", "qa", "close"];
  const currentIdx = PHASES.indexOf(phase.toLowerCase());
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
      className="scale-[0.82] origin-left -mt-1"
    />
  );
}

export function SpecCard({ data, repoPath, className }: SpecCardProps) {
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

        <div className="flex items-center gap-1.5 shrink-0">
          <StatusPill status={data.status} />
          <PhaseChip phase={data.phase} />
          <span
            className="text-[11px] text-muted-foreground tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
            title="Duração"
          >
            {formatDuration(data.duration_ms)}
          </span>

          {/* Kebab action menu — visible on hover/focus */}
          <SpecActionMenu repoPath={repoPath} spec={data.spec} status={data.status} />
        </div>
      </div>

      {/* Mini pipeline timeline */}
      <MiniTimeline card={data} />

      {/* Quantitativos */}
      <div className="flex items-center gap-4 flex-wrap text-[11px] text-muted-foreground tabular-nums"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {data.total_waves != null && (
          <span title="Ondas">
            ondas{" "}
            <span className="text-foreground/70 font-medium">
              {data.current_wave ?? "—"}/{data.total_waves}
            </span>
          </span>
        )}
        <span title="Acceptance criteria">
          ACs{" "}
          <span className="text-foreground/70 font-medium">
            {data.ac_passed}/{data.ac_total}
          </span>
        </span>
        <span title="Arquivos tocados">
          arquivos{" "}
          <span className="text-foreground/70 font-medium">{data.files_touched}</span>
        </span>
        <span title="Ferramentas usadas">
          tools{" "}
          <span className="text-foreground/70 font-medium">{data.tools_used}</span>
        </span>
        {data.model && (
          <span
            className="ml-auto font-mono text-[10px] text-muted-foreground/50 truncate max-w-[120px]"
            title={data.model}
          >
            {data.model}
          </span>
        )}
      </div>
    </div>
  );
}
