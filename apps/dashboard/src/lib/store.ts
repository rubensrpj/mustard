import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';

type Store = {
  projectsRoot: string | null;
  selectedProjectId: string | null;
  activeWorkspaceId: string | null;
  knowledgeQuery: string;
  // BCP-47 locale codes (see memory `project_locale_codes`).
  language: 'pt-BR' | 'en-US';
  // Main navigation sidebar collapsed to an icon rail (~56px) instead of the
  // full 220px tree. Persisted so the layout choice survives reloads.
  sidebarCollapsed: boolean;
  setProjectsRoot: (root: string | null) => void;
  setSelectedProjectId: (id: string | null) => void;
  setActiveWorkspaceId: (id: string | null) => void;
  clearActiveWorkspace: () => void;
  setKnowledgeQuery: (q: string) => void;
  setLanguage: (l: 'pt-BR' | 'en-US') => void;
  toggleSidebar: () => void;
};

export const useStore = create<Store>()(
  persist(
    (set) => ({
      projectsRoot: null,
      selectedProjectId: null,
      activeWorkspaceId: null,
      knowledgeQuery: '',
      language: 'pt-BR',
      sidebarCollapsed: false,
      setProjectsRoot: (root) => set({ projectsRoot: root }),
      setSelectedProjectId: (id) => set({ selectedProjectId: id }),
      setActiveWorkspaceId: (id) => set({ activeWorkspaceId: id }),
      clearActiveWorkspace: () => set({ activeWorkspaceId: null }),
      setKnowledgeQuery: (q) => set({ knowledgeQuery: q }),
      setLanguage: (l) => { set({ language: l }); i18n.changeLanguage(l); },
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
    }),
    {
      name: 'mustard-dashboard-store',
      onRehydrateStorage: () => (state) => { if (state?.language) i18n.changeLanguage(state.language); },
    }
  )
);
