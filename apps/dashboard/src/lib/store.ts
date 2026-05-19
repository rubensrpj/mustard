import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';

type Store = {
  projectsRoot: string | null;
  selectedProjectId: string | null;
  activeWorkspaceId: string | null;
  knowledgeQuery: string;
  language: 'pt' | 'en';
  setProjectsRoot: (root: string | null) => void;
  setSelectedProjectId: (id: string | null) => void;
  setActiveWorkspaceId: (id: string | null) => void;
  clearActiveWorkspace: () => void;
  setKnowledgeQuery: (q: string) => void;
  setLanguage: (l: 'pt' | 'en') => void;
};

export const useStore = create<Store>()(
  persist(
    (set) => ({
      projectsRoot: null,
      selectedProjectId: null,
      activeWorkspaceId: null,
      knowledgeQuery: '',
      language: 'pt',
      setProjectsRoot: (root) => set({ projectsRoot: root }),
      setSelectedProjectId: (id) => set({ selectedProjectId: id }),
      setActiveWorkspaceId: (id) => set({ activeWorkspaceId: id }),
      clearActiveWorkspace: () => set({ activeWorkspaceId: null }),
      setKnowledgeQuery: (q) => set({ knowledgeQuery: q }),
      setLanguage: (l) => { set({ language: l }); i18n.changeLanguage(l); },
    }),
    {
      name: 'mustard-dashboard-store',
      onRehydrateStorage: () => (state) => { if (state?.language) i18n.changeLanguage(state.language); },
    }
  )
);
