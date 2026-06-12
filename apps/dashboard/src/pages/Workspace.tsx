import { useStore } from "@/lib/store";
import { useProjects } from "@/lib/dashboard";
import { useTranslate } from "@/lib/i18n";
import {
  EmptyState,
  PageSurface,
  EditorialBand,
  SectionHeader,
} from "@/components/page";
import { Separator } from "@/components/ui/separator";
import { SpecStatusCards } from "@/features/workspace/SpecStatusCards";
import { SpecAlertsBand } from "@/features/workspace/SpecAlertsBand";
import { ProjectsPanel } from "@/features/workspace/ProjectsPanel";

/**
 * Visão Geral — redesign (spec `redesenho-rota-visao-geral-dashboard`). This is
 * the routed `/workspace` page; it replaces the old multi-surface layout
 * (hero, status counters, token-savings card, specs-by-status, alerts column,
 * execution trace) with two purpose-built sections. Consumption detail still
 * lives on the Economia page; the removed surfaces are intentionally not
 * loaded here.
 *
 *   - Specs    — stage cards (Planejando/Executando/Finalizadas) + an Alerts
 *                band (Suspeitas, Specs paradas), each deep-linking to `/specs`.
 *   - Projetos — project identity (monorepo, languages, stacks), local git
 *                state, and the reused most-touched-files ranking.
 */
export function Workspace() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const t = useTranslate();

  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;

  if (!projectsRoot) {
    return (
      <PageSurface>
        <EmptyState
          title={t("empty.noRoot.title")}
          description={t("empty.noRoot.description")}
        />
      </PageSurface>
    );
  }

  if (!activeWorkspaceId || !activeProject) {
    return (
      <PageSurface>
        <EmptyState
          title={t("empty.noWorkspace.title")}
          description={t("empty.noWorkspace.description")}
        />
      </PageSurface>
    );
  }

  const repoPath = activeProject.path;

  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Workspace"
        title={activeProject.name}
        subtitle={t("workspace.editorialSubtitle").replace("{name}", activeProject.name)}
      />

      {/* ── Specs: stage cards + alerts band (Suspeitas, Specs paradas) ──── */}
      <section className="flex flex-col gap-3">
        <SectionHeader title="Specs" />
        <SpecStatusCards repoPath={repoPath} />
        <SpecAlertsBand repoPath={repoPath} />
      </section>

      <Separator />

      {/* ── Projetos: master-detail — sub-sidebar picks identity / git /
            most-touched files; the right panel shows only the active one. ── */}
      <section className="flex flex-col gap-3">
        <SectionHeader title="Projetos" />
        <ProjectsPanel repoPath={repoPath} />
      </section>
    </PageSurface>
  );
}
