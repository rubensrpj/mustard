import { useEffect } from "react";
import { HashRouter, Routes, Route, Navigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { AppShell } from "@/components/layout/AppShell";
import { Workspace } from "@/pages/Workspace";
import { Specs } from "@/pages/Specs";
import { Economia } from "@/pages/Economia";
import { ProjectDetail } from "@/pages/ProjectDetail";
import { SpecDetail } from "@/pages/SpecDetail";
import { Commands } from "@/pages/Commands";
import { Knowledge } from "@/pages/Knowledge";
import { Settings } from "@/pages/Settings";
import { Preferences } from "@/pages/Preferences";
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
          <Route path="/project/:id/spec/:specName" element={<SpecDetail />} />
          <Route path="/knowledge" element={<Knowledge />} />
          <Route path="/commands" element={<Commands />} />
          <Route path="/sessions" element={<Sessions />} />
          <Route path="/sessions/:id" element={<SessionDetail />} />
          {/* /activity, /telemetry, /quality removed in Wave 6 — consolidated into Workspace/Specs/Economia */}
          <Route path="/prompt-economy" element={<Navigate to="/economy" replace />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="/preferences" element={<Preferences />} />
        </Routes>
      </AppShell>
    </HashRouter>
  );
}

export default App;
