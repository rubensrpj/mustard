import { useQuery } from "@tanstack/react-query";
import { dashboardSpecCard, type SpecCard } from "@/lib/dashboard";

// Wave 3 (2026-05-22): the spec drill-down has no dedicated watcher kind, so
// it keeps a poll — but lengthened from 5s to a 60s fallback. spec-card is also
// invalidated on mutations (useSpecAction), so the fallback only covers the
// rarer case of an EXECUTE counter advancing with no user action.
// `refetchIntervalInBackground: false` pauses polling when the window is hidden.
export function useSpecCard(repoPath: string | null, spec: string | null) {
  return useQuery<SpecCard>({
    queryKey: ["spec-card", repoPath, spec],
    queryFn: () => dashboardSpecCard(repoPath as string, spec as string),
    enabled: !!repoPath && !!spec,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}
