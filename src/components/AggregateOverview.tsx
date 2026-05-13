import { useNavigate } from "react-router";
import { Activity, FolderGit2, Layers, Play, CheckCircle2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import {
  useAggregate,
  type ActivePipelineRow,
  type TimelineRow,
} from "@/hooks/useAggregate";
import type { Project } from "@/api/discovery";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { relativeTime } from "@/lib/time";

function specVariant(phase: string | null, status: string | null): StatusDotVariant {
  if (status === "blocked") return "blocked";
  switch (phase) {
    case "EXECUTE":
      return "active";
    case "ANALYZE":
    case "PLAN":
    case "QA":
      return "planning";
    case "CLOSE":
      return "done";
    default:
      return "idle";
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

function Counter({
  label,
  value,
  icon: Icon,
  loading,
}: {
  label: string;
  value: number;
  icon: LucideIcon;
  loading: boolean;
}) {
  return (
    <div className="flex flex-col gap-1 px-3 py-2 rounded border border-border bg-card">
      <div className="flex items-center gap-2 text-muted-foreground">
        <Icon className="h-3.5 w-3.5" />
        <span className="text-[10px] uppercase tracking-wider">{label}</span>
      </div>
      <span className="text-xl font-mono font-medium text-foreground">
        {loading ? "—" : value}
      </span>
    </div>
  );
}

export function AggregateOverview({ projects }: { projects: Project[] }) {
  const navigate = useNavigate();
  const { counters, activePipelines, timeline, loading } = useAggregate(projects);

  return (
    <div className="flex flex-col gap-6">
      <section>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
          <Counter label="Specs ativas" value={counters.activeSpecs} icon={Layers} loading={loading} />
          <Counter label="Em EXECUTE" value={counters.executing} icon={Play} loading={loading} />
          <Counter label="Completed 7d" value={counters.completed7d} icon={CheckCircle2} loading={loading} />
          <Counter label="Eventos hoje" value={counters.eventsToday} icon={Activity} loading={loading} />
        </div>
      </section>

      <section>
        <div className="flex items-baseline gap-2 mb-2">
          <h2 className="text-[11px] uppercase tracking-wider font-medium text-foreground">
            Pipelines ativas
          </h2>
          <span className="text-[11px] text-muted-foreground/50 font-mono">
            {loading ? "…" : activePipelines.length}
          </span>
        </div>
        {!loading && activePipelines.length === 0 ? (
          <p className="text-xs text-muted-foreground py-2">Sem pipelines ativas.</p>
        ) : (
          <ul className="flex flex-col gap-0.5 text-sm">
            {activePipelines.map((row: ActivePipelineRow) => {
              const variant = specVariant(row.spec.phase, row.spec.status);
              return (
                <li
                  key={`${row.projectId}/${row.spec.name}`}
                  className="flex items-center gap-2 px-2 py-1 rounded hover:bg-muted/40 cursor-pointer"
                  onClick={() =>
                    navigate(
                      `/project/${row.projectId}/spec/${encodeURIComponent(row.spec.name)}`,
                    )
                  }
                >
                  <StatusDot variant={variant} pulse={variant === "active"} />
                  <span className="text-muted-foreground text-xs">{row.projectName}</span>
                  <span className="text-muted-foreground/50">/</span>
                  <span className="font-mono">{row.spec.name}</span>
                  {row.spec.phase && (
                    <Badge variant="secondary" className="text-[10px] py-0">
                      {row.spec.phase}
                    </Badge>
                  )}
                  <span className="ml-auto text-muted-foreground text-xs">
                    {row.spec.started_at ? relativeTime(row.spec.started_at) : "—"}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <Separator />

      <section>
        <div className="flex items-baseline gap-2 mb-2">
          <h2 className="text-[11px] uppercase tracking-wider font-medium text-foreground">
            Atividade recente
          </h2>
          <span className="text-[11px] text-muted-foreground/50 font-mono">
            {loading ? "…" : timeline.length}
          </span>
        </div>
        {!loading && timeline.length === 0 ? (
          <p className="text-xs text-muted-foreground py-2">Sem eventos recentes.</p>
        ) : (
          <ul className="flex flex-col gap-0.5 text-sm">
            {timeline.map((row: TimelineRow, i: number) => (
              <li
                key={i}
                className="flex items-baseline gap-2 px-2 py-1 rounded hover:bg-muted/40"
              >
                <Badge variant="secondary" className="text-[10px] py-0 font-mono">
                  {row.event.event_type}
                </Badge>
                <span className="text-muted-foreground text-xs flex items-center gap-1">
                  <FolderGit2 className="h-3 w-3" />
                  {row.projectName}
                </span>
                {row.event.ts && (
                  <span className="text-muted-foreground text-xs">
                    {relativeTime(row.event.ts)}
                  </span>
                )}
                {row.event.summary && (
                  <span className="text-muted-foreground text-xs">
                    — {truncate(row.event.summary, 120)}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
