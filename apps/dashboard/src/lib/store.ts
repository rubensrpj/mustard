import { create } from 'zustand';
import { persist } from 'zustand/middleware';

type Store = {
  projectsRoot: string | null;
  selectedProjectId: string | null;
  knowledgeQuery: string;
  setProjectsRoot: (root: string | null) => void;
  setSelectedProjectId: (id: string | null) => void;
  setKnowledgeQuery: (q: string) => void;
};

export const useStore = create<Store>()(
  persist(
    (set) => ({
      projectsRoot: null,
      selectedProjectId: null,
      knowledgeQuery: '',
      setProjectsRoot: (root) => set({ projectsRoot: root }),
      setSelectedProjectId: (id) => set({ selectedProjectId: id }),
      setKnowledgeQuery: (q) => set({ knowledgeQuery: q }),
    }),
    { name: 'mustard-dashboard-store' }
  )
);
