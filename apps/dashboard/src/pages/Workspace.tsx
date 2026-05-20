import { useNavigate } from "react-router";
import { useStore } from "@/lib/store";
import { useProjects } from "@/lib/dashboard";
import { useWorkspaceSummarySingle } from "@/hooks/useWorkspaceSummary";
import { useTelemetryHeatmap } from "@/hooks/useTelemetryHeatmap";
import {
  PageHeader,
  SectionHeader,
  EmptyState,
  DataCard,
} from "@/components/page";
import { WorkspaceStatusBar } from "@/components/workspace/WorkspaceStatusBar";
import { SpecTracksList } from "@/components/workspace/SpecTracksList";
import { WorkspaceEffortFooter } from "@/components/workspace/WorkspaceEffortFooter";
import { WorkspaceAlertsColumn } from "@/components/workspace/WorkspaceAlertsColumn";
import { EffortHeatmap } from "@/components/telemetry/EffortHeatmap";
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

  const { data: heatmapCells = [] } = useTelemetryHeatmap(
    activeProject?.path ?? null,
    "today",
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

  if (!activeWorkspaceId) {
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
  const topFiles = summary?.top_files_today ?? [];

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

      {/* Effort heatmap — full width, below hero */}
      <DataCard padded>
        <div className="flex flex-col gap-2">
          <span className="text-[11px] font-medium uppercase tracking-widest text-muted-foreground">
            Atividade hoje
          </span>
          <div className="overflow-x-auto">
            <EffortHeatmap cells={heatmapCells} />
          </div>
        </div>
      </DataCard>

      <div className="flex gap-6">
        <main className="flex-1 flex flex-col gap-6 min-w-0">
          <section className="flex flex-col gap-3">
            <SectionHeader
              title="Specs ativas"
              right={tracks.length > 0 ? String(tracks.length) : undefined}
            />
            <DataCard padded>
              <SpecTracksList tracks={tracks} />
            </DataCard>
          </section>

          <WorkspaceEffortFooter topFiles={topFiles} />
        </main>

        <aside className="w-[280px] shrink-0">
          <WorkspaceAlertsColumn
            alerts={alerts}
            onAlertClick={handleAlertClick}
          />
        </aside>
      </div>
    </div>
  );
}
