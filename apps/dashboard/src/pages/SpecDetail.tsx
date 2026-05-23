import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router";
import { useEffect } from "react";
import { useStore } from "@/lib/store";
import { queryClient } from "@/lib/query-client";
import { fetchSpecs, fetchSpecMarkdown } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";
import { Markdown } from "@/components/page/Markdown";

export function SpecDetail() {
  const { id, specName: rawSpecName } = useParams<{ id: string; specName: string }>();
  const specName = rawSpecName ? decodeURIComponent(rawSpecName) : "";
  const projectsRoot = useStore((s) => s.projectsRoot);
  const setSelectedProjectId = useStore((s) => s.setSelectedProjectId);
  const projects = queryClient.getQueryData<Project[]>(["discover", projectsRoot]) ?? [];
  const project = projects.find((p) => p.id === id) ?? null;

  useEffect(() => {
    if (id) setSelectedProjectId(id);
  }, [id, setSelectedProjectId]);

  const { data: specs } = useQuery({
    queryKey: ["specs", project?.path],
    queryFn: () => fetchSpecs(project!.path),
    enabled: !!project,
    staleTime: 30_000,
  });
  const row = specs?.find((s) => s.name === specName) ?? null;

  const {
    data: markdown,
    isLoading: mdLoading,
    error: mdError,
    dataUpdatedAt,
  } = useQuery({
    queryKey: ["spec-markdown", project?.path, specName],
    queryFn: () => fetchSpecMarkdown(project!.path, specName),
    enabled: !!project && !!specName,
    staleTime: 10_000,
    refetchInterval: 30_000,
  });

  if (!project) {
    return (
      <div className="text-sm text-muted-foreground">
        Projeto não encontrado. Volte ao{" "}
        <Link to="/" className="underline">
          Home
        </Link>
        .
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-start justify-between gap-2 mb-4">
        <div className="flex flex-col gap-1 min-w-0">
          <nav className="text-[13px] text-muted-foreground">
            Mustard / Projetos /{" "}
            <Link to={`/project/${project.id}?tab=about`} className="hover:underline">
              {project.name}
            </Link>{" "}
            /{" "}
            <Link to={`/project/${project.id}?tab=specs`} className="hover:underline">
              Specs
            </Link>{" "}
            / <span className="text-foreground">{specName}</span>
          </nav>
          <h1 className="text-base font-medium font-mono break-all">{specName}</h1>
          <div className="flex items-center gap-2 flex-wrap">
            {row?.phase && (
              <Badge variant="secondary" className="text-[11px] py-0">
                {row.phase}
              </Badge>
            )}
            {row?.status && (
              <Badge variant="outline" className="text-[11px] py-0">
                {row.status}
              </Badge>
            )}
            {row?.started_at && (
              <span className="text-[13px] text-muted-foreground">
                started {relativeTime(row.started_at)}
              </span>
            )}
            {row?.completed_at && (
              <span className="text-[13px] text-muted-foreground">
                completed {relativeTime(row.completed_at)}
              </span>
            )}
            {dataUpdatedAt > 0 && (
              <span className="text-[10px] text-muted-foreground">
                Atualizado {relativeTime(new Date(dataUpdatedAt).toISOString())}
              </span>
            )}
          </div>
        </div>
        <Link
          to={`/project/${project.id}?tab=specs`}
          className="text-[13px] text-muted-foreground hover:text-foreground border border-border rounded px-2 py-1 shrink-0"
        >
          ← Voltar para Specs
        </Link>
      </div>

      {mdLoading && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-16 bg-muted/40 rounded animate-pulse" />
          ))}
        </div>
      )}

      {mdError && (
        <p className="text-destructive text-sm">{(mdError as Error).message}</p>
      )}

      {markdown && (
        <div className="flex flex-col gap-6">
          <section>
            <Markdown content={markdown} />
          </section>

          <section>
            <h2 className="text-xs uppercase tracking-wider font-medium text-foreground mb-2">
              Affected files
            </h2>
            {!row || row.affected_files.length === 0 ? (
              <p className="text-[13px] text-muted-foreground">Sem arquivos registrados.</p>
            ) : (
              <ul className="font-mono text-xs flex flex-col gap-0.5">
                {row.affected_files.map((f) => (
                  <li key={f} className="text-muted-foreground">
                    {f}
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
