import { useQueries } from "@tanstack/react-query";
import { fetchRecentEvents, type RecentEvent } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";

export interface ActivityFeedRow {
  projectId: string;
  projectName: string;
  event: RecentEvent;
}

interface ActivityFeedResult {
  events: ActivityFeedRow[];
  types: string[];
  loading: boolean;
}

function tsMs(s: string | null | undefined): number | null {
  if (!s) return null;
  const t = Date.parse(s);
  return Number.isFinite(t) ? t : null;
}

export function useActivityFeed(
  projects: Project[],
  limitPerProject: number,
): ActivityFeedResult {
  const queries = useQueries({
    queries: projects.map((p) => ({
      queryKey: ["activity-feed", p.path, limitPerProject],
      queryFn: () => fetchRecentEvents(p.path, limitPerProject),
      staleTime: 5_000,
      // Wave 3 (2026-05-22): watcher-driven via "events" — poll removed.
      refetchOnWindowFocus: true,
    })),
  });

  const loading = queries.some((q) => q.isLoading);

  const events: ActivityFeedRow[] = [];
  const typeSet = new Set<string>();

  projects.forEach((p, i) => {
    const rows = queries[i]?.data ?? [];
    for (const event of rows) {
      events.push({ projectId: p.id, projectName: p.name, event });
      typeSet.add(event.event_type);
    }
  });

  events.sort((a, b) => {
    const aT = tsMs(a.event.ts) ?? 0;
    const bT = tsMs(b.event.ts) ?? 0;
    return bT - aT;
  });

  return {
    events,
    types: Array.from(typeSet).sort(),
    loading,
  };
}
