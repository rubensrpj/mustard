// React Query wrapper over the Tauri `dashboard_spec_trace` command.
// The query key includes both projectPath and specName so switching
// workspaces or specs doesn't reuse a stale tree. `enabled` guards
// against the common pre-mount case where either input is null.

import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { TraceNode } from "@/lib/types/trace";

export function useSpecTrace(
  projectPath: string | null,
  specName: string | null,
) {
  return useQuery<TraceNode>({
    queryKey: ["spec-trace", projectPath, specName] as const,
    queryFn: () =>
      invoke<TraceNode>("dashboard_spec_trace", {
        projectPath: projectPath as string,
        specName: specName as string,
      }),
    enabled: !!projectPath && !!specName,
    // Trees are append-mostly: revalidate often enough to feel live, but
    // not so often that scrolling a deep tree thrashes. 10s matches the
    // existing `useSpecWaves` cadence.
    staleTime: 10_000,
    refetchInterval: 10_000,
    refetchIntervalInBackground: false,
  });
}
