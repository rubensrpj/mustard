import type { ReactNode } from "react";
import { Sidebar } from "../Sidebar";
import { Topbar } from "../Topbar";
import { CodeViewerPanel } from "@/components/page/CodeViewerPanel";
import { useStore } from "@/lib/store";

/**
 * Single page shell for every route. `<main>` is a horizontal split: the page
 * content (children) on the left fills the remaining width and owns its own
 * vertical scroll, and the docked, IDE-style `<CodeViewerPanel />` sits on the
 * RIGHT. The panel renders nothing until a file is opened, so the content
 * occupies the whole area by default and only shares once a tab is open.
 *
 * Content runs full-width (no centered max-width column) so the page uses every
 * pixel — its own padding (PageSurface / per-page `px-6`) keeps it readable —
 * and shrinks cleanly when the code panel docks in.
 *
 * The first grid column tracks the navigation sidebar: ~56px when collapsed to
 * an icon rail, 220px when expanded (persisted in the zustand store).
 */
export function AppShell({ children }: { children: ReactNode }) {
  const collapsed = useStore((s) => s.sidebarCollapsed);
  return (
    <div
      className={`grid ${collapsed ? "grid-cols-[56px_1fr]" : "grid-cols-[220px_1fr]"} grid-rows-[40px_1fr] h-screen bg-background text-foreground`}
    >
      <Sidebar />
      <Topbar />
      <main className="row-start-2 col-start-2 flex min-h-0 min-w-0 overflow-hidden">
        <div className="flex-1 min-w-0 overflow-y-auto">
          <div className="w-full px-6">{children}</div>
        </div>
        <CodeViewerPanel />
      </main>
    </div>
  );
}
