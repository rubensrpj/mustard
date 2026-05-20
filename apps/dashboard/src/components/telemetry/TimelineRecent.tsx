import { useTelemetryTimeline } from "@/hooks/useTelemetryTimeline";
import type { TimeRange } from "@/lib/types/telemetry";
import { relativeTime } from "@/lib/time";

interface Props {
  repoPath: string | null;
  timeRange: TimeRange;
}

export function TimelineRecent({ repoPath, timeRange }: Props) {
  const { data, isLoading } = useTelemetryTimeline(repoPath, timeRange, 5);

  if (isLoading && !data) {
    return (
      <ul className="flex flex-col gap-1.5">
        {Array.from({ length: 5 }).map((_, i) => (
          <li
            key={i}
            className="h-4 rounded bg-muted animate-pulse"
            style={{ width: `${60 + (i % 3) * 12}%` }}
          />
        ))}
      </ul>
    );
  }

  if (!data || data.length === 0) {
    return (
      <p className="text-[12px] text-muted-foreground/60 py-1">
        Nenhum evento registrado para o período.
      </p>
    );
  }

  return (
    <ul className="flex flex-col divide-y divide-border/40">
      {data.map((ev) => (
        <li key={ev.id} className="py-1.5 flex flex-col gap-0.5 min-w-0">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="text-[11px] text-muted-foreground font-mono tabular-nums shrink-0">
              {ev.ts ? relativeTime(ev.ts) : "—"}
            </span>
            {ev.phase && (
              <>
                <span className="text-muted-foreground/40 text-[10px]">·</span>
                <span className="text-[10px] font-medium text-foreground/70 shrink-0">
                  {ev.phase.toLowerCase()}
                </span>
              </>
            )}
            {ev.spec && (
              <>
                <span className="text-muted-foreground/40 text-[10px]">·</span>
                <span
                  className="text-[10px] font-mono text-muted-foreground truncate"
                  title={ev.spec}
                >
                  {ev.spec.length > 28 ? `${ev.spec.slice(0, 28)}…` : ev.spec}
                </span>
              </>
            )}
          </div>
          <span className="text-[11px] text-foreground/80 line-clamp-1" title={ev.summary}>
            {ev.summary}
          </span>
        </li>
      ))}
    </ul>
  );
}
