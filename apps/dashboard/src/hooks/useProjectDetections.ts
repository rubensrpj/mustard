// Per-project Mustard-installation detection fan-out (B6 Wave 2).
//
// One TanStack Query per registered project, keyed by `project.path` so the
// cache identity follows the project, not the array index. Returns a 1:1
// aligned array with `projects` so callers can `projects.forEach((p, i) =>
// detections[i])` without re-zipping. Follows the dashboard's canonical
// `useQueries` fan-out convention.

import { useQueries } from "@tanstack/react-query";
import { detectProjectMustard, type ProjectDetection } from "@/lib/projects";
import {
  useProjectsStore,
  type ProjectEntry,
} from "@/lib/projects-store";

export interface ProjectDetectionRow {
  project: ProjectEntry;
  detection: ProjectDetection | undefined;
  isLoading: boolean;
  error: unknown;
}

export function useProjectDetections(): ProjectDetectionRow[] {
  const projects = useProjectsStore((s) => s.projects);

  const queries = useQueries({
    queries: projects.map((p) => ({
      queryKey: ["project-detection", p.path],
      queryFn: () => detectProjectMustard(p.path),
      staleTime: 30_000,
      refetchOnMount: true,
    })),
  });

  return projects.map((project, i) => {
    const q = queries[i];
    return {
      project,
      detection: q?.data,
      isLoading: q?.isLoading ?? false,
      error: q?.error,
    };
  });
}
