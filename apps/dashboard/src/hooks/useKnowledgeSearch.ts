import { useQueries } from "@tanstack/react-query";
import { fetchSearchKnowledge, type KnowledgeRow } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";

export interface KnowledgeHit {
  projectId: string;
  projectName: string;
  row: KnowledgeRow;
}

interface KnowledgeSearchResult {
  results: KnowledgeHit[];
  loading: boolean;
}

export function useKnowledgeSearch(
  projects: Project[],
  query: string,
): KnowledgeSearchResult {
  const trimmed = query.trim();
  const enabled = trimmed.length >= 2;

  const queries = useQueries({
    queries: projects.map((p) => ({
      queryKey: ["knowledge-search", p.path, trimmed],
      queryFn: () => fetchSearchKnowledge(p.path, trimmed, 50),
      enabled,
      staleTime: 60_000,
    })),
  });

  const loading = enabled && queries.some((q) => q.isLoading);

  const results: KnowledgeHit[] = [];
  if (enabled) {
    projects.forEach((p, i) => {
      const rows = queries[i]?.data ?? [];
      for (const row of rows) {
        results.push({ projectId: p.id, projectName: p.name, row });
      }
    });
    results.sort((a, b) => b.row.confidence - a.row.confidence);
  }

  return { results, loading };
}
