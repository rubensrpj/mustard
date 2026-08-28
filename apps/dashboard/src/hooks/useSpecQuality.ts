import { useQuery } from "@tanstack/react-query";
import { dashboardSpecQuality, type SpecQualityItem } from "@/lib/dashboard";

/**
 * Per-acceptance-criterion quality list for one spec.
 *
 * `enabled` is the caller's own gate, ANDed with the repo/spec guard — the twin
 * of the one on `useSpecWaves`, and for the same reason: a collapsed
 * `<details>` still MOUNTS its children, so the detail component fired this
 * query for every row on the activity page whether or not anyone had opened it.
 *
 * Defaults to `true` so a caller that really is always visible keeps the plain
 * two-argument call.
 */
export function useSpecQuality(
  repoPath: string | null,
  spec: string | null,
  enabled = true,
) {
  return useQuery<SpecQualityItem[]>({
    queryKey: ["spec-quality", repoPath, spec],
    queryFn: () => dashboardSpecQuality(repoPath as string, spec as string),
    enabled: !!repoPath && !!spec && enabled,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}
