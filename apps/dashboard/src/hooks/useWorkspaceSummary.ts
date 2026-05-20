import { useQueries, useQuery } from "@tanstack/react-query";
import { dashboardWorkspaceSummary, type WorkspaceSummary } from "@/lib/dashboard";
import type { Project } from "@/lib/dashboard";

/**
 * Fan-out per project — follows the same useQueries pattern used by
 * telemetry/knowledge hooks. Returns one result entry per project.
 */
export function useWorkspaceSummary(projects: Project[]) {
  return useQueries({
    queries: projects.map((p) => ({
      queryKey: ["workspace-summary", p.path] as const,
      queryFn: (): Promise<WorkspaceSummary> => dashboardWorkspaceSummary(p.path),
      enabled: !!p.path,
      staleTime: 5_000,
    })),
  });
}

/** Convenience: single-project variant using plain useQuery. */
export function useWorkspaceSummarySingle(repoPath: string | null) {
  return useQuery<WorkspaceSummary>({
    queryKey: ["workspace-summary", repoPath],
    queryFn: () => dashboardWorkspaceSummary(repoPath as string),
    enabled: !!repoPath,
    staleTime: 5_000,
  });
}
