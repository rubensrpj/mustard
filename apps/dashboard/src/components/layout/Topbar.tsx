import { useLocation } from "react-router";
import { Sun, Moon, RefreshCw } from "lucide-react";
import { useQueryClient, useQuery, useQueries } from "@tanstack/react-query";
import { useTheme } from "@/hooks/useTheme";
import { useStore } from "@/lib/store";
import { useT } from "@/lib/i18n";
import { discoverProjects } from "@/api/discovery";
import { fetchActivePipelines } from "@/lib/dashboard";

/**
 * Map a route to its breadcrumb label. `t` is the resolver from `@/lib/i18n`,
 * passed in so the Topbar re-renders whenever the Preferences language slice
 * changes (the hook reads `useStore((s) => s.language)`).
 */
function pathLabel(pathname: string, t: (key: string, fallback?: string) => string): string {
  if (pathname === "/" || pathname === "/workspace") return t("sidebar.overview");
  if (pathname === "/specs") return t("sidebar.specs");
  if (pathname === "/economy") return t("sidebar.economy");
  if (pathname === "/knowledge") return t("sidebar.knowledge");
  if (pathname === "/settings") return t("sidebar.settings");
  if (pathname === "/preferences") return t("sidebar.preferences");
  if (pathname === "/activity") return t("sidebar.activity");
  if (pathname === "/telemetry") return t("sidebar.telemetry");
  if (pathname === "/quality") return t("sidebar.quality");
  if (pathname === "/commands") return t("sidebar.commands");
  if (pathname === "/prd") return t("sidebar.prd");
  if (pathname.startsWith("/project/")) return t("breadcrumb.workspace");
  return pathname.replace(/^\//, "").replace(/^./, (c) => c.toUpperCase());
}

export function Topbar() {
  const location = useLocation();
  const t = useT();
  const label = pathLabel(location.pathname, t);
  const { theme, toggle } = useTheme();
  const queryClient = useQueryClient();
  const projectsRoot = useStore((s) => s.projectsRoot);

  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  // Wave 3 (2026-05-22): watcher-driven via "pipeline-state" — no poll needed.
  const liveQueries = useQueries({
    queries: (projects ?? []).map((p) => ({
      queryKey: ['active-pipelines', p.path],
      queryFn: () => fetchActivePipelines(p.path),
      staleTime: 5_000,
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
            <span className="w-1.5 h-1.5 rounded-full bg-[--color-ok] animate-pulse" />
            <span>live</span>
          </div>
        )}
      </div>
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          title={t("action.reload_projects")}
          aria-label={t("action.reload_projects")}
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
