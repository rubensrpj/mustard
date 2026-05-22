import { useQuery } from "@tanstack/react-query";
import { dashboardSpecTimeline, type SpecTimelineNode } from "@/lib/dashboard";

export function useSpecTimeline(repoPath: string | null, spec: string | null) {
  return useQuery<SpecTimelineNode[]>({
    queryKey: ["spec-timeline", repoPath, spec],
    queryFn: () => dashboardSpecTimeline(repoPath as string, spec as string),
    enabled: !!repoPath && !!spec,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}
