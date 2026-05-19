<!-- mustard:generated at:2026-05-13 role:ui -->
# Examples — mustard-dashboard-use-queries-hook-pattern

## src/hooks/useActivityFeed.ts (lines 1-40)

```ts
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
      refetchInterval: 5_000,
      refetchOnWindowFocus: true,
    })),
  });

  const loading = queries.some((q) => q.isLoading);

  const events: ActivityFeedRow[] = [];
  const typeSet = new Set<string>();
```

## Notes

- `tsMs` is duplicated across `useActivityFeed.ts` and `useAggregate.ts` — copy it locally rather than introducing a shared util unless you bring in 3+ call sites.
- `queryKey` first slot is the resource slug; second slot is `project.path`; later slots are extra args. Keep this order.
- For conditional fetches (`useKnowledgeSearch.ts`), set `enabled: <predicate>` on each per-project query AND guard the fan-in loop with the same predicate so loading flips off correctly.
