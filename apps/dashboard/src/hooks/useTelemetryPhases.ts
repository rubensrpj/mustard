import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryPhases, type PhaseSummary } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryPhases(repoPath: string | null, timeRange: TimeRange) {
  return useQuery<PhaseSummary[]>({
    queryKey: ["telemetry-phases", repoPath, timeRange],
    queryFn: () => dashboardTelemetryPhases(repoPath as string, timeRange),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 5_000,
    refetchOnWindowFocus: true,
  });
}
