// Project-registry zustand store (B6 Wave 1).
//
// Holds the user-curated list of projects shown in the dashboard sidebar.
// Persistence is delegated to `@tauri-apps/plugin-store` (file:
// `projects.json`, key `"projects"`) so the registry survives across desktop
// runs without touching browser localStorage — relevant because the dashboard
// also runs in `pnpm dev` (browser) where plugin-store may not be available;
// in that case all operations no-op gracefully (`loadFromStore` returns the
// already-in-memory empty list, `persist` swallows the error).
//
// Convention: select via slices (`useProjectsStore((s) => s.projects)`); the
// dashboard guards forbid full-store destructure (re-renders on every change).

import { create } from "zustand";
import { load, type Store as TauriStore } from "@tauri-apps/plugin-store";
import { discoverProjects } from "@/api/discovery";
import { useStore } from "@/lib/store";

const STORE_FILE = "projects.json";
const STORE_KEY = "projects";

export interface ProjectEntry {
  /** Absolute filesystem path. Doubles as the entry's identity. */
  path: string;
  /** Display label — defaults to the basename of `path`. */
  name: string;
  /** ISO timestamp the entry was added (UTC). */
  addedAt: string;
}

interface ProjectsState {
  projects: ProjectEntry[];
  hydrated: boolean;
  loadFromStore: () => Promise<void>;
  addProject: (path: string) => Promise<void>;
  removeProject: (path: string) => Promise<void>;
  /** Mark the given registered project as the active workspace. Sets
   *  `projectsRoot=path` (the project folder doubles as discovery root so the
   *  legacy `useQuery(['discover', root])` flow returns exactly that project)
   *  and resolves the matching `activeWorkspaceId` via a Rust discovery call.
   *  No-ops outside Tauri. */
  activateProject: (path: string) => Promise<void>;
}

let storeHandle: TauriStore | null = null;

/** Lazily resolve the plugin-store handle. Returns `null` outside Tauri
 *  (e.g. `pnpm dev` browser preview) so callers can no-op cleanly. */
async function getStore(): Promise<TauriStore | null> {
  if (storeHandle) return storeHandle;
  try {
    // Omit StoreOptions — the v2 type requires `defaults`, but we don't want
    // to inject placeholder keys; we explicitly call `save()` after every
    // mutation, so the default `autoSave` behaviour is harmless either way.
    storeHandle = await load(STORE_FILE);
    return storeHandle;
  } catch {
    return null;
  }
}

/** Extract the trailing path segment as a display name. Handles both
 *  forward and back slashes (Windows + POSIX) and trims trailing separators. */
function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const idx = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
}

/** Persist the current in-memory list to plugin-store. Swallows failures so
 *  the UI never breaks when running outside the Tauri shell. */
async function persistProjects(projects: ProjectEntry[]): Promise<void> {
  const handle = await getStore();
  if (!handle) return;
  try {
    await handle.set(STORE_KEY, projects);
    await handle.save();
  } catch {
    // Persistence is best-effort; the in-memory list is authoritative for the
    // current session.
  }
}

export const useProjectsStore = create<ProjectsState>()((set, get) => ({
  projects: [],
  hydrated: false,

  loadFromStore: async () => {
    const handle = await getStore();
    if (!handle) {
      set({ hydrated: true });
      return;
    }
    try {
      const raw = await handle.get<ProjectEntry[]>(STORE_KEY);
      const projects = Array.isArray(raw) ? raw : [];
      set({ projects, hydrated: true });
    } catch {
      set({ hydrated: true });
    }
  },

  addProject: async (path: string) => {
    const existing = get().projects;
    if (existing.some((p) => p.path === path)) {
      // Already registered — still activate so the user's directive ("ao
      // adicionar uma pasta ao projeto, deve carregar como workspace") holds
      // for re-adds.
      await get().activateProject(path);
      return;
    }
    const entry: ProjectEntry = {
      path,
      name: basename(path),
      addedAt: new Date().toISOString(),
    };
    const next = [...existing, entry];
    set({ projects: next });
    await persistProjects(next);
    // Bridge: a newly registered project becomes the active workspace so the
    // user lands inside it immediately, matching the directive above.
    await get().activateProject(path);
  },

  removeProject: async (path: string) => {
    const next = get().projects.filter((p) => p.path !== path);
    if (next.length === get().projects.length) return;
    set({ projects: next });
    await persistProjects(next);
  },

  activateProject: async (path: string) => {
    // The id used across the existing dashboard (Activity/Telemetry/Quality/
    // Knowledge/Home pages) is the FNV-1a hash of the canonical path produced
    // by the `discover_projects` Tauri command. Rather than re-implement that
    // hash on the JS side, we round-trip through discovery: setting
    // `projectsRoot=path` makes the discovery scan return exactly this folder,
    // and we then read its `id` for `activeWorkspaceId`. Outside Tauri the
    // invoke throws — we swallow it (the browser preview has no working
    // workspace pages anyway).
    const workspaceStore = useStore.getState();
    workspaceStore.setProjectsRoot(path);
    try {
      const discovered = await discoverProjects(path);
      const match = discovered.find((p) => p.path === path) ?? discovered[0];
      if (match) {
        workspaceStore.setActiveWorkspaceId(match.id);
      }
    } catch {
      // Outside Tauri or discovery failure — projectsRoot is already set so
      // the existing useQuery fan-out will retry on its own.
    }
  },
}));
