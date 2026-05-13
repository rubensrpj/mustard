import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Search, FolderGit2, ChevronRight, ChevronDown } from "lucide-react";
import { useStore } from "@/lib/store";
import { discoverProjects } from "@/api/discovery";
import { useKnowledgeSearch, type KnowledgeHit } from "@/hooks/useKnowledgeSearch";
import { Badge } from "@/components/ui/badge";

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

function formatConfidence(c: number): string {
  return `${Math.round(c * 100)}%`;
}

function hitKey(hit: KnowledgeHit): string {
  return `${hit.projectId}::${hit.row.id}`;
}

export function Knowledge() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const persistedQuery = useStore((s) => s.knowledgeQuery);
  const setKnowledgeQuery = useStore((s) => s.setKnowledgeQuery);

  const { data: projects } = useQuery({
    queryKey: ["discover", projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const [query, setQuery] = useState(persistedQuery);
  const [debouncedQuery, setDebouncedQuery] = useState(persistedQuery);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  useEffect(() => {
    const t = setTimeout(() => {
      setDebouncedQuery(query);
      setKnowledgeQuery(query);
    }, 300);
    return () => clearTimeout(t);
  }, [query, setKnowledgeQuery]);

  const { results, loading } = useKnowledgeSearch(projects ?? [], debouncedQuery);
  const trimmed = debouncedQuery.trim();
  const hasQuery = trimmed.length >= 2;

  function toggle(key: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <nav className="text-xs text-muted-foreground">
          Mustard / <span className="text-foreground">Knowledge</span>
        </nav>
        <h1 className="text-base font-medium">Knowledge cross-project</h1>
      </div>

      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Buscar patterns, conventions, lições…"
          className="w-full pl-9 pr-3 py-2 bg-card border border-border rounded text-sm outline-none placeholder:text-muted-foreground focus:border-primary"
        />
      </div>

      {!projectsRoot ? (
        <p className="text-xs text-muted-foreground">
          Configure o diretório de projetos em Settings.
        </p>
      ) : !hasQuery ? (
        <p className="text-xs text-muted-foreground">Digite ≥2 caracteres para buscar.</p>
      ) : loading ? (
        <ul className="flex flex-col gap-1">
          {[0, 1, 2].map((i) => (
            <li key={i} className="h-8 bg-muted/40 rounded animate-pulse" />
          ))}
        </ul>
      ) : results.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          Nenhum resultado para "{trimmed}".
        </p>
      ) : (
        <>
          <div className="flex items-baseline gap-2">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Resultados
            </span>
            <span className="text-[10px] font-mono text-muted-foreground/50">
              {results.length}
            </span>
          </div>
          <ul className="flex flex-col gap-0.5 text-sm">
            {results.map((hit) => {
              const key = hitKey(hit);
              const isOpen = expanded.has(key);
              const Chevron = isOpen ? ChevronDown : ChevronRight;
              return (
                <li
                  key={key}
                  className="flex flex-col px-2 py-1 rounded hover:bg-muted/40"
                >
                  <button
                    type="button"
                    onClick={() => toggle(key)}
                    className="flex items-baseline gap-2 flex-wrap text-left cursor-pointer w-full"
                  >
                    <Chevron className="h-3 w-3 text-muted-foreground self-center shrink-0" />
                    <Badge variant="secondary" className="text-[10px] py-0">
                      {hit.row.type}
                    </Badge>
                    <span className="font-mono font-medium">{hit.row.name}</span>
                    <span className="text-muted-foreground text-xs flex items-center gap-1">
                      <FolderGit2 className="h-3 w-3" />
                      {hit.projectName}
                    </span>
                    <span className="text-muted-foreground text-xs font-mono">
                      {formatConfidence(hit.row.confidence)}
                    </span>
                    {!isOpen && hit.row.description && (
                      <span className="text-muted-foreground text-xs basis-full pl-5">
                        {truncate(hit.row.description, 120)}
                      </span>
                    )}
                  </button>
                  {isOpen && (
                    <div className="pl-5 pt-1 pb-2 flex flex-col gap-1 text-xs">
                      {hit.row.description && (
                        <p className="text-foreground whitespace-pre-wrap">
                          {hit.row.description}
                        </p>
                      )}
                      <dl className="flex flex-wrap gap-x-4 gap-y-0.5 text-muted-foreground">
                        <div className="flex gap-1">
                          <dt>id:</dt>
                          <dd className="font-mono text-foreground">{hit.row.id}</dd>
                        </div>
                        {hit.row.source && (
                          <div className="flex gap-1">
                            <dt>source:</dt>
                            <dd className="font-mono text-foreground">{hit.row.source}</dd>
                          </div>
                        )}
                        <div className="flex gap-1">
                          <dt>projeto:</dt>
                          <dd className="font-mono text-foreground">{hit.projectName}</dd>
                        </div>
                      </dl>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
