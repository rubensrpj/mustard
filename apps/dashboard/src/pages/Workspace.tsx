import { useNavigate } from "react-router";
import { useStore } from "@/lib/store";
import { useProjects } from "@/lib/dashboard";
import { useWorkspaceSummarySingle } from "@/hooks/useWorkspaceSummary";
import { useTranslate } from "@/lib/i18n";
import {
  PageHeader,
  EmptyState,
  DataCard,
} from "@/components/page";
import { WorkspaceHero } from "@/components/workspace/WorkspaceHero";
import { WorkspaceStatusCounters } from "@/components/workspace/WorkspaceStatusCounters";
import { WorkspaceAlertsColumn } from "@/components/workspace/WorkspaceAlertsColumn";
import { WorkspaceSpecsByStatus } from "@/components/workspace/WorkspaceSpecsByStatus";
import { WorkspaceTokenSummary } from "@/components/workspace/WorkspaceTokenSummary";
import { WorkspaceFilesRanking } from "@/components/workspace/WorkspaceFilesRanking";
import { ExecutionTrace } from "@/components/trace/ExecutionTrace";

/**
 * Wave 8 (2026-05-21, spec
 * `2026-05-20-economia-moat-unification/wave-8-visao-geral-revamp`) — layout
 * revamp:
 *
 *   • `<WorkspaceHero>` (multi-spec list) replaces the old
 *     `<WorkspaceStatusBar>` + single-pipeline `<PipelineTimeline>` pair.
 *   • `<WorkspaceStatusCounters>` (5 big tiles) replaces the previous
 *     month-activity calendar — that surface is intentionally not loaded
 *     here; reachable from `/specs?date=…` if needed.
 *   • `<WorkspaceSpecsByStatus>` is rendered full-width (no `col-span-2`
 *     wrapper).
 *   • Bottom row is a 50/50 grid pairing `<WorkspaceAlertsColumn>` and
 *     `<WorkspaceFilesRanking>` so both surfaces breathe at the same width.
 *   • `<ExecutionTrace>` (Wave 6) remains as the tail surface; it follows the
 *     same "primary active spec" heuristic as before.
 *
 * Labels run through the Wave-8 `useTranslate()` provider so the language
 * preference under `/preferences` switches them live (other pages stay on the
 * legacy i18next runtime until they migrate lazily).
 */
export function Workspace() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const navigate = useNavigate();
  const t = useTranslate();

  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;

  const { data: summary, isLoading } = useWorkspaceSummarySingle(
    activeProject?.path ?? null,
  );

  if (!projectsRoot) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: t("workspace.title") }]}
          title={t("workspace.title")}
          subtitle={t("workspace.subtitle")}
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
          breadcrumb={[{ label: t("workspace.title") }]}
          title={t("workspace.title")}
          subtitle={t("workspace.subtitle")}
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
          breadcrumb={[{ label: t("workspace.title") }]}
          title={t("workspace.title")}
          subtitle={t("workspace.subtitle")}
        />
        <p className="text-[13px] text-muted-foreground">{t("common.loading")}</p>
      </div>
    );
  }

  const tracks = summary?.spec_tracks ?? [];
  const alerts = summary?.alerts ?? [];

  // Pick the first non-terminal spec — same heuristic the Hero uses to lead
  // its list. Falls back to the freshest track when everything is closed so
  // ExecutionTrace still has something to render.
  const heroTrack =
    tracks.find(
      (track) =>
        !["completed", "closed", "cancelled", "no-events"].includes(
          track.status.toLowerCase(),
        ),
    ) ?? tracks[0];
  const primaryActiveSpec = heroTrack?.spec ?? null;

  function handleAlertClick(alert: { spec: string }) {
    navigate(`/specs#${alert.spec}`);
  }

  const repoPath = activeProject.path;

  return (
    <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={[{ label: t("workspace.title") }]}
        title={t("workspace.title")}
        subtitle={t("workspace.subtitle")}
      />

      {/* Hero: multi-spec list (replaces single-pipeline StatusBar + Timeline). */}
      <WorkspaceHero summary={summary} />

      {/* Status counters (replaces MonthCalendar) + token savings card. */}
      <div className="grid grid-cols-3 gap-6">
        <div className="col-span-2">
          <WorkspaceStatusCounters repoPath={repoPath} />
        </div>
        <WorkspaceTokenSummary repoPath={repoPath} />
      </div>

      {/* Specs by status — full width, no col-span-2 wrapper. */}
      <WorkspaceSpecsByStatus repoPath={repoPath} />

      {/* Bottom split 50/50: alerts | files ranking */}
      <div className="grid grid-cols-2 gap-6">
        <WorkspaceAlertsColumn
          alerts={alerts}
          onAlertClick={handleAlertClick}
        />
        <WorkspaceFilesRanking repoPath={repoPath} />
      </div>

      {/* Wave 6 trace viewer stays as the tail surface (hierarchical spec →
          wave → agent → tool tree from `dashboard_spec_trace`). */}
      <DataCard padded>
        <ExecutionTrace
          projectPath={repoPath}
          specName={primaryActiveSpec}
        />
      </DataCard>
    </div>
  );
}
