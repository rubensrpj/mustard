import type { ReactNode } from "react";
import { Sidebar } from "./Sidebar";
import { Topbar } from "./Topbar";

/**
 * Single page shell for every route. The `<main>` scrolls; inside it a
 * centered column caps the content at a readable width and applies uniform
 * padding. Every page renders into this column, so there are no more orphan
 * narrow columns or full-bleed pages — width and padding are decided here,
 * once, instead of per page.
 *
 * `max-w-screen-2xl` keeps wide dashboards (Telemetry grids) from stretching
 * edge-to-edge on ultrawide monitors while still giving tables room to breathe.
 */
export function AppShell({ children }: { children: ReactNode }) {
  return (
    <div className="grid grid-cols-[220px_1fr] grid-rows-[48px_1fr] h-screen bg-background text-foreground">
      <Sidebar />
      <Topbar />
      <main className="row-start-2 col-start-2 overflow-y-auto">
        <div className="mx-auto w-full max-w-screen-2xl px-6 py-6">
          {children}
        </div>
      </main>
    </div>
  );
}
