import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { queryClient } from "./query-client";
import { onSpecsSnapshot } from "./dashboard";

export async function startWatcher(repoPaths: string[]): Promise<void> {
  await invoke("dashboard_watch_repos", { repoPaths });
}

/**
 * Apply one `dashboard:specs-snapshot` push (spec
 * `performance-dashboard-rotas-lentas-cache`, W3). The payload already carries
 * the rebuilt `dashboard_specs` + `dashboard_active_pipelines` projections, so
 * the data enters the cache via `setQueryData` — zero refetches, replacing the
 * old mass invalidation of the specs keys. The snapshot carries no sequence
 * number: application is last-write-wins in reception order, and the queries
 * keep their own staleTime / refetchInterval fallbacks to reconcile the
 * theoretical case of two overlapping rebuilds landing out of order.
 */
function subscribeSpecsSnapshot(): Promise<() => void> {
  return onSpecsSnapshot(({ repo_path, specs, active_pipelines }) => {
    queryClient.setQueryData(["specs", repo_path], specs);
    queryClient.setQueryData(["active-pipelines", repo_path], active_pipelines);
    // The snapshot does not carry the batch spec-cards; one invalidation per
    // burst keeps the Specs list live (the batch refetch is a single fold).
    queryClient.invalidateQueries({ queryKey: ["spec-cards", repo_path] });
  });
}

/**
 * Subscribe to both watcher channels: the aggregated specs-snapshot push
 * (applied via `setQueryData` above) and the `dashboard:fs-change`
 * compatibility channel, which now invalidates ONLY the keys derived from the
 * changed kind — the specs list / active-pipeline keys come exclusively from
 * the push. Resolves to one combined unlisten.
 */
export function subscribeFsChange(): Promise<() => void> {
  const fsChange = listen<{ repo_path: string; kind: string }>(
    "dashboard:fs-change",
    ({ payload }) => {
      const { repo_path, kind } = payload;
      if (kind === "events") {
        queryClient.invalidateQueries({ queryKey: ["recent-events", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["metrics", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["activity"] });
        queryClient.invalidateQueries({ queryKey: ["telemetry", repo_path] });
        // Per-spec timeline + sessions are rebuilt from the per-spec/.session
        // NDJSON event log, so a new `.events/*.ndjson` line is their refresh
        // trigger. `useSpecTimeline` keys as ["spec-timeline", repoPath, spec],
        // so the repo_path leaf scopes the refetch to the changed repo.
        queryClient.invalidateQueries({ queryKey: ["spec-timeline", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["sessions", repo_path] });
        // The session drill-in is the rich trace, keyed
        // ["trace", "session", repoPath, sessionId] (via `useTrace`) — so a new
        // `.session/{id}/.events/*.ndjson` line tails it live. The repo_path leaf
        // (3rd element) scopes the prefix-match refetch to the changed repo.
        queryClient.invalidateQueries({ queryKey: ["trace", "session", repo_path] });
        // Per-wave checklist progress folds `checklist.item.marked` events
        // (plus meta.json sidecars), so an event-log write refreshes it.
        queryClient.invalidateQueries({ queryKey: ["spec-checklist", repo_path] });
        // These activity views are derived from the same NDJSON event log, so
        // an event-log write is their trigger as well (their query keys don't
        // share the "activity" prefix above — they are distinct first elements).
        queryClient.invalidateQueries({ queryKey: ["activity-feed", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["activity-agg", repo_path] });
        // Knowledge is derived from the NDJSON event log too, so an event-log
        // write is also a knowledge change — invalidate those keys here so the
        // Knowledge page refreshes event-driven instead of polling.
        queryClient.invalidateQueries({ queryKey: ["knowledge-browse", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["knowledge-search", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["knowledge", repo_path] });
      } else if (kind === "pipeline-state") {
        // Legacy `.pipeline-states` channel — the only kind that does NOT
        // schedule a snapshot rebuild on the Rust side (the event log is
        // canonical for pipeline fields). Keep a pointwise invalidation as the
        // reconciliation fallback for the projections it used to refresh.
        queryClient.invalidateQueries({ queryKey: ["active-pipelines", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["pipelines", repo_path] });
      } else if (kind === "spec") {
        // The specs list itself arrives via the snapshot push (`spec` writes
        // schedule a rebuild on the Rust side) — only the markdown bodies and
        // the checklist fold, which the snapshot does not carry, refetch here.
        queryClient.invalidateQueries({ queryKey: ["spec-md", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["spec-markdown", repo_path] });
        // `meta.json#checklist` writes (seed / done flips) classify as `spec`
        // — they carry the per-wave totals the progress fold reads.
        queryClient.invalidateQueries({ queryKey: ["spec-checklist", repo_path] });
      } else if (kind === "knowledge") {
        queryClient.invalidateQueries({ queryKey: ["knowledge-browse", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["knowledge-search", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["knowledge", repo_path] });
      } else if (kind === "memory") {
        queryClient.invalidateQueries({ queryKey: ["activity-feed", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["activity-agg", repo_path] });
      }
    },
  );
  return Promise.all([fsChange, subscribeSpecsSnapshot()]).then(
    (unlistens) => () => {
      for (const unlisten of unlistens) unlisten();
    },
  );
}
