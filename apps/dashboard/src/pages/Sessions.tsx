// Sessions — list of Claude Code sessions the harness has seen.
//
// Aggregates one row per session from the active project's
// `.claude/.session/{id}/.events/*.ndjson` event logs via the
// `dashboard_sessions` Tauri command. Lives next to the rest of the dashboard pages and follows
// the same primitives (`PageSurface`, `EditorialBand`, `DataCard`,
// `EmptyState`, `StatusDot`) so it inherits the design-system rhythm.
//
// Data flow:
//   useStore(projectsRoot) → fetchSessions(repoPath) → SessionRow[]
//
// The Tauri command is wrapped in `lib/dashboard.ts::fetchSessions`. Live
// tailing is handled by the existing `subscribeFsChange()` listener in
// `lib/watcher.ts` — it invalidates `["sessions", repoPath]` on every
// `.session/{id}/.events/*.ndjson` write, so a new SessionStart row appears
// without the page polling.

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { ChevronRight } from "lucide-react";
import { useStore } from "@/lib/store";
import {
  fetchSessions,
  useActiveProjectName,
  type SessionRow,
} from "@/lib/dashboard";
import {
  PageSurface,
  EditorialBand,
  DataCard,
  EmptyState,
  StatusDot,
  SectionHeader,
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

// Render the tool breakdown compactly, e.g. `4 Read · 2 Edit · 1 Bash`.
// Capped so a busy session can't overflow the row; the full list lives in the
// drill-in. The `+N` tail signals there are more tools than shown.
function breakdownText(session: SessionRow, cap = 4): string {
  if (session.tool_breakdown.length === 0) return "";
  const shown = session.tool_breakdown.slice(0, cap);
  const rest = session.tool_breakdown.length - shown.length;
  const head = shown.map((t) => `${t.count} ${t.name}`).join(" · ");
  return rest > 0 ? `${head} · +${rest}` : head;
}

function SessionRowItem({ session }: { session: SessionRow }) {
  // The slug is a human handle (e.g. `dev-rubens-2026-05-24-12-30`); fall
  // back to the id for older sessions that may not have one populated. The
  // `unknown` attribution-leak bucket has no real handle, so name it plainly.
  const handle = session.is_unknown_bucket
    ? "(sessão não atribuída)"
    : session.slug || session.id;
  const startedRel = relativeTime(session.started_at);
  const activeRel = session.last_activity_at
    ? relativeTime(session.last_activity_at)
    : null;
  // The fold: "what was done / adjusted". Hover the file count to see the list.
  const breakdown = breakdownText(session);
  const filesTitle =
    session.files.length > 0
      ? session.files.join("\n")
      : "Nenhum arquivo registrado nesta sessão.";
  return (
    <li
      className={cn(
        // The unknown bucket is an attribution leak, not a real session —
        // dim it so live sessions stay visually dominant without hiding it.
        session.is_unknown_bucket && "opacity-70",
      )}
    >
      <Link
        to={`/sessions/${encodeURIComponent(session.id)}`}
        className={cn(
          "flex items-center gap-3 px-3 py-2.5 border-b border-border/40",
          "last:border-b-0 hover:bg-muted/30 transition-colors",
        )}
      >
        <StatusBadge status={session.status} />
        <div className="flex flex-col flex-1 min-w-0">
          <div className="flex items-center gap-2 min-w-0">
            {/* The request text (when present) is the title; the UUID/handle
                drops to a smaller secondary line below. Falls back to the
                handle as the title for sessions with no captured request. */}
            <span
              className="font-mono font-medium text-[12px] truncate text-foreground"
              title={session.title ?? handle}
            >
              {session.title ?? handle}
            </span>
            {session.is_unknown_bucket && (
              <Badge
                variant="outline"
                className="text-[10px] py-0 border-amber-500/50 text-amber-500"
                title="Eventos cujo session_id não pôde ser resolvido na emissão — agrupados no balde 'unknown'."
              >
                não atribuída
              </Badge>
            )}
            {session.last_spec && (
              <Badge variant="outline" className="text-[10px] py-0">
                {session.last_spec}
              </Badge>
            )}
          </div>
          {/* The UUID/handle as a quiet secondary line — only when the title
              came from the request (otherwise the handle is already the title). */}
          {session.title && (
            <span className="font-mono text-[10px] text-muted-foreground truncate">
              {handle}
            </span>
          )}
          {/* The fold summary: what was adjusted (files) + done (tools). */}
          <div className="flex items-center gap-2 text-[11px] text-foreground/80">
            <span className="font-medium" title={filesTitle}>
              {session.files_touched}{" "}
              {session.files_touched === 1 ? "arquivo" : "arquivos"}
            </span>
            <span aria-hidden className="text-muted-foreground">
              ·
            </span>
            <span>
              {session.tools_used} {session.tools_used === 1 ? "tool" : "tools"}
            </span>
            {breakdown && (
              <span className="text-muted-foreground truncate" title={breakdown}>
                ({breakdown})
              </span>
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
            <span aria-hidden>·</span>
            <span className="font-mono">
              {session.event_count}{" "}
              {session.event_count === 1 ? "evento" : "eventos"}
            </span>
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
      </Link>
    </li>
  );
}

// Friendly labels for the known categories. The key `"__null__"` stands in for
// a `null` category (a session with no command) — the sentinel used as the
// bucket key below. Unmapped categories capitalise their own value.
const CATEGORY_LABELS: Record<string, string> = {
  feature: "Feature",
  bugfix: "Bugfix",
  task: "Task",
  scan: "Scan",
  knowledge: "Conhecimento",
  qa: "QA",
  outros: "Outros",
  __null__: "Avulsas (sem comando)",
};

// Sentinel bucket key for the `null` category — JS object keys are strings, so
// a real `null` can't be a Map key without coercion; this keeps it explicit.
const NULL_BUCKET = "__null__";

// Fixed front of the section order; everything else sorts alphabetically by
// label between these and the always-last "Avulsas" bucket.
const PRIORITY_ORDER = ["feature", "bugfix", "task"];

function categoryLabel(category: string): string {
  const mapped = CATEGORY_LABELS[category];
  if (mapped) return mapped;
  // Unmapped category — capitalise its own value (e.g. "review" → "Review").
  return category.charAt(0).toUpperCase() + category.slice(1);
}

interface SessionSection {
  key: string;
  label: string;
  sessions: SessionRow[];
}

/** Bucket sessions by `category` and order the sections. Within a section the
 *  backend's arrival order is preserved (open-first, then last-activity desc),
 *  with the `unknown` attribution-leak bucket pushed to the tail. */
function groupSessions(
  data: SessionRow[] | undefined,
): SessionSection[] | undefined {
  if (!data) return undefined;
  const buckets = new Map<string, SessionRow[]>();
  for (const s of data) {
    const key = s.category ?? NULL_BUCKET;
    const bucket = buckets.get(key);
    if (bucket) bucket.push(s);
    else buckets.set(key, [s]);
  }
  // Stable within-section order: preserve arrival, leak bucket last.
  for (const bucket of buckets.values()) {
    bucket.sort(
      (a, b) => Number(a.is_unknown_bucket) - Number(b.is_unknown_bucket),
    );
  }
  const keys = [...buckets.keys()];
  keys.sort((a, b) => {
    // "Avulsas" (null bucket) always last.
    if (a === NULL_BUCKET) return 1;
    if (b === NULL_BUCKET) return -1;
    const ai = PRIORITY_ORDER.indexOf(a);
    const bi = PRIORITY_ORDER.indexOf(b);
    // Both in the priority front → keep priority order.
    if (ai !== -1 && bi !== -1) return ai - bi;
    // Priority front beats the alphabetical tail.
    if (ai !== -1) return -1;
    if (bi !== -1) return 1;
    // Neither prioritised → alphabetical by friendly label.
    return categoryLabel(a).localeCompare(categoryLabel(b));
  });
  return keys.map((key) => ({
    key,
    label: key === NULL_BUCKET ? CATEGORY_LABELS[NULL_BUCKET] : categoryLabel(key),
    sessions: buckets.get(key)!,
  }));
}

export function Sessions() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeProjectName = useActiveProjectName();

  const { data, isLoading, error } = useQuery<SessionRow[]>({
    queryKey: ["sessions", projectsRoot],
    queryFn: () => fetchSessions(projectsRoot!),
    enabled: !!projectsRoot,
    // Watcher-driven (`subscribeFsChange` invalidates this key on every
    // `.session/.events/*.ndjson` write), so a long staleTime is safe. The
    // page never polls.
    staleTime: 30_000,
  });

  // Group by `category` into sections. The backend already returns rows
  // open-first then last-activity desc, so we preserve arrival order while
  // bucketing (only the unknown leak is pushed to the tail within its bucket).
  // Section order: feature, bugfix, task, then the rest alphabetically by
  // label, with "Avulsas" (the null-category bucket) always last.
  const sections = useMemo(() => groupSessions(data), [data]);

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
        subtitle={
          activeProjectName
            ? `Sessões do projeto ${activeProjectName}`
            : "Histórico de sessões do Claude Code neste workspace"
        }
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
      {sections && sections.length === 0 && (
        <EmptyState
          title="Sem sessões registradas"
          description="Nenhuma sessão do Claude Code foi registrada neste projeto ainda. Inicie uma para vê-la aqui."
        />
      )}
      {sections && sections.length > 0 && (
        <div className="flex flex-col gap-6">
          {sections.map((section) => (
            <div key={section.key} className="flex flex-col gap-2">
              <SectionHeader
                title={section.label}
                right={section.sessions.length}
              />
              <DataCard>
                <ul className="flex flex-col">
                  {section.sessions.map((s) => (
                    <SessionRowItem key={s.id} session={s} />
                  ))}
                </ul>
              </DataCard>
            </div>
          ))}
        </div>
      )}
    </PageSurface>
  );
}
