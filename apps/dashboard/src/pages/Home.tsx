import { Link, useNavigate } from "react-router";
import { useQuery, useQueries } from "@tanstack/react-query";
import { FolderGit2 } from "lucide-react";
import { StatusDot } from "@/components/StatusDot";
import { useStore } from "@/lib/store";
import { discoverProjects } from "@/api/discovery";
import type { Project as DiscoveryProject } from "@/api/discovery";
import { relativeTime } from "@/lib/time";
import { AggregateOverview } from "@/components/AggregateOverview";
import { Separator } from "@/components/ui/separator";
import { fetchActivePipelines } from "@/lib/dashboard";
import { LivePipelineCard } from "@/components/LivePipelineCard";
import { WorkspaceDigest } from "@/components/WorkspaceDigest";

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

  const { data: livePipelines } = useQuery({
    queryKey: ['active-pipelines', activeProject?.path],
    queryFn: () => fetchActivePipelines(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 5_000,
    refetchInterval: 12_000,
  });

  // Portfolio mode: live pipelines across all projects
  const livePipelinesQueries = useQueries({
    queries: (!activeProject ? (discovered ?? []) : []).map((p) => ({
      queryKey: ['active-pipelines', p.path],
      queryFn: () => fetchActivePipelines(p.path),
      staleTime: 5_000,
      refetchInterval: 12_000,
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
      <p className="text-sm text-muted-foreground">
        Configure o diretório de projetos em{" "}
        <Link to="/settings" className="underline">Settings</Link>.
      </p>
    );
  }

  if (discovering && !discovered) {
    return <p className="text-sm text-muted-foreground">Descobrindo projetos…</p>;
  }

  if (!discovered || discovered.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        Nenhum projeto encontrado em{" "}
        <code className="font-mono text-foreground">{projectsRoot}</code>.
      </p>
    );
  }

  // ── Workspace mode ──────────────────────────────────────────────────────────
  if (activeProject) {
    // db_path null → no DB yet
    if (!activeProject.db_path) {
      return (
        <div className="flex flex-col gap-3">
          <h1 className="text-2xl font-semibold">{activeProject.name}</h1>
          <p className="text-sm font-mono text-muted-foreground">{activeProject.path}</p>
          <div className="border border-border rounded p-4 text-[13px] text-muted-foreground mt-2">
            Este workspace ainda não emitiu eventos. Rode um pipeline (<code className="font-mono">/feature</code>, <code className="font-mono">/bugfix</code>) para popular os dados.
          </div>
        </div>
      );
    }

    return (
      <div className="flex flex-col gap-8">
        {/* Header */}
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold">{activeProject.name}</h1>
          <p className="text-sm font-mono text-muted-foreground">{activeProject.path}</p>
        </div>

        {/* Active pipelines */}
        <section>
          <h2 className="text-xs uppercase tracking-wider font-medium text-muted-foreground mb-2">
            Pipelines ativos
          </h2>
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
        <div>
          <h2 className="text-xs uppercase tracking-wider font-medium text-muted-foreground mb-2">
            Resumo de hoje
          </h2>
          <WorkspaceDigest project={activeProject} />
        </div>
      </div>
    );
  }

  // ── Portfolio mode ──────────────────────────────────────────────────────────
  return (
    <div className="flex flex-col gap-8">
      <AggregateOverview projects={discovered} />

      <section>
        <h2 className="text-xs uppercase tracking-wider font-medium text-muted-foreground mb-2">
          Active pipelines
        </h2>
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

      <section>
        <div className="flex items-baseline gap-2 mb-2">
          <h2 className="text-xs uppercase tracking-wider font-medium text-muted-foreground">
            Projetos
          </h2>
          <span className="text-[13px] text-muted-foreground/50 font-mono">{discovered.length}</span>
        </div>
        <ul className="flex flex-col gap-0.5 text-sm">
          {discovered.map((p) => (
            <li
              key={p.id}
              className="flex items-center gap-2 px-2 py-1 rounded hover:bg-muted/40 cursor-pointer"
              onClick={() => {
                setSelectedProjectId(p.id);
                navigate(`/project/${p.id}`);
              }}
            >
              <FolderGit2 className="h-3.5 w-3.5 text-muted-foreground" />
              <StatusDot
                variant={
                  p.last_activity_ms && Date.now() - p.last_activity_ms < 3_600_000
                    ? 'active'
                    : 'idle'
                }
                pulse={false}
                size="md"
              />
              <span>{p.name}</span>
              <span className="text-muted-foreground text-[13px] ml-auto">
                {p.last_activity_ms
                  ? relativeTime(new Date(p.last_activity_ms).toISOString())
                  : '—'}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
