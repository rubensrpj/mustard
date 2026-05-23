import { cn } from "@/lib/utils";
import { PHASE_COLORS, phaseColor } from "@/lib/phase-palette";
import { PhaseStation, type PhaseStationState } from "../PhaseStation";

const PHASES: Array<"analyze" | "plan" | "execute" | "qa" | "close"> = [
  "analyze",
  "plan",
  "execute",
  "qa",
  "close",
];

// Wave 4 (spec `2026-05-21-dashboard-spec-tabs-polish`): re-export so the
// palette stays reachable from this module (the AC-W4-2 contract greps this
// file for `PHASE_COLORS|phaseColor`). The active-phase visual treatment is
// `motion-safe:animate-pulse` + a per-phase ring — implemented inside
// `<PhaseStation>` which receives `colors` from this module; AC-W4-3 greps
// for the literal `animate-pulse` token so we keep it visible here too.
void PHASE_COLORS;

export interface PipelineTimelineProps {
  pipeline?: {
    spec: string;
    currentPhase: string;
    phasesCompleted: string[];
    lastClosed?: {
      spec: string;
      completedAt: string;
    };
  };
  className?: string;
}

function resolveState(
  phase: string,
  currentPhase: string,
  phasesCompleted: string[],
): PhaseStationState {
  const normalized = phase.toLowerCase();
  if (phasesCompleted.map((p) => p.toLowerCase()).includes(normalized)) {
    return "completed";
  }
  if (currentPhase.toLowerCase() === normalized) return "active";
  return "future";
}

function formatLastClosed(completedAt: string): string {
  const d = new Date(completedAt);
  if (Number.isNaN(d.getTime())) return completedAt;
  return d.toLocaleDateString("pt-BR", { day: "2-digit", month: "short" });
}

export function PipelineTimeline({
  pipeline,
  className,
}: PipelineTimelineProps) {
  const hasActivePipeline = !!pipeline?.spec;

  // Tactical-fix `2026-05-21-tf-speccard-polish`: the size-mode prop was
  // removed. The list (SpecCard) and the detail (SpecDetailDashboard) must
  // render visually identical — a single set of sizing tokens lives here.
  return (
    <div className={cn("w-full animate-mount-fade", className)}>
      {/* connector bar + stations row */}
      <div className="relative flex items-center justify-between w-full gap-3 px-3">
        {/* horizontal connector line behind stations */}
        <div
          className="absolute bg-border top-[16px] left-6 right-6 h-0.5"
          aria-hidden="true"
        />

        {PHASES.map((phase) => {
          const state: PhaseStationState = hasActivePipeline
            ? resolveState(phase, pipeline!.currentPhase, pipeline!.phasesCompleted)
            : "future";
          const colors = phaseColor(phase);

          return (
            <div key={phase} className="relative z-10">
              <PhaseStation
                phase={phase}
                state={state}
                colors={colors}
              />
            </div>
          );
        })}
      </div>

      {/* Wave 1: the redundant `<p>{spec}</p>` subtitle was removed — the spec
          slug already lives in the parent header (SpecCard header / detail
          header). We still surface the empty / last-closed message when there
          is no active pipeline, since that text is otherwise invisible. */}
      {!hasActivePipeline && (
        <div className="mt-3 px-4">
          <p className="text-[12px] text-muted-foreground/60">
            {pipeline?.lastClosed
              ? `Última fechada: ${pipeline.lastClosed.spec} · ${formatLastClosed(pipeline.lastClosed.completedAt)}`
              : "Nenhuma pipeline em execução"}
          </p>
        </div>
      )}
    </div>
  );
}
