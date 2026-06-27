// Activity — the project's work, grouped by human-readable work TYPE.
//
// Replaces the Specs nav entry (spec `dashboard-aba-atividade-agrupar-trabalho`).
// Each session is one unit of work; we group by its `pipeline.kind` work-type
// (`SessionRow.kind`: feature / bugfix / task / tactical-fix) and relabel that
// to a human heading ("Nova funcionalidade", "Correção", "Ajuste", "Mudança
// rápida"). Grouping by `kind` — NOT `category` — is what reveals the lean
// `task`/`bugfix` fast-paths the spec is about: those emit a `pipeline.kind`
// event but never a `skill.invoked`, so their `category` (the skill suffix) is
// null and grouping by it would dump them in the loose bucket, invisible-by-
// type. `category` is only the FALLBACK for older runs predating `pipeline.kind`
// (see `groupingKey`). The row TITLE is the original request
// (`SessionRow.title`); expanding a row reveals the narrative — request ->
// changes -> outcome — and links to the rich `<ExecutionTrace>` drill-in
// (`/sessions/:id`) for the full phases/tools/diffs story.
//
// Why reuse `fetchSessions` rather than `useAggregate`/`useTelemetryTimeline`:
// the per-session fold is the ONLY surface that already carries BOTH the
// work-type (`kind`, from the earliest `pipeline.kind` event) AND the original
// request text (`title`, from `payload.args`). The aggregate/timeline hooks
// expose specs + raw events but no request narrative, so they cannot title a
// work item by its request. Modeled on `pages/Sessions.tsx` (same grouping
// primitives, same watcher-driven refresh on `["sessions", repoPath]`).

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { ChevronRight, ArrowUpRight } from "lucide-react";
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
  CollapsibleGroup,
} from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { useT } from "@/lib/i18n";
import { relativeTime } from "@/lib/time";
import { cn } from "@/lib/utils";

// Sentinel bucket key for a `null` category (a session with no command) — JS
// object keys are strings, so a real `null` cannot be a Map key without
// coercion; this keeps it explicit (mirrors Sessions.tsx).
const NULL_BUCKET = "__null__";

// Fixed front of the section order, by work TYPE. The `pipeline.kind` values
// (`feature`/`bugfix`/`task`/`tactical-fix`) lead; the `category`-fallback
// vocabulary (`analyze`/…) follows. Everything else sorts alphabetically by its
// human label between these and the always-last "Avulsas" bucket.
const PRIORITY_ORDER = ["feature", "task", "bugfix", "tactical-fix", "analyze"];

// The grouping key for one session: the `pipeline.kind` work-type when present
// (the honest type signal that even spec-less lean runs carry), else the
// `category` (skill suffix) so older runs tagged by skill but predating
// `pipeline.kind` still group by type, else the loose null bucket. Keeping
// `kind` PRIMARY is the fix for the review defect — `category` alone misses the
// lean `task`/`bugfix` runs the spec is about (they have a `pipeline.kind`
// event but no `skill.invoked`, so `category` is null for them).
function groupingKey(s: SessionRow): string {
  return s.kind ?? s.category ?? NULL_BUCKET;
}

// Resolve a grouping key (a `kind`/`category` token, or the null sentinel) to
// its human work-TYPE heading. The `activity.kind.*` dictionary covers both the
// `pipeline.kind` and `category` vocabularies; an unmapped token capitalises its
// own value so a new kind still reads sensibly without a code change.
function kindLabel(t: ReturnType<typeof useT>, key: string): string {
  const dictKey = key === NULL_BUCKET ? "activity.kind.__null__" : `activity.kind.${key}`;
  const mapped = t(dictKey, "");
  if (mapped) return mapped;
  return key.charAt(0).toUpperCase() + key.slice(1);
}

// Compact "Mudanças" line: distinct files + tool count, e.g.
// `3 arquivos · 12 tools`. The full file list rides in the title attribute.
function changesText(session: SessionRow): string {
  const files = `${session.files_touched} ${session.files_touched === 1 ? "arquivo" : "arquivos"}`;
  const tools = `${session.tools_used} ${session.tools_used === 1 ? "tool" : "tools"}`;
  return `${files} · ${tools}`;
}

function ActivityRow({ session }: { session: SessionRow }) {
  const t = useT();
  // The handle is the human session slug; the `unknown` attribution-leak
  // bucket has no real handle, so name it plainly.
  const handle = session.is_unknown_bucket
    ? t("activity.unattributed")
    : session.slug || session.id;
  // The title IS the original request; fall back to a plain marker when the
  // request text was not captured (rather than leaking the UUID as a title).
  const title = session.title ?? t("activity.untitled");
  const isOpen = session.status === "open";
  const startedRel = relativeTime(session.started_at);
  const activeRel = session.last_activity_at
    ? relativeTime(session.last_activity_at)
    : null;
  const filesTitle =
    session.files.length > 0
      ? session.files.join("\n")
      : "Nenhum arquivo registrado.";

  return (
    <li
      className={cn(
        "border-b border-border/40 last:border-b-0",
        // The unknown bucket is an attribution leak, not real work — dim it so
        // genuine items stay visually dominant without hiding it.
        session.is_unknown_bucket && "opacity-70",
      )}
    >
      <details className="group">
        <summary
          className={cn(
            "flex items-center gap-3 px-3 py-2.5 cursor-pointer list-none",
            "hover:bg-muted/30 transition-colors",
          )}
        >
          <span className="inline-flex items-center gap-1.5 shrink-0">
            <StatusDot variant={isOpen ? "active" : "done"} pulse={isOpen} size="sm" />
            <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
              {isOpen ? t("activity.outcome.open") : t("activity.outcome.closed")}
            </span>
          </span>
          <div className="flex flex-col flex-1 min-w-0">
            {/* The original request is the title. */}
            <span
              className="font-medium text-[12px] truncate text-foreground"
              title={title}
            >
              {title}
            </span>
            <div className="flex items-center gap-2 text-[11px] text-foreground/80">
              <span className="font-medium" title={filesTitle}>
                {changesText(session)}
              </span>
              <span aria-hidden className="text-muted-foreground">
                ·
              </span>
              <span className="text-muted-foreground">started {startedRel}</span>
              {activeRel && (
                <>
                  <span aria-hidden className="text-muted-foreground">·</span>
                  <span className="text-muted-foreground">last {activeRel}</span>
                </>
              )}
            </div>
          </div>
          <ChevronRight
            aria-hidden
            className="h-3.5 w-3.5 shrink-0 text-muted-foreground/50 transition-transform group-open:rotate-90"
          />
        </summary>

        {/* The narrative, revealed on expand: request -> changes -> outcome,
            then a link out to the full phases/tools/diffs trace. */}
        <div className="px-3 pb-3 pt-1 pl-[2.1rem] flex flex-col gap-2 text-[11px]">
          <div className="flex flex-col gap-0.5">
            <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
              {t("activity.narrative.request")}
            </span>
            <span className="text-foreground/90">{title}</span>
          </div>

          <div className="flex flex-col gap-0.5">
            <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
              {t("activity.narrative.changes")}
            </span>
            <span className="text-foreground/90">{changesText(session)}</span>
            {session.tool_breakdown.length > 0 && (
              <span className="text-muted-foreground">
                {session.tool_breakdown
                  .slice(0, 6)
                  .map((tc) => `${tc.count} ${tc.name}`)
                  .join(" · ")}
              </span>
            )}
            {session.files.length > 0 && (
              <ul className="mt-0.5 flex flex-col gap-0.5 text-muted-foreground">
                {session.files.slice(0, 8).map((f) => (
                  <li key={f} className="font-mono truncate" title={f}>
                    {f}
                  </li>
                ))}
                {session.files.length > 8 && (
                  <li className="text-muted-foreground/60">
                    +{session.files.length - 8}
                  </li>
                )}
              </ul>
            )}
          </div>

          <div className="flex flex-col gap-0.5">
            <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
              {t("activity.narrative.outcome")}
            </span>
            <span className="text-foreground/90">
              {isOpen ? t("activity.outcome.open") : t("activity.outcome.closed")}
              {/* Scope rides as an annotation (not a separate group): the spec
                  distinguishes feature·full from feature·light, but fragmenting
                  the groups by scope hurts scannability — surface it here. */}
              {session.scope && (
                <Badge variant="outline" className="ml-2 text-[10px] py-0 uppercase tracking-wide">
                  {session.scope}
                </Badge>
              )}
              {session.last_spec && (
                <Badge variant="outline" className="ml-2 text-[10px] py-0">
                  {session.last_spec}
                </Badge>
              )}
            </span>
          </div>

          <div className="flex items-center gap-2 pt-0.5">
            <Link
              to={`/sessions/${encodeURIComponent(session.id)}`}
              className="inline-flex items-center gap-1 text-primary hover:underline"
            >
              {t("activity.narrative.openTrace")}
              <ArrowUpRight className="h-3 w-3" aria-hidden />
            </Link>
            <span aria-hidden className="text-muted-foreground/50">·</span>
            <span className="font-mono text-[10px] text-muted-foreground/70 truncate" title={handle}>
              {handle}
            </span>
          </div>
        </div>
      </details>
    </li>
  );
}

interface ActivitySection {
  key: string;
  label: string;
  sessions: SessionRow[];
}

/** Bucket sessions by work TYPE — `pipeline.kind` primary, `category` fallback
 *  (see `groupingKey`) — and order the sections: priority work types first,
 *  then alphabetically by human label, with the loose null bucket always last.
 *  Within a section the backend's arrival order (open-first, last-activity desc)
 *  is preserved; the unknown attribution-leak bucket is pushed to the tail. */
function groupByKind(
  data: SessionRow[] | undefined,
  t: ReturnType<typeof useT>,
): ActivitySection[] | undefined {
  if (!data) return undefined;
  const buckets = new Map<string, SessionRow[]>();
  for (const s of data) {
    const key = groupingKey(s);
    const bucket = buckets.get(key);
    if (bucket) bucket.push(s);
    else buckets.set(key, [s]);
  }
  for (const bucket of buckets.values()) {
    bucket.sort(
      (a, b) => Number(a.is_unknown_bucket) - Number(b.is_unknown_bucket),
    );
  }
  const keys = [...buckets.keys()];
  keys.sort((a, b) => {
    if (a === NULL_BUCKET) return 1;
    if (b === NULL_BUCKET) return -1;
    const ai = PRIORITY_ORDER.indexOf(a);
    const bi = PRIORITY_ORDER.indexOf(b);
    if (ai !== -1 && bi !== -1) return ai - bi;
    if (ai !== -1) return -1;
    if (bi !== -1) return 1;
    return kindLabel(t, a).localeCompare(kindLabel(t, b));
  });
  return keys.map((key) => ({
    key,
    label: kindLabel(t, key),
    sessions: buckets.get(key)!,
  }));
}

export function Activity() {
  const t = useT();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeProjectName = useActiveProjectName();

  const { data, isLoading, error } = useQuery<SessionRow[]>({
    queryKey: ["sessions", projectsRoot],
    queryFn: () => fetchSessions(projectsRoot!),
    enabled: !!projectsRoot,
    // Watcher-driven: `subscribeFsChange` invalidates `["sessions", repoPath]`
    // on every `.session/.events/*.ndjson` write, so a long staleTime is safe
    // and the page never polls.
    staleTime: 30_000,
  });

  const sections = useMemo(() => groupByKind(data, t), [data, t]);

  if (!projectsRoot) {
    return (
      <PageSurface>
        <EmptyState
          title={t("activity.empty.noProject.title")}
          description={t("activity.empty.noProject.description")}
        />
      </PageSurface>
    );
  }

  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Atividade"
        title={t("activity.editorialTitle")}
        subtitle={
          activeProjectName
            ? t("activity.editorialSubtitle.named").replace("{name}", activeProjectName)
            : t("activity.editorialSubtitle")
        }
      />
      {isLoading && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-12 rounded bg-muted/40 animate-pulse" />
          ))}
        </div>
      )}
      {error && (
        <p className="text-destructive text-sm">{(error as Error).message}</p>
      )}
      {sections && sections.length === 0 && (
        <EmptyState
          title={t("activity.empty.none.title")}
          description={t("activity.empty.none.description")}
        />
      )}
      {sections && sections.length > 0 && (
        <div className="flex flex-col gap-3">
          {sections.map((section, idx) => (
            // The first (highest-priority) group opens by default so the most
            // relevant work is visible without a click; the rest collapse to
            // keep a long history scannable.
            <CollapsibleGroup
              key={section.key}
              label={section.label}
              count={section.sessions.length}
              defaultOpen={idx === 0}
            >
              <DataCard>
                <ul className="flex flex-col">
                  {section.sessions.map((s) => (
                    <ActivityRow key={s.id} session={s} />
                  ))}
                </ul>
              </DataCard>
            </CollapsibleGroup>
          ))}
        </div>
      )}
    </PageSurface>
  );
}
