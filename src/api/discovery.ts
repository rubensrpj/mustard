import { invoke } from '@tauri-apps/api/core';

export type Project = {
  id: string;
  name: string;
  path: string;
  db_path: string;
  last_activity_ms: number | null;
};

export async function discoverProjects(root: string): Promise<Project[]> {
  return invoke('discover_projects', { root });
}
