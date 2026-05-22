import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryEffort, type EffortBreakdown } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryEffort(repoPath: string | null, timeRange: TimeRange) {
  return useQuery<EffortBreakdown>({
    queryKey: ["telemetry-effort", repoPath, timeRange],
    queryFn: () => dashboardTelemetryEffort(repoPath as string, timeRange),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });
}
