// React Query wrapper over the Tauri trace commands — ONE hook for both the
// spec trace (`dashboard_spec_trace`) and the session trace
// (`dashboard_session_trace`). The two backends return the identical
// `TraceNode` shape (one shared `build_trace_tree` on the Rust side), so the
// frontend keeps a single hook + a single `<ExecutionTrace>` renderer rather
// than a parallel session view.
//
// The query key includes the source kind + projectPath + the per-kind leaf
// (specName | sessionId) so switching workspaces, specs or sessions doesn't
// reuse a stale tree. `enabled` guards the common pre-mount case where any
// input is null.

import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { TraceNode } from "@/lib/types/trace";

/** What the trace is rooted at — a spec or a session. The `<ExecutionTrace>`
 *  call sites pass one of these; the hook routes to the matching command. */
export type TraceSource =
  | { kind: "spec"; specName: string }
  | { kind: "session"; sessionId: string };

/** The per-kind leaf used in the query key + the `enabled` guard. */
function sourceLeaf(source: TraceSource | null): string | undefined {
  if (!source) return undefined;
  return source.kind === "spec" ? source.specName : source.sessionId;
}

export function useTrace(projectPath: string | null, source: TraceSource | null) {
  const leaf = sourceLeaf(source);
  return useQuery<TraceNode>({
    queryKey: ["trace", source?.kind, projectPath, leaf] as const,
    queryFn: () =>
      source?.kind === "spec"
        ? invoke<TraceNode>("dashboard_spec_trace", {
            projectPath: projectPath as string,
            specName: source.specName,
          })
        : invoke<TraceNode>("dashboard_session_trace", {
            projectPath: projectPath as string,
            sessionId: (source as { sessionId: string }).sessionId,
          }),
    enabled: !!projectPath && !!source && !!leaf,
    // Trees are append-mostly and have no watcher kind; a 60s fallback poll
    // keeps them eventually-fresh without thrashing.
    staleTime: 10_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}
