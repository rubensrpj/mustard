import { HashRouter, Routes, Route } from "react-router";
import { AppShell } from "@/components/layout/AppShell";
import { Home } from "@/pages/Home";
import { ProjectDetail } from "@/pages/ProjectDetail";
import { SpecDetail } from "@/pages/SpecDetail";
import { Knowledge } from "@/pages/Knowledge";
import { Activity } from "@/pages/Activity";
import { Settings } from "@/pages/Settings";
import { CommandPalette } from "@/components/CommandPalette";

function App() {
  return (
    <HashRouter>
      <CommandPalette />
      <AppShell>
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/project/:id" element={<ProjectDetail />} />
          <Route path="/project/:id/spec/:specName" element={<SpecDetail />} />
          <Route path="/knowledge" element={<Knowledge />} />
          <Route path="/activity" element={<Activity />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </AppShell>
    </HashRouter>
  );
}

export default App;
