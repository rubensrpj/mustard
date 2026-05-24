import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router";
import { useEffect } from "react";
import { useStore } from "@/lib/store";
import { queryClient } from "@/lib/query-client";
import { fetchSpecs, fetchSpecMarkdown } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";
import {
  Markdown,
  PageSurface,
  EditorialBand,
  SectionHeader,
  DataCard,
  EmptyState,
} from "@/components/page";

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
      <PageSurface>
        <EmptyState
          title="Projeto não encontrado"
          description={
            <>
              Volte ao{" "}
              <Link to="/" className="underline">Home</Link>.
            </>
          }
        />
      </PageSurface>
    );
  }

  const subtitleParts: string[] = [];
  if (row?.started_at) subtitleParts.push(`iniciado ${relativeTime(row.started_at)}`);
  if (row?.completed_at) subtitleParts.push(`concluído ${relativeTime(row.completed_at)}`);
  if (dataUpdatedAt > 0) subtitleParts.push(`atualizado ${relativeTime(new Date(dataUpdatedAt).toISOString())}`);
  const subtitle = subtitleParts.length > 0 ? subtitleParts.join(' · ') : undefined;

  return (
    <PageSurface>
      <EditorialBand
        eyebrow={
          <>
            Mustard /{" "}
            <Link to={`/project/${project.id}?tab=about`} className="hover:underline">
              {project.name}
            </Link>{" "}
            /{" "}
            <Link to={`/project/${project.id}?tab=specs`} className="hover:underline">
              Specs
            </Link>
          </>
        }
        title={specName}
        subtitle={subtitle}
        actions={
          <div className="flex items-center gap-2">
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
            <Link
              to={`/project/${project.id}?tab=specs`}
              className="text-[13px] text-muted-foreground hover:text-foreground border border-border rounded px-2 py-1 shrink-0"
            >
              ← Voltar para Specs
            </Link>
          </div>
        }
      />

      {mdLoading && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-16 bg-muted rounded animate-pulse" />
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

          <section className="flex flex-col gap-2">
            <SectionHeader title="Affected files" />
            {!row || row.affected_files.length === 0 ? (
              <p className="text-[13px] text-muted-foreground">Sem arquivos registrados.</p>
            ) : (
              <DataCard padded>
                <ul className="font-mono text-xs flex flex-col gap-0.5">
                  {row.affected_files.map((f) => (
                    <li key={f} className="text-muted-foreground">
                      {f}
                    </li>
                  ))}
                </ul>
              </DataCard>
            )}
          </section>
        </div>
      )}
    </PageSurface>
  );
}
