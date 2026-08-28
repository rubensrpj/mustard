// Project-registry zustand store (B6 Wave 1).
//
// Holds the list of projects shown in the dashboard sidebar. The list itself
// lives on the SERVER (`@/lib/projects` → `~/.claude/dashboard-projects.json`),
// not in this store and not in browser storage: the dashboard covers every
// Mustard project on the machine, so the registry is a fact about the machine.
// Opened from a second browser — or from a phone over the network — a
// browser-held list would come back empty, and neither view would be the truth.
// This store is the in-memory mirror the components render from.
//
// There is no folder picker any more, so nothing feeds the registry by hand:
// `loadFromServer` folds the discovery scan into it, and the scan walks the
// tree the server was started in. A project that appears on disk therefore
// appears in the sidebar on the next load, with no gesture to perform — which
// is the whole point, since there is no longer a button to add one.
//
// The consequence is deliberate: `removeProject` drops a registry entry, and a
// project the scan still finds comes back on the next load. Removal is how you
// clear an entry the scan can NOT re-derive — one outside the scan root, or one
// whose `mustard.json` is gone.
//
// Convention: select via slices (`useProjectsStore((s) => s.projects)`); the
// dashboard guards forbid full-store destructure (re-renders on every change).

import { create } from "zustand";
import {
  listRegisteredProjects,
  registerProject,
  unregisterProject,
  type ProjectEntry,
} from "@/lib/projects";
import { discoverProjects } from "@/api/discovery";
import { useStore } from "@/lib/store";

export type { ProjectEntry };

interface ProjectsState {
  projects: ProjectEntry[];
  hydrated: boolean;
  /** Pull the registry from the server with the discovery scan folded in.
   *  Rejects when the server cannot be reached — the caller decides how to
   *  report it. */
  loadFromServer: () => Promise<void>;
  removeProject: (path: string) => Promise<void>;
  /** Mark the given registered project as the active workspace. Sets
   *  `projectsRoot=path` (the project folder doubles as discovery root so the
   *  legacy `useQuery(['discover', root])` flow returns exactly that project)
   *  and resolves the matching `activeWorkspaceId` through a discovery call. */
  activateProject: (path: string) => Promise<void>;
}

export const useProjectsStore = create<ProjectsState>()((set) => ({
  projects: [],
  hydrated: false,

  loadFromServer: async () => {
    let projects = await listRegisteredProjects();
    // Fold in what is actually on the machine. The scan runs from where the
    // server was started, so the projects it finds are the projects of the
    // machine holding the `.claude/` trees. Registering a known path is a
    // no-op on the server, so a settled registry costs reads, not writes.
    const known = new Set(projects.map((p) => p.path));
    const discovered = await discoverProjects();
    for (const project of discovered) {
      if (known.has(project.path)) continue;
      projects = await registerProject(project.path);
    }
    set({ projects, hydrated: true });
  },

  removeProject: async (path: string) => {
    set({ projects: await unregisterProject(path) });
  },

  activateProject: async (path: string) => {
    // The id used across the existing dashboard (Activity/Telemetry/Quality/
    // Knowledge/Home pages) is the FNV-1a hash of the canonical path produced
    // by the `discover_projects` command. Rather than re-implement that hash
    // on the JS side, we round-trip through discovery: setting
    // `projectsRoot=path` makes the discovery scan return exactly this folder,
    // and we then read its `id` for `activeWorkspaceId`.
    const workspaceStore = useStore.getState();
    workspaceStore.setProjectsRoot(path);
    try {
      const discovered = await discoverProjects(path);
      const match = discovered.find((p) => p.path === path) ?? discovered[0];
      if (match) {
        workspaceStore.setActiveWorkspaceId(match.id);
      }
    } catch (e) {
      // `projectsRoot` is already set, so the page fan-out proceeds and the
      // `['discover', root]` query retries on its own — but the failure is
      // reported rather than swallowed, because the server being unreachable
      // is now a real fault instead of the expected browser case.
      console.error("activateProject: discovery failed", e);
    }
  },
}));
