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
