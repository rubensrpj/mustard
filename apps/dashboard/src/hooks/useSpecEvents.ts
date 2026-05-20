import { useQuery } from "@tanstack/react-query";
import { dashboardSpecEvents, type SpecTimelineEvent, type EventFilter } from "@/lib/dashboard";

export function useSpecEvents(
  repoPath: string | null,
  spec: string | null,
  filter?: EventFilter,
) {
  return useQuery<SpecTimelineEvent[]>({
    queryKey: ["spec-events", repoPath, spec, filter],
    queryFn: () => dashboardSpecEvents(repoPath as string, spec as string, filter),
    enabled: !!repoPath && !!spec,
    staleTime: 5_000,
  });
}
