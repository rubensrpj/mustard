import { useQuery } from "@tanstack/react-query";
import { dashboardEventsFeed, type FeedEvent } from "@/lib/dashboard";

/**
 * Most-recent feed events for the workspace, newest first. Wave 3 (2026-05-22):
 * lengthened the poll from 5s to a 60s fallback — this key has no dedicated
 * watcher kind, but window-focus refetch keeps it responsive on return.
 * Disabled when `repoPath` is null.
 */
export function useWorkspaceEventsFeed(repoPath: string | null, limit: number = 50) {
  return useQuery<FeedEvent[]>({
    queryKey: ["workspace-events-feed", repoPath, limit],
    queryFn: () => dashboardEventsFeed(repoPath as string, limit),
    enabled: !!repoPath,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });
}
