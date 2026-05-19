import { useState, useEffect } from "react";
import { fetchSubprojects, fetchRecipes, fetchSkills, fetchRecentEvents } from "@/lib/dashboard";
import type { SubprojectInfo, RecipeMeta, SkillMeta, RecentEvent } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";

interface ProjectState {
  subprojects: SubprojectInfo[] | null;
  recipes: RecipeMeta[] | null;
  skills: SkillMeta[] | null;
  recentEvents: RecentEvent[] | null;
  loading: boolean;
  error: string | null;
}

export function useProject(project: Project | null): ProjectState {
  const [subprojects, setSubprojects] = useState<SubprojectInfo[] | null>(null);
  const [recipes, setRecipes] = useState<RecipeMeta[] | null>(null);
  const [skills, setSkills] = useState<SkillMeta[] | null>(null);
  const [recentEvents, setRecentEvents] = useState<RecentEvent[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!project) {
      setSubprojects(null);
      setRecipes(null);
      setSkills(null);
      setRecentEvents(null);
      setLoading(false);
      setError('Sem projeto selecionado');
      return;
    }
    setLoading(true);
    setError(null);
    Promise.all([
      fetchSubprojects(project.path),
      fetchRecipes(project.path),
      fetchSkills(project.path),
      fetchRecentEvents(project.path, 20),
    ])
      .then(([sp, r, sk, ev]) => {
        setSubprojects(sp);
        setRecipes(r);
        setSkills(sk);
        setRecentEvents(ev);
      })
      .catch((e: unknown) => {
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        setLoading(false);
      });
  }, [project?.path]);

  return { subprojects, recipes, skills, recentEvents, loading, error };
}
