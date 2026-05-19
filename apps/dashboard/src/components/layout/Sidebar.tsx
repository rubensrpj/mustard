import { NavLink } from "react-router";
import { Home, Settings, BookOpen, Activity, Gauge, Terminal, FileText, Sparkles } from "lucide-react";
import { Separator } from "@/components/ui/separator";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useStore } from "@/lib/store";
import { useQuery } from "@tanstack/react-query";
import { discoverProjects } from "@/api/discovery";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";

const navItemClass = ({ isActive }: { isActive: boolean }) =>
  `flex items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150 ${isActive ? "bg-primary/10 text-primary font-medium" : "text-sidebar-foreground/70 hover:bg-muted/40 hover:text-foreground"}`;

const groupHeaderClass =
  "text-xs uppercase tracking-wider font-medium text-muted-foreground px-3 py-1.5";

export function Sidebar() {
  const { t } = useTranslation();
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const setActiveWorkspaceId = useStore((s) => s.setActiveWorkspaceId);
  const projectsRoot = useStore((s) => s.projectsRoot);
  const workspaceLocked = activeWorkspaceId === null;

  const { data: projects, isLoading } = useQuery({
    queryKey: ["discover", projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const workspaceGroup = (
    <div
      className={cn(
        "flex flex-col",
        workspaceLocked && "opacity-50 pointer-events-none",
      )}
      aria-disabled={workspaceLocked}
    >
      <NavLink to="/" end className={navItemClass}>
        <Home className="h-3.5 w-3.5" /> {t('nav.home')}
      </NavLink>
      <NavLink to="/activity" className={navItemClass}>
        <Activity className="h-3.5 w-3.5" /> {t('nav.activity')}
      </NavLink>
      <NavLink to="/telemetry" className={navItemClass}>
        <Gauge className="h-3.5 w-3.5" /> {t('nav.telemetry')}
      </NavLink>
      <NavLink to="/quality" className={navItemClass}>
        <Sparkles className="h-3.5 w-3.5" /> {t('nav.quality')}
      </NavLink>
      <NavLink to="/knowledge" className={navItemClass}>
        <BookOpen className="h-3.5 w-3.5" /> {t('nav.knowledge')}
      </NavLink>
    </div>
  );

  return (
    <aside className="row-span-2 col-start-1 bg-sidebar text-sidebar-foreground border-r border-sidebar-border flex flex-col gap-0">
      <div className="border-b border-sidebar-border">
        <WorkspaceSwitcher
          projects={projects ?? []}
          activeId={activeWorkspaceId}
          onSelect={setActiveWorkspaceId}
          projectsRoot={projectsRoot}
          loading={isLoading}
        />
      </div>
      <div className="px-2 pt-3 flex flex-col flex-1">
        <div className={groupHeaderClass}>{t('group.workspace')}</div>
        {workspaceLocked ? (
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <div>{workspaceGroup}</div>
              </TooltipTrigger>
              <TooltipContent side="right">{t('tooltip.selectWorkspace')}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : (
          workspaceGroup
        )}

        <Separator className="my-3" />

        <div className={groupHeaderClass}>{t('group.tools')}</div>
        <NavLink to="/commands" className={navItemClass}>
          <Terminal className="h-3.5 w-3.5" /> {t('nav.commands')}
        </NavLink>
        <NavLink to="/prd" className={navItemClass}>
          <FileText className="h-3.5 w-3.5" /> {t('nav.prd')}
        </NavLink>

        <div className="mt-auto" />
        <Separator className="my-3" />
        <NavLink to="/settings" className={navItemClass}>
          <Settings className="h-3.5 w-3.5" /> {t('nav.settings')}
        </NavLink>
      </div>
    </aside>
  );
}
