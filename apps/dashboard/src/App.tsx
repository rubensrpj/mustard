import { useEffect } from "react";
import { HashRouter, Routes, Route, Navigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { AppShell } from "@/components/layout/AppShell";
import { Home } from "@/pages/Home";
import { ProjectDetail } from "@/pages/ProjectDetail";
import { SpecDetail } from "@/pages/SpecDetail";
import { Commands } from "@/pages/Commands";
import { Knowledge } from "@/pages/Knowledge";
import { Activity } from "@/pages/Activity";
import { Settings } from "@/pages/Settings";
import { Telemetry } from "@/pages/Telemetry";
import { Prd } from "@/pages/Prd";
import { Quality } from "@/pages/Quality";
import { CommandPalette } from "@/components/CommandPalette";
import { Toaster } from "sonner";
import { useStore } from "@/lib/store";
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

  return (
    <HashRouter>
      <Toaster position="bottom-right" richColors theme={theme} />
      <CommandPalette />
      <AppShell>
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/project/:id" element={<ProjectDetail />} />
          <Route path="/project/:id/spec/:specName" element={<SpecDetail />} />
          <Route path="/knowledge" element={<Knowledge />} />
          <Route path="/commands" element={<Commands />} />
          <Route path="/prd" element={<Prd />} />
          <Route path="/activity" element={<Activity />} />
          <Route path="/telemetry" element={<Telemetry />} />
          <Route path="/quality" element={<Quality />} />
          {/* Prompt Economy is now the "Economia" tab inside Telemetry —
              keep the old path working for bookmarks. */}
          <Route path="/prompt-economy" element={<Navigate to="/telemetry?tab=economia" replace />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </AppShell>
    </HashRouter>
  );
}

export default App;
