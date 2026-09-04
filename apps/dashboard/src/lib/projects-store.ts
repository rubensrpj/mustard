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
// `loadFromServer` READS, it does not write. It used to fold the discovery scan
// back into the registry on every page load, re-registering everything the scan
// found — which quietly undid every removal, since a removed folder inside the
// scanned root simply came back on the next open. The scan still exists and the
// automatic writers (`mustard init`, the session observer) still register what
// they create; what changed is who wins when the operator and an automatic
// writer disagree. The operator's choice rides ON the row now (`hidden`), so
// the server can honour it against every writer, and the sidebar has one
// answer to render instead of a read followed by a burst of writes.
//
// Hidden rows are kept, not discarded: they are what the "put it back" gesture
// works from, and an absent row could not be told apart from a folder that was
// never registered. They are split off into `hiddenProjects` at load time
// rather than filtered in render, so the fan-out hooks (`useProjectDetections`,
// `useArtifactDrift`) keep selecting a stable array identity.
//
// Convention: select via slices (`useProjectsStore((s) => s.projects)`); the
// dashboard guards forbid full-store destructure (re-renders on every change).

import { create } from "zustand";
import {
  listRegisteredProjects,
  registerProject,
  hideProject,
  unhideProject,
  type ProjectEntry,
} from "@/lib/projects";
import { discoverProjects } from "@/api/discovery";
import { useStore } from "@/lib/store";

export type { ProjectEntry };

interface ProjectsState {
  /** The rows the sidebar draws — everything the operator has NOT hidden. */
  projects: ProjectEntry[];
  /** The rows the operator took off the list, so the UI can offer them back. */
  hiddenProjects: ProjectEntry[];
  hydrated: boolean;
  /** Pull the registry from the server. Rejects when the server cannot be
   *  reached — the caller decides how to report it. */
  loadFromServer: () => Promise<void>;
  /** Track `path` and make sure it is visible. Registering is what the manual
   *  "add folder" gesture asks for; unhiding is what it MEANS — the server
   *  keeps a hidden path hidden through `register` (so the automatic writers
   *  cannot undo a removal), so without the second call adding back a folder
   *  the operator had removed would silently do nothing. */
  addProject: (path: string) => Promise<void>;
  /** Take `path` off the list. The row is marked on the server, not dropped. */
  hideProject: (path: string) => Promise<void>;
  /** Put a previously hidden `path` back on the list. */
  unhideProject: (path: string) => Promise<void>;
  /** Mark the given registered project as the active workspace. Sets
   *  `projectsRoot=path` (the project folder doubles as discovery root so the
   *  legacy `useQuery(['discover', root])` flow returns exactly that project)
   *  and resolves the matching `activeWorkspaceId` through a discovery call. */
  activateProject: (path: string) => Promise<void>;
}

/** Split one server answer into the two slices the components select from. */
function partition(rows: ProjectEntry[]): {
  projects: ProjectEntry[];
  hiddenProjects: ProjectEntry[];
} {
  return {
    projects: rows.filter((row) => !row.hidden),
    hiddenProjects: rows.filter((row) => row.hidden),
  };
}

export const useProjectsStore = create<ProjectsState>()((set) => ({
  projects: [],
  hiddenProjects: [],
  hydrated: false,

  loadFromServer: async () => {
    set({ ...partition(await listRegisteredProjects()), hydrated: true });
  },

  addProject: async (path: string) => {
    await registerProject(path);
    set(partition(await unhideProject(path)));
  },

  hideProject: async (path: string) => {
    set(partition(await hideProject(path)));
  },

  unhideProject: async (path: string) => {
    set(partition(await unhideProject(path)));
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
