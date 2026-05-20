<!-- mustard:generated at:2026-05-19 role:ui -->
# Examples — dashboard-use-queries-fanout

## useActivityFeed.ts (simplest fan-out, single resource)

```ts
// src/hooks/useActivityFeed.ts
import { useQueries } from "@tanstack/react-query";
import { fetchRecentEvents, type RecentEvent } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";

export interface ActivityFeedRow {
  projectId: string;
  projectName: string;
  event: RecentEvent;
}

function tsMs(s: string | null | undefined): number | null {
  if (!s) return null;
  const t = Date.parse(s);
  return Number.isFinite(t) ? t : null;
}

export function useActivityFeed(
  projects: Project[],
  limitPerProject: number,
): { events: ActivityFeedRow[]; types: string[]; loading: boolean } {
  const queries = useQueries({
    queries: projects.map((p) => ({
      queryKey: ["activity-feed", p.path, limitPerProject],
      queryFn: () => fetchRecentEvents(p.path, limitPerProject),
      staleTime: 5_000,
      refetchInterval: 5_000,
    })),
  });

  const loading = queries.some((q) => q.isLoading);
  const events: ActivityFeedRow[] = [];
  const typeSet = new Set<string>();

  projects.forEach((p, i) => {
    for (const event of queries[i]?.data ?? []) {
      events.push({ projectId: p.id, projectName: p.name, event });
      typeSet.add(event.event_type);
    }
  });

  events.sort((a, b) => (tsMs(b.event.ts) ?? 0) - (tsMs(a.event.ts) ?? 0));

  return { events, types: Array.from(typeSet).sort(), loading };
}
```

## useKnowledgeSearch.ts (fan-out with enabled gate)

```ts
// src/hooks/useKnowledgeSearch.ts — enabled only when query.trim().length >= 2
const queries = useQueries({
  queries: projects.map((p) => ({
    queryKey: ["knowledge-search", p.path, trimmed],
    queryFn: () => fetchSearchKnowledge(p.path, trimmed, 50),
    enabled,           // gate: skip API call until user has typed ≥2 chars
    staleTime: 60_000,
  })),
});
// Fan-in sorted by confidence (not timestamp)
results.sort((a, b) => b.row.confidence - a.row.confidence);
```

## useAggregate.ts (multi-resource fan-out)

```ts
// src/hooks/useAggregate.ts — two independent useQueries blocks, then fan-in both
const specsQueries = useQueries({
  queries: projects.map((p) => ({
    queryKey: ["specs", p.path],
    queryFn: () => fetchSpecs(p.path),
    staleTime: 30_000,
  })),
});
const eventsQueries = useQueries({
  queries: projects.map((p) => ({
    queryKey: ["recent-events", p.path, 10],
    queryFn: () => fetchRecentEvents(p.path, 10),
    staleTime: 15_000,
  })),
});
// Fan-in uses index alignment: projects[i] ↔ specsQueries[i]
projects.forEach((p, i) => {
  const specs = specsQueries[i]?.data ?? [];
  // ... accumulate counters and activePipelines
});
```
