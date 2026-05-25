// Sessions — list of Claude Code sessions the harness has seen.
//
// W5 (`2026-05-24-mustard-unification`, T5.4). Reads the `sessions` table
// from the active project's `mustard.db` via the new `dashboard_sessions`
// Tauri command. Lives next to the rest of the dashboard pages and follows
// the same primitives (`PageSurface`, `EditorialBand`, `DataCard`,
// `EmptyState`, `StatusDot`) so it inherits the design-system rhythm.
//
// Data flow:
//   useStore(projectsRoot) → fetchSessions(repoPath) → SessionRow[]
//
// The Tauri command is wrapped in `lib/dashboard.ts::fetchSessions`. Live
// tailing is handled by the existing `subscribeFsChange()` listener in
// `lib/watcher.ts` — it now invalidates `["sessions", repoPath]` on every
// `mustard.db` write, so a new SessionStart row appears without the page
// polling.

import { useQuery } from "@tanstack/react-query";
import { ChevronRight } from "lucide-react";
import { useStore } from "@/lib/store";
import { fetchSessions, type SessionRow } from "@/lib/dashboard";
import {
  PageSurface,
  EditorialBand,
  DataCard,
  EmptyState,
  StatusDot,
} from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";
import { cn } from "@/lib/utils";

function StatusBadge({ status }: { status: string }) {
  // Two-state for now (open | closed); the dot already encodes intent so the
  // text label stays small. Anything unknown defaults to neutral.
  const variant = status === "open" ? "active" : "done";
  return (
    <span className="inline-flex items-center gap-1.5 shrink-0">
      <StatusDot variant={variant} pulse={status === "open"} size="sm" />
      <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {status}
      </span>
    </span>
  );
}

function SessionRowItem({ session }: { session: SessionRow }) {
  // The slug is a human handle (e.g. `dev-rubens-2026-05-24-12-30`); fall
  // back to the id for older sessions that may not have one populated.
  const handle = session.slug || session.id;
  const startedRel = relativeTime(session.started_at);
  const activeRel = session.last_activity_at
    ? relativeTime(session.last_activity_at)
    : null;
  return (
    <li
      className={cn(
        "flex items-center gap-3 px-3 py-2.5 border-b border-border/40",
        "last:border-b-0 hover:bg-muted/30 transition-colors",
      )}
    >
      <StatusBadge status={session.status} />
      <div className="flex flex-col flex-1 min-w-0">
        <div className="flex items-center gap-2 min-w-0">
          <span className="font-mono text-[12px] truncate text-foreground">
            {handle}
          </span>
          {session.last_spec && (
            <Badge variant="outline" className="text-[10px] py-0">
              {session.last_spec}
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
          <span title={session.started_at}>started {startedRel}</span>
          {activeRel && (
            <>
              <span aria-hidden>·</span>
              <span title={session.last_activity_at ?? undefined}>
                last activity {activeRel}
              </span>
            </>
          )}
          {session.cwd && (
            <>
              <span aria-hidden>·</span>
              <span className="font-mono truncate" title={session.cwd}>
                {session.cwd}
              </span>
            </>
          )}
        </div>
      </div>
      <ChevronRight
        aria-hidden
        className="h-3.5 w-3.5 shrink-0 text-muted-foreground/50"
      />
    </li>
  );
}

export function Sessions() {
  const projectsRoot = useStore((s) => s.projectsRoot);

  const { data, isLoading, error } = useQuery<SessionRow[]>({
    queryKey: ["sessions", projectsRoot],
    queryFn: () => fetchSessions(projectsRoot!),
    enabled: !!projectsRoot,
    // Watcher-driven (`subscribeFsChange` invalidates this key on every
    // mustard.db write), so a long staleTime is safe. The page never polls.
    staleTime: 30_000,
  });

  if (!projectsRoot) {
    return (
      <PageSurface>
        <EmptyState
          title="Nenhum projeto ativo"
          description="Selecione um projeto na barra lateral para ver suas sessões."
        />
      </PageSurface>
    );
  }

  return (
    <PageSurface>
      <EditorialBand
        title="Sessions"
        subtitle="Histórico de sessões do Claude Code neste workspace"
      />
      {isLoading && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <div
              key={i}
              className="h-12 rounded bg-muted/40 animate-pulse"
            />
          ))}
        </div>
      )}
      {error && (
        <p className="text-destructive text-sm">{(error as Error).message}</p>
      )}
      {data && data.length === 0 && (
        <EmptyState
          title="Sem sessões registradas"
          description="Nenhuma sessão do Claude Code foi registrada neste projeto ainda. Inicie uma para vê-la aqui."
        />
      )}
      {data && data.length > 0 && (
        <DataCard>
          <ul className="flex flex-col">
            {data.map((s) => (
              <SessionRowItem key={s.id} session={s} />
            ))}
          </ul>
        </DataCard>
      )}
    </PageSurface>
  );
}
