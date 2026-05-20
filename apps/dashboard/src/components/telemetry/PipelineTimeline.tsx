import { cn } from "@/lib/utils";
import { PhaseStation, type PhaseStationState } from "./PhaseStation";

const PHASES: Array<"analyze" | "plan" | "execute" | "qa" | "close"> = [
  "analyze",
  "plan",
  "execute",
  "qa",
  "close",
];

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

export function PipelineTimeline({ pipeline, className }: PipelineTimelineProps) {
  const hasActivePipeline = !!pipeline?.spec;

  return (
    <div className={cn("w-full animate-mount-fade", className)}>
      {/* connector bar + stations row */}
      <div className="relative flex items-start justify-between px-4">
        {/* horizontal connector line behind stations */}
        <div
          className="absolute top-[18px] left-8 right-8 h-px bg-border"
          aria-hidden="true"
        />

        {PHASES.map((phase) => {
          const state: PhaseStationState = hasActivePipeline
            ? resolveState(phase, pipeline!.currentPhase, pipeline!.phasesCompleted)
            : "future";

          return (
            <div key={phase} className="relative z-10">
              <PhaseStation phase={phase} state={state} />
            </div>
          );
        })}
      </div>

      {/* spec label or empty state */}
      <div className="mt-3 px-4">
        {hasActivePipeline ? (
          <p className="text-[12px] text-muted-foreground truncate">
            <span className="text-foreground font-medium">{pipeline!.spec}</span>
          </p>
        ) : (
          <p className="text-[12px] text-muted-foreground/60">
            {pipeline?.lastClosed
              ? `Última fechada: ${pipeline.lastClosed.spec} · ${formatLastClosed(pipeline.lastClosed.completedAt)}`
              : "Nenhuma pipeline em execução"}
          </p>
        )}
      </div>
    </div>
  );
}
