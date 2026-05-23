import { cn } from "@/lib/utils";
import { ActivePipeline } from "@/lib/dashboard";
import { StatusDot, StatusDotVariant } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";
import { formatDurationMs } from "@/lib/format";

interface LivePipelineCardProps {
  pipeline: ActivePipeline;
  projectName?: string;
  onClick?: () => void;
}

function phaseToVariant(phase: string, hasFailure: boolean): StatusDotVariant {
  if (hasFailure) return "blocked";
  const p = phase.toUpperCase();
  if (p === "EXECUTE") return "active";
  if (p === "CLOSE") return "done";
  if (p === "ANALYZE" || p === "PLAN" || p === "QA") return "planning";
  return "idle";
}

export function LivePipelineCard({ pipeline, projectName, onClick }: LivePipelineCardProps) {
  const {
    spec_name,
    phase,
    model,
    has_dispatch_failure,
    failure_age_ms,
    current_wave,
    total_waves,
    tasks_pending,
    tasks_in_progress,
    tasks_completed,
    updated_at,
  } = pipeline;

  const variant = phaseToVariant(phase, has_dispatch_failure);
  const pulse = phase.toUpperCase() === "EXECUTE" && !has_dispatch_failure;

  const total = tasks_pending + tasks_in_progress + tasks_completed;
  const completedPct = total > 0 ? (tasks_completed / total) * 100 : 0;
  const inProgressPct = total > 0 ? (tasks_in_progress / total) * 100 : 0;

  const showWave =
    current_wave != null && total_waves != null && total_waves > 0;
  const showTasks = total > 0;
  const showFailure = has_dispatch_failure;

  const interactive = !!onClick;

  return (
    <li
      className={cn(
        "flex flex-col gap-1 px-2 py-1.5 rounded",
        interactive && "cursor-pointer hover:bg-muted/40",
      )}
      onClick={onClick}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onKeyDown={
        interactive
          ? (e) => {
              if (e.key === "Enter") onClick();
            }
          : undefined
      }
    >
      {/* Primary row */}
      <div className="flex items-center gap-2">
        <StatusDot variant={variant} pulse={pulse} />
        {projectName && (
          <span className="text-muted-foreground text-[12px]">{projectName}</span>
        )}
        <span className="font-mono text-[13px]">{spec_name}</span>
        <Badge variant="outline" className="text-[10px] font-mono">
          {phase}
        </Badge>
        {model && (
          <Badge variant="secondary" className="text-[10px]">
            {model}
          </Badge>
        )}
        <span className="ml-auto text-[12px] text-muted-foreground">
          {updated_at ? relativeTime(updated_at) : "—"}
        </span>
      </div>

      {/* Wave bar */}
      {showWave && (
        <div className="flex items-baseline gap-2 text-[12px] text-muted-foreground">
          <span>
            W{current_wave}/{total_waves}
          </span>
          <div className="flex-1 h-1 bg-muted rounded overflow-hidden">
            <div
              className="h-full bg-[--color-accent-mustard]/40"
              style={{ width: `${(current_wave! / total_waves!) * 100}%` }}
            />
          </div>
        </div>
      )}

      {/* Tasks progress */}
      {showTasks && (
        <div className="text-[12px] text-muted-foreground flex items-center gap-2">
          <span>
            {tasks_completed}/{total} done
          </span>
          <div className="flex h-1 w-32 rounded overflow-hidden bg-muted">
            <div className="bg-[--color-ok]/40" style={{ width: `${completedPct}%` }} />
            <div className="bg-[--color-accent-mustard]/40" style={{ width: `${inProgressPct}%` }} />
          </div>
        </div>
      )}

      {/* Failure banner */}
      {showFailure && (
        <div className="text-[12px] rounded px-2 py-1 bg-[--color-error]/10 border border-[--color-error]/30 text-[--color-error]">
          Dispatch failed {formatDurationMs(failure_age_ms ?? 0)} ago — run{" "}
          <code className="font-mono">/resume</code>
        </div>
      )}
    </li>
  );
}
