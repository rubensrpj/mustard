import { useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useStore } from "@/lib/store";
import { discoverProjects } from "@/api/discovery";
import { useActivityFeed } from "@/hooks/useActivityFeed";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";

const PAGE_SIZE = 50;
const LIMIT_PER_PROJECT = 100;

function eventVariant(eventType: string): StatusDotVariant {
  switch (eventType) {
    case "tool.use":
    case "commit-gate.check":
      return "idle";
    case "pipeline.phase":
      return "planning";
    case "qa.result":
      return "done";
    case "agent.start":
    case "session.start":
      return "active";
    default:
      return "idle";
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

export function Activity() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const { data: projects } = useQuery({
    queryKey: ["discover", projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const { events, types, loading } = useActivityFeed(projects ?? [], LIMIT_PER_PROJECT);

  const [activeTypes, setActiveTypes] = useState<Set<string>>(new Set());
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);

  const filtered = useMemo(() => {
    if (activeTypes.size === 0) return events;
    return events.filter((row) => activeTypes.has(row.event.event_type));
  }, [events, activeTypes]);

  const visible = filtered.slice(0, visibleCount);
  const hasMore = filtered.length > visible.length;

  function toggleType(t: string) {
    setActiveTypes((prev) => {
      const next = new Set(prev);
      if (next.has(t)) next.delete(t);
      else next.add(t);
      return next;
    });
    setVisibleCount(PAGE_SIZE);
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <nav className="text-xs text-muted-foreground">
          Mustard / <span className="text-foreground">Activity</span>
        </nav>
        <h1 className="text-base font-medium">Activity cross-project</h1>
      </div>

      {!projectsRoot ? (
        <p className="text-xs text-muted-foreground">
          Configure o diretório de projetos em Settings.
        </p>
      ) : (
        <>
          {types.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {types.map((t) => {
                const active = activeTypes.has(t);
                return (
                  <button
                    key={t}
                    onClick={() => toggleType(t)}
                    className="cursor-pointer"
                    type="button"
                  >
                    <Badge
                      variant={active ? "default" : "outline"}
                      className="text-[10px] py-0 font-mono"
                    >
                      {t}
                    </Badge>
                  </button>
                );
              })}
            </div>
          )}

          {loading ? (
            <ul className="flex flex-col gap-1">
              {[0, 1, 2, 3, 4].map((i) => (
                <li key={i} className="h-6 bg-muted/40 rounded animate-pulse" />
              ))}
            </ul>
          ) : filtered.length === 0 ? (
            <p className="text-xs text-muted-foreground py-2">Sem eventos.</p>
          ) : (
            <>
              <div className="flex items-baseline gap-2">
                <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
                  Eventos
                </span>
                <span className="text-[10px] font-mono text-muted-foreground/50">
                  {visible.length} / {filtered.length}
                </span>
              </div>
              <ul className="flex flex-col gap-0.5 text-sm">
                {visible.map((row, i) => {
                  const variant = eventVariant(row.event.event_type);
                  return (
                    <li
                      key={`${row.projectId}-${i}-${row.event.ts ?? ""}`}
                      className="flex items-baseline gap-2 px-2 py-1 rounded hover:bg-muted/40"
                    >
                      <StatusDot
                        variant={variant}
                        pulse={variant === "active"}
                        className="self-center"
                      />
                      <Badge variant="secondary" className="text-[10px] py-0 font-mono">
                        {row.event.event_type}
                      </Badge>
                      <span className="text-muted-foreground text-xs">{row.projectName}</span>
                      {row.event.ts && (
                        <span className="text-muted-foreground text-xs">
                          {relativeTime(row.event.ts)}
                        </span>
                      )}
                      {row.event.summary && (
                        <span className="text-muted-foreground text-xs">
                          — {truncate(row.event.summary, 200)}
                        </span>
                      )}
                    </li>
                  );
                })}
              </ul>
              {hasMore && (
                <button
                  type="button"
                  onClick={() => setVisibleCount((n) => n + PAGE_SIZE)}
                  className="self-start text-xs text-muted-foreground hover:text-foreground border border-border rounded px-2 py-1 mt-2"
                >
                  Carregar mais {PAGE_SIZE}
                </button>
              )}
            </>
          )}
        </>
      )}
    </div>
  );
}
