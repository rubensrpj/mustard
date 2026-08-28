import { useQuery } from "@tanstack/react-query";
import { fetchGitInfo, type GitInfo } from "@/lib/dashboard";

/**
 * Local git state (remote, branch, ahead/behind, last commit) for a single
 * workspace. Backed by the `dashboard_git_info` backend command, which is
 * fail-open — a non-repo / no-remote path resolves to an empty `GitInfo`
 * (`is_repo: false`), so callers render an empty state rather than catch.
 *
 * Disabled when `repoPath` is null so the hook is safe to mount before a
 * workspace is selected. `repoPath` sits at the queryKey leaf so the cache
 * keys per project.
 */
export function useGitInfo(repoPath: string | null) {
  return useQuery<GitInfo>({
    queryKey: ["git-info", repoPath],
    queryFn: () => fetchGitInfo(repoPath as string),
    enabled: !!repoPath,
    staleTime: 30_000,
  });
}
