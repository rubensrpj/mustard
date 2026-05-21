// React Query hook for the W7 economy summary (spec
// 2026-05-20-economia-moat-unification — Wave 7). Single fetch entry point so
// every Economia component (header KPIs, per-agent table, savings breakdown)
// shares one query key and never double-invokes the backend.
//
// AC-4 contract: this file MUST reference `scope` literally so the spec's
// post-build check finds it. The hook key includes the full scope so a tab
// switch in `<ScopeBar>` re-fetches without invalidating sibling pages.

import { useQuery, type UseQueryResult } from "@tanstack/react-query";
import { fetchEconomySummary } from "@/lib/dashboard";
import type { EconomyScope, EconomySummary } from "@/lib/types/economy";

/**
 * Fetch the economy summary for `scope`. Pass `null` to disable the query
 * (e.g. while the workspace is still being resolved). Polls at the same 30s
 * cadence as `usePromptEconomy` / `useTelemetryPhases` so the Economia page
 * refreshes coherently across panels.
 */
export function useEconomySummary(
  scope: EconomyScope | null,
): UseQueryResult<EconomySummary> {
  return useQuery<EconomySummary>({
    // Stable key — JSON-stringify is fine for our discriminated union shape:
    // every variant has a small fixed field set with primitive values, no
    // ordering surprises across rerenders.
    queryKey: ["economy-summary", scope && stableScopeKey(scope)],
    queryFn: () => fetchEconomySummary(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 15_000,
    refetchInterval: 30_000,
    refetchOnWindowFocus: true,
  });
}

/**
 * Produce a deterministic cache key fragment for an economy scope. Sorts the
 * `projects` array in the `all_projects` variant so a reordered selection from
 * `<ScopeBar>` reuses the same query rather than refetching needlessly.
 */
function stableScopeKey(scope: EconomyScope): string {
  switch (scope.kind) {
    case "project":
      return `p:${scope.project}`;
    case "spec":
      return `s:${scope.project}|${scope.spec}`;
    case "wave":
      return `w:${scope.project}|${scope.spec}|${scope.wave}`;
    case "all_projects":
      return `a:${[...scope.projects].sort().join(",")}`;
  }
}
