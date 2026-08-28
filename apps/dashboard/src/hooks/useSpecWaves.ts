import { useQuery } from "@tanstack/react-query";
import { dashboardSpecWaves, type SpecWave } from "@/lib/dashboard";

/**
 * Per-wave list for one spec.
 *
 * `enabled` is the caller's own gate, ANDed with the repo/spec guard. It exists
 * because a React component mounting is not the same thing as a component being
 * visible: `<details>` renders its children whether or not it is open, and the
 * browser merely hides them. So every collapsed activity row was mounting its
 * detail and firing this query — 200 rows meant 200 requests, repeated by
 * `refetchInterval` every 60 seconds, and the route took minutes to open.
 *
 * Defaults to `true` so a caller that really is always visible keeps the plain
 * two-argument call.
 */
export function useSpecWaves(
  repoPath: string | null,
  spec: string | null,
  enabled = true,
) {
  return useQuery<SpecWave[]>({
    queryKey: ["spec-waves", repoPath, spec],
    queryFn: () => dashboardSpecWaves(repoPath as string, spec as string),
    enabled: !!repoPath && !!spec && enabled,
    staleTime: 5_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}
