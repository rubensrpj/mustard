import { Link, useNavigate } from "react-router";
import { useQuery, useQueries } from "@tanstack/react-query";
import { FolderGit2 } from "lucide-react";
import {
  StatusDot,
  PageSurface,
  EditorialBand,
  DataCard,
  DataRow,
  SectionHeader,
  EmptyState,
} from "@/components/page";
import { useStore } from "@/lib/store";
import { discoverProjects } from "@/api/discovery";
import type { Project as DiscoveryProject } from "@/api/discovery";
import { relativeTime } from "@/lib/time";
import { AggregateOverview } from "@/features/workspace/AggregateOverview";
import { Separator } from "@/components/ui/separator";
import { fetchActivePipelines } from "@/lib/dashboard";
import { LivePipelineCard } from "@/features/workspace/LivePipelineCard";
import { WorkspaceDigest } from "@/features/workspace/WorkspaceDigest";

export function Home() {
  const navigate = useNavigate();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const setSelectedProjectId = useStore((s) => s.setSelectedProjectId);

  const { data: discovered, isFetching: discovering } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const activeProject = (discovered as DiscoveryProject[] | undefined ?? []).find((p) => p.id === activeWorkspaceId) ?? null;

  // Wave 3 (2026-05-22): ["active-pipelines"] is invalidated by the FS watcher
  // on every "pipeline-state" change, so the 12s poll is redundant. staleTime
  // remains the cache-freshness floor.
  const { data: livePipelines } = useQuery({
    queryKey: ['active-pipelines', activeProject?.path],
    queryFn: () => fetchActivePipelines(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 5_000,
  });

  // Portfolio mode: live pipelines across all projects (watcher-driven too).
  const livePipelinesQueries = useQueries({
    queries: (!activeProject ? (discovered ?? []) : []).map((p) => ({
      queryKey: ['active-pipelines', p.path],
      queryFn: () => fetchActivePipelines(p.path),
      staleTime: 5_000,
    })),
  });

  const allLive = livePipelinesQueries
    .flatMap((q, i) =>
      (q.data ?? []).map((pipeline) => ({ pipeline, project: (discovered ?? [])[i] })),
    )
    .filter((item) => !!item.project)
    .sort((a, b) => +new Date(b.pipeline.updated_at ?? 0) - +new Date(a.pipeline.updated_at ?? 0))
    .slice(0, 5);

  if (!projectsRoot) {
    return (
      <PageSurface>
        <EmptyState
          title="Configure o diretório de projetos"
          description={
            <>
              Vá em <Link to="/settings" className="underline">Settings</Link> e aponte para a pasta onde estão seus repos.
            </>
          }
        />
      </PageSurface>
    );
  }

  if (discovering && !discovered) {
    return (
      <PageSurface>
        <p className="text-sm text-muted-foreground">Descobrindo projetos…</p>
      </PageSurface>
    );
  }

  if (!discovered || discovered.length === 0) {
    return (
      <PageSurface>
        <EmptyState
          title="Nenhum projeto encontrado"
          description={
            <>
              Não encontramos projetos em <code className="font-mono text-foreground">{projectsRoot}</code>.
            </>
          }
        />
      </PageSurface>
    );
  }

  // ── Workspace mode ──────────────────────────────────────────────────────────
  if (activeProject) {
    // db_path null → no DB yet
    if (!activeProject.db_path) {
      return (
        <PageSurface>
          <EditorialBand
            eyebrow="Mustard"
            title={activeProject.name}
            subtitle={activeProject.path}
          />
          <DataCard padded>
            <p className="text-[13px] text-muted-foreground">
              Este workspace ainda não emitiu eventos. Rode um pipeline (<code className="font-mono">/feature</code>, <code className="font-mono">/bugfix</code>) para popular os dados.
            </p>
          </DataCard>
        </PageSurface>
      );
    }

    return (
      <PageSurface>
        <EditorialBand
          eyebrow="Mustard"
          title={activeProject.name}
          subtitle={activeProject.path}
        />

        {/* Active pipelines */}
        <section className="flex flex-col gap-2">
          <SectionHeader title="Pipelines ativos" />
          {!livePipelines || livePipelines.length === 0 ? (
            <p className="text-[13px] text-muted-foreground">Nenhum pipeline ativo.</p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {livePipelines.map((pipeline) => (
                <LivePipelineCard
                  key={pipeline.spec_name}
                  pipeline={pipeline}
                  projectName={activeProject.name}
                  onClick={() => navigate(`/project/${activeProject.id}/spec/${pipeline.spec_name}`)}
                />
              ))}
            </ul>
          )}
        </section>

        {/* Resumo do dia */}
        <section className="flex flex-col gap-2">
          <SectionHeader title="Resumo de hoje" />
          <WorkspaceDigest project={activeProject} />
        </section>
      </PageSurface>
    );
  }

  // ── Portfolio mode ──────────────────────────────────────────────────────────
  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Mustard"
        title="Portfólio"
        subtitle="Visão consolidada de todos os projetos descobertos no diretório raiz."
      />

      <AggregateOverview projects={discovered} />

      <section className="flex flex-col gap-2">
        <SectionHeader title="Pipelines ativos" />
        {allLive.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">Nenhuma pipeline em execução.</p>
        ) : (
          <ul className="flex flex-col gap-0.5">
            {allLive.map(({ pipeline, project }) => (
              <LivePipelineCard
                key={`${project.id}-${pipeline.spec_name}`}
                pipeline={pipeline}
                projectName={project.name}
                onClick={() => navigate(`/project/${project.id}/spec/${pipeline.spec_name}`)}
              />
            ))}
          </ul>
        )}
      </section>

      <Separator />

      <section className="flex flex-col gap-2">
        <SectionHeader title="Projetos" right={String(discovered.length)} />
        <DataCard>
          {discovered.map((p) => (
            <DataRow
              key={p.id}
              onClick={() => {
                setSelectedProjectId(p.id);
                navigate(`/project/${p.id}`);
              }}
              lead={
                <span className="inline-flex items-center gap-2">
                  <FolderGit2 className="h-3.5 w-3.5" />
                  <StatusDot
                    variant={
                      p.last_activity_ms && Date.now() - p.last_activity_ms < 3_600_000
                        ? 'active'
                        : 'idle'
                    }
                    pulse={false}
                    size="md"
                  />
                </span>
              }
              primary={p.name}
              trailing={
                <span className="text-[12px] text-muted-foreground">
                  {p.last_activity_ms
                    ? relativeTime(new Date(p.last_activity_ms).toISOString())
                    : '—'}
                </span>
              }
            />
          ))}
        </DataCard>
      </section>
    </PageSurface>
  );
}
