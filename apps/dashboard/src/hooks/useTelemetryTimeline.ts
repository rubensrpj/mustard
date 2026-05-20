import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryTimeline, type TimelineEvent } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryTimeline(
  repoPath: string | null,
  timeRange: TimeRange,
  limit = 50,
) {
  return useQuery<TimelineEvent[]>({
    queryKey: ["telemetry-timeline", repoPath, timeRange, limit],
    queryFn: () => dashboardTelemetryTimeline(repoPath as string, timeRange, limit),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 5_000,
    refetchOnWindowFocus: true,
  });
}
