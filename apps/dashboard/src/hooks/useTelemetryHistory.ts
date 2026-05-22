import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryHistory, type HistoryEntry } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryHistory(
  repoPath: string | null,
  timeRange: TimeRange,
  limit = 50,
) {
  return useQuery<HistoryEntry[]>({
    queryKey: ["telemetry-history", repoPath, timeRange, limit],
    queryFn: () => dashboardTelemetryHistory(repoPath as string, timeRange, limit),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });
}
