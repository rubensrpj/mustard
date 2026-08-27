import { call } from '@/lib/api-client';

export type Project = {
  id: string;
  name: string;
  path: string;
  last_activity_ms: number | null;
};

/**
 * Walk a tree looking for `mustard.json`. Omitting `root` scans from where the
 * server was started (or its `--root`): the projects shown are the ones on the
 * machine running the backend, so there is nothing for the browser to pick.
 */
export async function discoverProjects(root?: string): Promise<Project[]> {
  return call('discover_projects', { root });
}

/**
 * The directory `discoverProjects()` scans when given no root — surfaced so
 * the UI can name which machine's tree it is showing.
 */
export async function discoveryRoot(): Promise<string> {
  return call('dashboard_discovery_root');
}
