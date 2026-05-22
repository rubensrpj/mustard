import { useQuery } from "@tanstack/react-query";
import { dashboardSpecQuality, type SpecQualityItem } from "@/lib/dashboard";

export function useSpecQuality(repoPath: string | null, spec: string | null) {
  return useQuery<SpecQualityItem[]>({
    queryKey: ["spec-quality", repoPath, spec],
    queryFn: () => dashboardSpecQuality(repoPath as string, spec as string),
    enabled: !!repoPath && !!spec,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}
