import { useLocation } from "react-router";
import { Sun, Moon, RefreshCw } from "lucide-react";
import { useQueryClient, useQuery, useQueries } from "@tanstack/react-query";
import { useTheme } from "@/hooks/useTheme";
import { useStore } from "@/lib/store";
import { discoverProjects } from "@/api/discovery";
import { fetchActivePipelines } from "@/lib/dashboard";

function pathLabel(pathname: string): string {
  if (pathname === "/") return "Home";
  if (pathname === "/settings") return "Settings";
  if (pathname === "/activity") return "Atividade";
  if (pathname === "/telemetry") return "Telemetria";
  if (pathname === "/quality") return "Qualidade";
  if (pathname === "/knowledge") return "Knowledge";
  if (pathname === "/commands") return "Comandos";
  if (pathname === "/prd") return "PRD";
  if (pathname.startsWith("/project/")) return "Projeto";
  return pathname.replace(/^\//, "").replace(/^./, (c) => c.toUpperCase());
}

export function Topbar() {
  const location = useLocation();
  const label = pathLabel(location.pathname);
  const { theme, toggle } = useTheme();
  const queryClient = useQueryClient();
  const projectsRoot = useStore((s) => s.projectsRoot);

  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const liveQueries = useQueries({
    queries: (projects ?? []).map((p) => ({
      queryKey: ['active-pipelines', p.path],
      queryFn: () => fetchActivePipelines(p.path),
      staleTime: 5_000,
      refetchInterval: 12_000,
    })),
  });

  const hasActive = liveQueries.some((q) => (q.data?.length ?? 0) > 0);

  return (
    <header className="row-start-1 col-start-2 h-12 sticky top-0 bg-background border-b border-border flex items-center justify-between gap-3 px-5 z-10">
      <div className="flex items-center gap-3 min-w-0">
        <nav className="text-sm flex items-center gap-1.5 min-w-0" aria-label="Breadcrumb">
          <span className="text-muted-foreground">Mustard</span>
          <span className="text-muted-foreground">/</span>
          <span className="text-foreground font-medium truncate">{label}</span>
        </nav>
        {hasActive && (
          <div className="flex items-center gap-1 text-[10px] text-muted-foreground">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
            <span>live</span>
          </div>
        )}
      </div>
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          title="Reload projects"
          aria-label="Reload projects"
          disabled={!projectsRoot}
          onClick={() => {
            if (projectsRoot) {
              queryClient.invalidateQueries({ queryKey: ['discover', projectsRoot] });
            }
          }}
          className={`h-7 w-7 rounded-md text-muted-foreground hover:bg-muted hover:text-foreground transition-colors duration-150 inline-flex items-center justify-center ${!projectsRoot ? "opacity-50 cursor-not-allowed" : ""}`}
        >
          <RefreshCw className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          onClick={toggle}
          aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
          className="inline-flex items-center justify-center h-7 w-7 rounded-md text-muted-foreground hover:bg-muted hover:text-foreground transition-colors duration-150"
        >
          {theme === "dark" ? <Sun className="h-3.5 w-3.5" /> : <Moon className="h-3.5 w-3.5" />}
        </button>
      </div>
    </header>
  );
}
