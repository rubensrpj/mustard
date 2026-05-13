import { NavLink } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { Home, Settings, BookOpen, Activity } from "lucide-react";
import { Separator } from "@/components/ui/separator";
import { Badge } from "@/components/ui/badge";
import { StatusDot } from "@/components/StatusDot";
import { useStore } from "@/lib/store";
import { discoverProjects, type Project } from "@/api/discovery";

const navItemClass = ({ isActive }: { isActive: boolean }) =>
  `flex items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150 ${isActive ? "bg-primary/10 text-primary font-medium" : "text-sidebar-foreground/70 hover:bg-muted/40 hover:text-foreground"}`;

export function Sidebar() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const list: Project[] = projects ?? [];
  const count = list.length;

  return (
    <aside className="row-span-2 col-start-1 bg-sidebar text-sidebar-foreground border-r border-sidebar-border flex flex-col gap-0">
      <div className="px-3 py-3 border-b border-sidebar-border flex items-center justify-between">
        <span className="text-sm font-semibold tracking-tight">Mustard</span>
        <span className="text-[10px] uppercase tracking-wider text-muted-foreground bg-muted/40 px-1.5 py-0.5 rounded">v0.1.0</span>
      </div>
      <div className="px-2 pt-3 flex flex-col flex-1">
        <div className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground px-3 py-1.5">Navigation</div>
        <NavLink to="/" end className={navItemClass}>
          <Home className="h-3.5 w-3.5" /> Home
        </NavLink>
        <NavLink to="/knowledge" className={navItemClass}>
          <BookOpen className="h-3.5 w-3.5" /> Knowledge
        </NavLink>
        <NavLink to="/activity" className={navItemClass}>
          <Activity className="h-3.5 w-3.5" /> Activity
        </NavLink>

        <Separator className="my-3" />

        <div className="flex items-center justify-between px-3 py-1.5">
          <span className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground">Workspace</span>
          <Badge variant="secondary">{count}</Badge>
        </div>
        {!projectsRoot ? (
          <div className="text-xs text-muted-foreground/70 px-3 py-2">Configure root em Settings</div>
        ) : count === 0 ? (
          <div className="text-xs text-muted-foreground/70 px-3 py-2">Nenhum projeto encontrado.</div>
        ) : (
          list.map((p) => {
            const active = p.last_activity_ms !== null && Date.now() - p.last_activity_ms < 3_600_000;
            return (
              <NavLink key={p.id} to={`/project/${p.id}`} className={navItemClass}>
                <StatusDot variant={active ? "active" : "idle"} size="md" />
                <span className="truncate">{p.name}</span>
              </NavLink>
            );
          })
        )}

        <div className="mt-auto" />
        <Separator className="my-3" />
        <NavLink to="/settings" className={navItemClass}>
          <Settings className="h-3.5 w-3.5" /> Settings
        </NavLink>
      </div>
    </aside>
  );
}
