import { useQuery } from "@tanstack/react-query";
import { fetchProjectOverview, type ProjectOverview } from "@/lib/dashboard";

/**
 * Grain-model project overview (monorepo flag, project count, languages,
 * frameworks, detected stacks) for a single workspace. Backed by the
 * `dashboard_project_overview` backend command, which is fail-open — a
 * missing/unscanned `.claude/grain.model.json` resolves to an all-empty
 * overview, so callers render an empty state rather than catch.
 *
 * Disabled when `repoPath` is null so the hook is safe to mount before a
 * workspace is selected. `repoPath` sits at the queryKey leaf so the cache
 * keys per project.
 */
export function useProjectOverview(repoPath: string | null) {
  return useQuery<ProjectOverview>({
    queryKey: ["project-overview", repoPath],
    queryFn: () => fetchProjectOverview(repoPath as string),
    enabled: !!repoPath,
    staleTime: 60_000,
  });
}
