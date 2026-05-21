import { useQuery } from "@tanstack/react-query";
import { dashboardTelemetryPhases, type PhaseSummary } from "@/lib/dashboard";
import type { TimeRange } from "@/lib/types/telemetry";

export function useTelemetryPhases(repoPath: string | null, timeRange: TimeRange) {
  return useQuery<PhaseSummary[]>({
    queryKey: ["telemetry-phases", repoPath, timeRange],
    queryFn: () => dashboardTelemetryPhases(repoPath as string, timeRange),
    enabled: !!repoPath,
    staleTime: 5_000,
    // spec 2026-05-20-dashboard-ux-honest Wave 3: align polling cadence of the
    // Economia hooks (telemetry / promptEconomy / phases) at 30s.
    refetchInterval: 30_000,
    refetchOnWindowFocus: true,
  });
}
