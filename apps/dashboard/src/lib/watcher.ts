import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { queryClient } from "./query-client";

export async function startWatcher(repoPaths: string[]): Promise<void> {
  await invoke("dashboard_watch_repos", { repoPaths });
}

export function subscribeFsChange(): Promise<() => void> {
  return listen<{ repo_path: string; kind: string }>(
    "dashboard:fs-change",
    ({ payload }) => {
      const { repo_path, kind } = payload;
      if (kind === "events") {
        queryClient.invalidateQueries({ queryKey: ["recent-events", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["metrics", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["activity"] });
        queryClient.invalidateQueries({ queryKey: ["telemetry", repo_path] });
        // W5 (T5.3 / T5.4): per-spec timeline + sessions are written into
        // mustard.db by the harness, so a DB write is their refresh trigger.
        // Keeping these keys broad (no repo_path leaf) so any spec timeline
        // currently mounted picks the new rows up without a remount.
        queryClient.invalidateQueries({ queryKey: ["spec-timeline"] });
        queryClient.invalidateQueries({ queryKey: ["sessions", repo_path] });
        // These activity views are all derived from mustard.db too, so a DB
        // write is their trigger as well (their query keys don't share the
        // "activity" prefix above — they are distinct first elements).
        queryClient.invalidateQueries({ queryKey: ["activity-feed", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["activity-agg", repo_path] });
        // Knowledge lives in mustard.db (knowledge_patterns / memory_decisions),
        // so a DB write is also a knowledge change — invalidate those keys here
        // so the Knowledge page refreshes event-driven instead of polling.
        queryClient.invalidateQueries({ queryKey: ["knowledge-browse", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["knowledge-search", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["knowledge", repo_path] });
      } else if (kind === "pipeline-state") {
        queryClient.invalidateQueries({ queryKey: ["active-pipelines", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["specs", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["pipelines", repo_path] });
      } else if (kind === "spec") {
        queryClient.invalidateQueries({ queryKey: ["specs", repo_path] });
        queryClient.invalidateQueries({ queryKey: ["spec-md", repo_path] });
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
}
