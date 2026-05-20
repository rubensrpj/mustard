import { useNavigate } from "react-router";
import { useStore } from "@/lib/store";
import { useProjects } from "@/lib/dashboard";
import { useWorkspaceSummarySingle } from "@/hooks/useWorkspaceSummary";
import {
  PageHeader,
  EmptyState,
  DataCard,
} from "@/components/page";
import { WorkspaceStatusBar } from "@/components/workspace/WorkspaceStatusBar";
import { WorkspaceAlertsColumn } from "@/components/workspace/WorkspaceAlertsColumn";
import { WorkspaceSpecsByStatus } from "@/components/workspace/WorkspaceSpecsByStatus";
import { WorkspaceTokenSummary } from "@/components/workspace/WorkspaceTokenSummary";
import { WorkspaceMonthCalendar } from "@/components/workspace/WorkspaceMonthCalendar";
import { WorkspaceEventsFeed } from "@/components/workspace/WorkspaceEventsFeed";
import { WorkspaceFilesRanking } from "@/components/workspace/WorkspaceFilesRanking";
import { PipelineTimeline } from "@/components/telemetry/PipelineTimeline";

export function Workspace() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const navigate = useNavigate();

  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;

  const { data: summary, isLoading } = useWorkspaceSummarySingle(
    activeProject?.path ?? null,
  );

  if (!projectsRoot) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: "Workspace" }]}
          title="Visão Geral"
          subtitle="Sala de operações multi-track"
        />
        <EmptyState
          title="Diretório de projetos não configurado"
          description="Vá em Configurações e aponte para a pasta onde estão seus repos."
        />
      </div>
    );
  }

  if (!activeWorkspaceId || !activeProject) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: "Workspace" }]}
          title="Visão Geral"
          subtitle="Sala de operações multi-track"
        />
        <EmptyState
          title="Selecione um workspace"
          description="Use o seletor na sidebar para escolher um projeto."
        />
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: "Workspace" }]}
          title="Visão Geral"
          subtitle="Sala de operações multi-track"
        />
        <p className="text-[13px] text-muted-foreground">Carregando…</p>
      </div>
    );
  }

  const tracks = summary?.spec_tracks ?? [];
  const alerts = summary?.alerts ?? [];

  // Hero: prefer the first active spec track, fall back to the most recent.
  // PipelineTimeline owns its own empty state when nothing is supplied.
  const heroTrack =
    tracks.find(
      (t) => !["completed", "closed", "cancelled", "no-events"].includes(t.status),
    ) ?? tracks[0];

  const heroPipeline = heroTrack
    ? {
        spec: heroTrack.spec,
        currentPhase: heroTrack.current_phase,
        phasesCompleted: heroTrack.segments
          .filter((s) => s.state === "completed")
          .map((s) => s.phase),
      }
    : undefined;

  function handleAlertClick(alert: { spec: string }) {
    navigate(`/specs#${alert.spec}`);
  }

  const repoPath = activeProject.path;

  return (
    <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={[{ label: "Workspace" }]}
        title="Visão Geral"
        subtitle="Sala de operações multi-track"
      />

      {/* Hero: pulse stats above the 5-station PipelineTimeline */}
      <DataCard padded>
        <div className="flex flex-col gap-4">
          <WorkspaceStatusBar
            summary={summary}
            className="border-0 bg-transparent p-0"
          />
          <PipelineTimeline pipeline={heroPipeline} />
        </div>
      </DataCard>

      {/* KPI row: specs-by-status (2/3) + token-savings (1/3) */}
      <div className="grid grid-cols-3 gap-6">
        <div className="col-span-2">
          <WorkspaceSpecsByStatus repoPath={repoPath} />
        </div>
        <WorkspaceTokenSummary repoPath={repoPath} />
      </div>

      {/* Month-activity calendar — full width */}
      <WorkspaceMonthCalendar repoPath={repoPath} />

      {/* Main feed + alerts/files aside */}
      <div className="flex gap-6">
        <main className="flex-1 min-w-0">
          <WorkspaceEventsFeed repoPath={repoPath} />
        </main>

        <aside className="w-[280px] shrink-0 flex flex-col gap-6">
          <WorkspaceAlertsColumn
            alerts={alerts}
            onAlertClick={handleAlertClick}
          />
          <WorkspaceFilesRanking repoPath={repoPath} />
        </aside>
      </div>
    </div>
  );
}
