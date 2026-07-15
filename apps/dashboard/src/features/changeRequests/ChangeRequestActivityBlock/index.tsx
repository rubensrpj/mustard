/**
 * ChangeRequestActivityBlock — inline sub-block showing pipeline.change.request
 * events (mid-spec change requests) for a specific spec inside the drill-down.
 *
 * Follows the drill-down activity-block pattern (spec
 * 2026-05-20-session-bound-amendments, AC-15): query-key reuse,
 * chronological sort, null-when-empty contract.
 * Each request already arrives with a readable `summary` (built by the Rust
 * `event_summary()` case for "pipeline.change.request"), so this block renders
 * `e.summary` verbatim with no per-suffix label/icon map.
 *
 * Returns null when no change-request events exist for the spec — no empty state.
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useStore } from "@/lib/store";
import { useProjects, fetchRecentEvents, type RecentEvent } from "@/lib/dashboard";
import { relativeTime } from "@/lib/time";
import { cn } from "@/lib/utils";

// ── component ────────────────────────────────────────────────────────────────

export interface ChangeRequestActivityBlockProps {
  /** The spec name / id to filter events for. */
  specId: string;
}

export function ChangeRequestActivityBlock({ specId }: ChangeRequestActivityBlockProps) {
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const path = projects.find((p) => p.id === activeWorkspaceId)?.path ?? null;

  // Reuse the existing recent-events query key so it benefits from cache.
  const { data: allEvents } = useQuery({
    queryKey: ["recent-events-change-request", path],
    queryFn: () => fetchRecentEvents(path!, 500),
    enabled: !!path,
    staleTime: 15_000,
  });

  const changeRequestEvents = useMemo<RecentEvent[]>(() => {
    if (!allEvents) return [];
    return allEvents
      .filter(
        (e) =>
          e.spec === specId &&
          e.event_type === "pipeline.change.request",
      )
      // Sort chronologically (oldest first)
      .sort((a, b) => {
        if (!a.ts && !b.ts) return 0;
        if (!a.ts) return 1;
        if (!b.ts) return -1;
        return a.ts.localeCompare(b.ts);
      });
  }, [allEvents, specId]);

  // No change-request events → render nothing (per spec: component returns null)
  if (changeRequestEvents.length === 0) return null;

  return (
    <div
      className={cn(
        "ml-6 mt-2 border-t border-border/30 pt-2",
        "rounded-md px-3 py-2",
        // soft mustard background at 10% opacity via CSS custom prop fallback
      )}
      style={{
        backgroundColor: "color-mix(in srgb, var(--primary, #f5a623) 10%, transparent)",
      }}
    >
      <p className="text-[10px] uppercase tracking-wider text-muted-foreground/60 mb-1.5 font-medium">
        Solicitações
      </p>
      <ul className="flex flex-col gap-1">
        {changeRequestEvents.map((e, i) => (
          <li
            key={`change-request-${e.ts ?? ""}-${i}`}
            className="flex items-baseline gap-2 text-[12px] min-w-0"
          >
            <span
              className="shrink-0 font-mono text-[--primary] opacity-70"
              aria-hidden
            >
              ✎
            </span>
            <span className="text-foreground/80 truncate flex-1">
              {e.summary ?? "solicitação"}
            </span>
            {e.ts && (
              <span className="shrink-0 text-[11px] text-muted-foreground/50 tabular-nums">
                {relativeTime(e.ts)}
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
