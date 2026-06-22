// SessionDetail — drill-in for a single Claude Code session.
//
// Reached from the Sessions list (`/sessions/:id`). Renders the SAME rich
// hierarchical trace the specs use — `<ExecutionTrace>` rooted at the session
// (`source={{ kind: "session", … }}`) — so file diffs, tool results and the
// agent/tool nesting all come from one shared component. There is no parallel
// "flat events" view any more.
//
// Live tailing: the watcher invalidates `["trace", "session", repoPath]`-shaped
// keys on `.session/{id}/.events/*.ndjson` writes via prefix match, so the
// trace tails without polling.

import { Link, useParams } from "react-router";
import { useStore } from "@/lib/store";
import { PageSurface, EditorialBand, EmptyState } from "@/components/page";
import { ExecutionTrace } from "@/features/trace/ExecutionTrace";
import { useActiveProjectName } from "@/lib/dashboard";

export function SessionDetail() {
  const { id: rawId } = useParams<{ id: string }>();
  const sessionId = rawId ? decodeURIComponent(rawId) : "";
  const projectsRoot = useStore((s) => s.projectsRoot);
  // Called unconditionally (Rules of Hooks) — it returns `null` when no project
  // is selected, which is exactly the early-return branch below.
  const activeProjectName = useActiveProjectName();

  if (!projectsRoot) {
    return (
      <PageSurface>
        <EmptyState
          title="Nenhum projeto ativo"
          description="Selecione um projeto na barra lateral para ver esta sessão."
        />
      </PageSurface>
    );
  }

  return (
    <PageSurface>
      <EditorialBand
        eyebrow={
          <Link to="/sessions" className="hover:underline">
            Sessions
          </Link>
        }
        title={sessionId}
        subtitle={
          activeProjectName
            ? `Projeto ${activeProjectName} — agentes, ferramentas e diffs`
            : "Trace da sessão — agentes, ferramentas e diffs"
        }
        actions={
          <Link
            to="/sessions"
            className="text-[13px] text-muted-foreground hover:text-foreground border border-border rounded px-2 py-1 shrink-0"
          >
            ← Voltar para Sessions
          </Link>
        }
      />

      <ExecutionTrace
        projectPath={projectsRoot}
        source={{ kind: "session", sessionId }}
      />
    </PageSurface>
  );
}
