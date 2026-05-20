import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryCriteria, type AcceptanceCriterion } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryCriteria(repoPath: string | null, timeRange: TimeRange) {
  return useQuery<AcceptanceCriterion[]>({
    queryKey: ["telemetry-criteria", repoPath, timeRange],
    queryFn: () => dashboardTelemetryCriteria(repoPath as string, timeRange),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 5_000,
    refetchOnWindowFocus: true,
  });
}
