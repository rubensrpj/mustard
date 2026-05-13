import type { ReactNode } from "react";
import { Sidebar } from "./Sidebar";
import { Topbar } from "./Topbar";

export function AppShell({ children }: { children: ReactNode }) {
  return (
    <div className="grid grid-cols-[220px_1fr] grid-rows-[48px_1fr] h-screen bg-background text-foreground">
      <Sidebar />
      <Topbar />
      <main className="row-start-2 col-start-2 overflow-y-auto p-5">{children}</main>
    </div>
  );
}
