import { useQuery } from "@tanstack/react-query";
import { dashboardSpecWaves, type SpecWave } from "@/lib/dashboard";

export function useSpecWaves(repoPath: string | null, spec: string | null) {
  return useQuery<SpecWave[]>({
    queryKey: ["spec-waves", repoPath, spec],
    queryFn: () => dashboardSpecWaves(repoPath as string, spec as string),
    enabled: !!repoPath && !!spec,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}
