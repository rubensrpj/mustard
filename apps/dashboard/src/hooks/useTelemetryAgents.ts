import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryAgents, type AgentDispatch } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryAgents(repoPath: string | null, timeRange: TimeRange) {
  return useQuery<AgentDispatch[]>({
    queryKey: ["telemetry-agents", repoPath, timeRange],
    queryFn: () => dashboardTelemetryAgents(repoPath as string, timeRange),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });
}
