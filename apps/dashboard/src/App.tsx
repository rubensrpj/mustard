import { useEffect } from "react";
import { HashRouter, Routes, Route, Navigate, useParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { AppShell } from "@/components/layout/AppShell";
import { Workspace } from "@/pages/Workspace";
import { Specs } from "@/pages/Specs";
import { Economia } from "@/pages/Economia";
import { ProjectDetail } from "@/pages/ProjectDetail";
import { Commands } from "@/pages/Commands";
import { Knowledge } from "@/pages/Knowledge";
import { Settings } from "@/pages/Settings";
import { Sessions } from "@/pages/Sessions";
import { SessionDetail } from "@/pages/SessionDetail";
import { Activity } from "@/pages/Activity";
import { CommandPalette } from "@/components/layout/CommandPalette";
import { Toaster } from "sonner";
import { useStore } from "@/lib/store";
import { useProjectsStore } from "@/lib/projects-store";
import { discoverProjects } from "@/api/discovery";
import { startWatcher, subscribeFsChange } from "@/lib/watcher";
import { useTheme } from "@/hooks/useTheme";

/**
 * Legacy deep-link shim: `/project/:id/spec/:specName` used to render a
 * duplicate spec page; the canonical surface is the `/specs#{slug}` drill-in.
 * Sync the active workspace to `:id` first so the drill-in resolves the spec
 * against the right project, then redirect.
 */
function LegacySpecRedirect() {
  const { id, specName } = useParams<{ id: string; specName: string }>();
  const workspaceSynced = useStore((s) => !id || s.activeWorkspaceId === id);
  useEffect(() => {
    if (id) useStore.getState().setActiveWorkspaceId(id);
  }, [id]);
  if (!workspaceSynced) return null;
  return <Navigate to={`/specs#${encodeURIComponent(specName ?? "")}`} replace />;
}

function App() {
  const { theme } = useTheme();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const pathsKey = projects?.map((p) => p.path).sort().join('|') ?? '';
  useEffect(() => {
    if (!projects?.length) return;
    startWatcher(projects.map((p) => p.path)).catch((e) =>
      console.error('startWatcher failed', e),
    );
  }, [pathsKey]);

  useEffect(() => {
    const p = subscribeFsChange();
    return () => {
      p.then((u) => u()).catch(() => {});
    };
  }, []);

  // Hydrate the projects registry once on mount (Tauri plugin-store -> in-memory).
  useEffect(() => {
    useProjectsStore.getState().loadFromStore();
  }, []);

  return (
    <HashRouter>
      <Toaster position="bottom-right" richColors theme={theme} />
      <CommandPalette />
      <AppShell>
        <Routes>
          <Route path="/" element={<Navigate to="/workspace" replace />} />
          <Route path="/workspace" element={<Workspace />} />
          {/* `/activity` is the new primary nav entry (spec
              `dashboard-aba-atividade-agrupar-trabalho`); `/specs` stays
              routed for the Workspace `?filter=` lifecycle deep-links. */}
          <Route path="/activity" element={<Activity />} />
          <Route path="/specs" element={<Specs />} />
          <Route path="/economy" element={<Economia />} />
          <Route path="/project/:id" element={<ProjectDetail />} />
          <Route path="/project/:id/spec/:specName" element={<LegacySpecRedirect />} />
          <Route path="/knowledge" element={<Knowledge />} />
          <Route path="/commands" element={<Commands />} />
          <Route path="/sessions" element={<Sessions />} />
          <Route path="/sessions/:id" element={<SessionDetail />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </AppShell>
    </HashRouter>
  );
}

export default App;
