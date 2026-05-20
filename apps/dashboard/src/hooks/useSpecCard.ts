import { useQuery } from "@tanstack/react-query";
import { dashboardSpecCard, type SpecCard } from "@/lib/dashboard";

// Wave 5 (2026-05-20): the drill-down view polls every 5 seconds while a
// pipeline is running. Without this, the user had to click away and back to
// see EXECUTE counters update. `refetchIntervalInBackground: false` keeps
// the cost down — when the dashboard window is minimised, polling pauses.
export function useSpecCard(repoPath: string | null, spec: string | null) {
  return useQuery<SpecCard>({
    queryKey: ["spec-card", repoPath, spec],
    queryFn: () => dashboardSpecCard(repoPath as string, spec as string),
    enabled: !!repoPath && !!spec,
    staleTime: 5_000,
    refetchInterval: 5_000,
    refetchIntervalInBackground: false,
  });
}
