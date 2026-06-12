import { create } from 'zustand';

/** Derive a display basename from a path with either separator. */
function basename(path: string): string {
  const norm = path.replace(/\\/g, '/').replace(/\/+$/, '');
  const i = norm.lastIndexOf('/');
  return i < 0 ? norm : norm.slice(i + 1);
}

/** One open file in the docked, IDE-style code viewer panel. `id` keys the tab
 *  per (project, file) so opening the same file twice just re-activates the
 *  existing tab instead of duplicating it. `relPath` is whatever the caller
 *  passed (repo-relative OR absolute — `dashboard_read_file` resolves both with
 *  containment); `fileName` is the tab/header label (basename when omitted). */
export interface OpenTab {
  id: string;
  repoPath: string;
  relPath: string;
  fileName: string;
}

interface CodeViewerStore {
  /** All open tabs, in insertion order (left → right in the tab bar). */
  tabs: OpenTab[];
  /** The active tab's `id`, or `null` when no file is open (panel hidden). */
  activeId: string | null;
  /** Open a file in the panel. If a tab with the same (repoPath, relPath)
   *  already exists it is just re-activated; otherwise a new tab is appended
   *  and activated. `fileName` falls back to the path's basename. */
  openFile: (repoPath: string, relPath: string, fileName?: string) => void;
  /** Close one tab; the neighbour (next, else previous) becomes active. Closing
   *  the last tab hides the panel (`activeId` → null). */
  closeTab: (id: string) => void;
  /** Close every tab and hide the panel. */
  closeAll: () => void;
  /** Make `id` the active tab (no-op when it is not open). */
  setActive: (id: string) => void;
}

/** Global store for the docked code viewer's open tabs. Ephemeral by design (no
 *  `persist`) — the open-files set is session state, not a user preference.
 *  Mirrors the zustand style of `lib/store.ts`. */
export const useCodeViewerStore = create<CodeViewerStore>((set) => ({
  tabs: [],
  activeId: null,
  openFile: (repoPath, relPath, fileName) => {
    if (!repoPath || !relPath) return;
    const id = `${repoPath}::${relPath}`;
    set((state) => {
      // Already open → just activate it (no duplicate tab).
      if (state.tabs.some((t) => t.id === id)) return { activeId: id };
      const tab: OpenTab = {
        id,
        repoPath,
        relPath,
        fileName: fileName ?? basename(relPath),
      };
      return { tabs: [...state.tabs, tab], activeId: id };
    });
  },
  closeTab: (id) =>
    set((state) => {
      const idx = state.tabs.findIndex((t) => t.id === id);
      if (idx < 0) return {};
      const tabs = state.tabs.filter((t) => t.id !== id);
      // Keep the active tab unless it is the one being closed; then fall to the
      // neighbour (the tab that shifts into this slot, else the previous one).
      let activeId = state.activeId;
      if (state.activeId === id) {
        const next = tabs[idx] ?? tabs[idx - 1] ?? null;
        activeId = next ? next.id : null;
      }
      return { tabs, activeId };
    }),
  closeAll: () => set({ tabs: [], activeId: null }),
  setActive: (id) =>
    set((state) => (state.tabs.some((t) => t.id === id) ? { activeId: id } : {})),
}));
