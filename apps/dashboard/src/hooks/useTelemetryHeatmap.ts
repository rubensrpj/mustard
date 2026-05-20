import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryHeatmap, type HeatmapCell } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryHeatmap(repoPath: string | null, timeRange: TimeRange) {
  return useQuery<HeatmapCell[]>({
    queryKey: ["telemetry-heatmap", repoPath, timeRange],
    queryFn: () => dashboardTelemetryHeatmap(repoPath as string, timeRange),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 5_000,
    refetchOnWindowFocus: true,
  });
}
