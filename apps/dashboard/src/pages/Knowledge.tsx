import { useEffect, useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Search, AlertTriangle } from "lucide-react";
import { useStore } from "@/lib/store";
import {
  useProjects,
  fetchKnowledgeBrowse,
  fetchSearchKnowledge,
  fetchFriction,
  type KnowledgeRow,
  type FrictionEntry,
} from "@/lib/dashboard";
import { Badge } from "@/components/ui/badge";
import { KnowledgeCard } from "@/features/knowledge/KnowledgeCard";
import { KnowledgeBadge, kindFromType } from "@/features/knowledge/KnowledgeBadge";
import {
  SectionHeader,
  EmptyState,
  DataCard,
  PageSurface,
  EditorialBand,
} from "@/components/page";
import { relativeTime } from "@/lib/time";
import { useT } from "@/lib/i18n";

/**
 * Knowledge kind → i18n label key. Rows are projected from the per-spec NDJSON
 * event log and carry exactly two kinds: `decision` and `lesson`. Friction
 * signals (hook-retry, heavy pipeline) are NOT knowledge: they come from a
 * separate source (friction.json) and render in their own section below.
 */
const KIND_LABEL_KEYS: Record<string, string> = {
  decision: "knowledge.types.decision",
  lesson: "knowledge.types.lesson",
};

/** Sort order: decisions lead, lessons trail. */
const KIND_ORDER = ["decision", "lesson"];
function kindRank(k: string): number {
  const i = KIND_ORDER.indexOf(k);
  return i === -1 ? KIND_ORDER.length : i;
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

export function Knowledge() {
  const t = useT();
  const labelKind = (kind: string): string => {
    const key = KIND_LABEL_KEYS[kind];
    return key ? t(key) : kind;
  };
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const persistedQuery = useStore((s) => s.knowledgeQuery);
  const setKnowledgeQuery = useStore((s) => s.setKnowledgeQuery);
  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;

  const [query, setQuery] = useState(persistedQuery);
  const [debouncedQuery, setDebouncedQuery] = useState(persistedQuery);

  useEffect(() => {
    const t = setTimeout(() => {
      setDebouncedQuery(query);
      setKnowledgeQuery(query);
    }, 300);
    return () => clearTimeout(t);
  }, [query, setKnowledgeQuery]);

  const trimmed = debouncedQuery.trim();
  const hasQuery = trimmed.length >= 2;

  // Browse: decision/lesson rows projected from the per-spec NDJSON event
  // log, already sorted ts desc by the backend. Event-driven — the FS watcher
  // invalidates ["knowledge-browse"] on NDJSON event-shard writes (kind
  // "events"), so there is no poll. staleTime + window-focus refetch remain
  // as a safety net.
  const { data: browseRows, isLoading: browseLoading } = useQuery({
    queryKey: ["knowledge-browse", activeProject?.path],
    queryFn: () => fetchKnowledgeBrowse(activeProject!.path, 500),
    enabled: !!activeProject && !hasQuery,
    staleTime: 60_000,
    refetchOnWindowFocus: true,
  });

  // Search: when query >= 2 chars. Event-driven via the same watcher kind.
  const { data: searchRows, isLoading: searchLoading } = useQuery({
    queryKey: ["knowledge-search", activeProject?.path, trimmed],
    queryFn: () => fetchSearchKnowledge(activeProject!.path, trimmed, 200),
    enabled: !!activeProject && hasQuery,
    staleTime: 30_000,
    refetchOnWindowFocus: true,
  });

  // Friction: measured atrito — separate source (friction.json), no watcher
  // kind. Keep a long 60s fallback poll instead of the old 10s.
  const { data: friction } = useQuery({
    queryKey: ["friction", activeProject?.path],
    queryFn: () => fetchFriction(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 60_000,
    refetchOnWindowFocus: true,
    refetchInterval: 60_000,
  });

  const rows = useMemo<KnowledgeRow[]>(() => browseRows ?? [], [browseRows]);

  // Instant in-memory refinement of the browse list while the backend search
  // is in flight — same fields the backend matches (title + body + spec).
  const refinedBrowse = useMemo<KnowledgeRow[]>(() => {
    if (!hasQuery) return rows;
    const q = trimmed.toLowerCase();
    return rows.filter(
      (r) =>
        r.title.toLowerCase().includes(q) ||
        (r.body ?? "").toLowerCase().includes(q) ||
        (r.spec ?? "").toLowerCase().includes(q),
    );
  }, [rows, hasQuery, trimmed]);

  // Group browse results by kind, decisions first. Rows inside a group keep
  // the backend's ts-desc order.
  const grouped = useMemo<[string, KnowledgeRow[]][]>(() => {
    // `refinedBrowse` already returns the full `rows` when there is no query,
    // so it is the single source for both browse and in-flight-refine modes.
    const map = refinedBrowse.reduce<Record<string, KnowledgeRow[]>>((acc, row) => {
      (acc[row.kind] ??= []).push(row);
      return acc;
    }, {});
    return Object.entries(map).sort(([a], [b]) => kindRank(a) - kindRank(b));
  }, [refinedBrowse]);

  const searchResults: KnowledgeRow[] = hasQuery
    ? (searchRows ?? refinedBrowse)
    : [];

  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Knowledge"
        title={t("knowledge.editorialTitle")}
        subtitle={t("knowledge.editorialSubtitle")}
      />

      {/* Search */}
      <div className="relative w-full">
        <Search
          className="absolute left-3 inset-y-0 my-auto h-3.5 w-3.5 text-muted-foreground"
          aria-hidden
        />
        <input
          id="knowledge-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("knowledge.search.placeholder")}
          aria-label={t("knowledge.search.aria")}
          className="w-full pl-9 pr-3 py-2 bg-card border border-border rounded-md text-sm outline-none placeholder:text-muted-foreground focus:border-primary transition-colors"
        />
      </div>

      {/* Gate states */}
      {!projectsRoot ? (
        <EmptyState
          title={t("empty.noRoot.title")}
          description={t("empty.noRoot.descriptionSettings")}
        />
      ) : !activeWorkspaceId ? (
        <EmptyState
          title={t("empty.noWorkspace.title")}
          description={t("empty.noWorkspace.descriptionTop")}
        />
      ) : !activeProject ? (
        <p className="text-[13px] text-muted-foreground">{t("common.loadingDots")}</p>
      ) : hasQuery ? (
        // ── Search mode ─────────────────────────────────────────────────────
        searchLoading ? (
          <ul className="flex flex-col gap-1">
            {[0, 1, 2].map((i) => (
              <li key={i} className="h-8 bg-muted rounded animate-pulse" />
            ))}
          </ul>
        ) : searchResults.length === 0 ? (
          <EmptyState
            title={t("knowledge.searchEmpty.title").replace("{query}", trimmed)}
            description={t("knowledge.searchEmpty.description")}
          />
        ) : (
          <section className="flex flex-col gap-2">
            <SectionHeader title={t("knowledge.section.results")} right={`${searchResults.length}`} />
            <DataCard padded>
              <ul className="flex flex-col gap-0.5 text-sm">
                {searchResults.map((row, i) => (
                  <li
                    key={`${row.ts}-${i}`}
                    className="flex items-baseline gap-2 flex-wrap px-2 py-1.5 rounded hover:bg-muted/40"
                  >
                    <KnowledgeBadge
                      kind={kindFromType(row.kind)}
                      label={labelKind(row.kind)}
                    />
                    <span className="font-mono font-medium text-[13px]">{row.title}</span>
                    {row.spec && (
                      <span className="text-[11px] font-mono text-muted-foreground">
                        {row.spec}
                      </span>
                    )}
                    {row.ts && (
                      <span className="text-[11px] text-muted-foreground/60 ml-auto">
                        {relativeTime(row.ts)}
                      </span>
                    )}
                    {row.body && (
                      <span className="text-muted-foreground text-[12.5px] basis-full pl-1">
                        {truncate(row.body, 160)}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </DataCard>
          </section>
        )
      ) : (
        // ── Browse mode ─────────────────────────────────────────────────────
        <div className="flex flex-col gap-8">
          {/* Padrões & decisões */}
          <section className="flex flex-col gap-3">
            <SectionHeader
              title={t("knowledge.section.patterns.title")}
              description={t("knowledge.section.patterns.description")}
              right={browseRows ? `${rows.length}` : undefined}
            />
            {browseLoading ? (
              <ul className="flex flex-col gap-1">
                {[0, 1, 2].map((i) => (
                  <li key={i} className="h-8 bg-muted rounded animate-pulse" />
                ))}
              </ul>
            ) : rows.length === 0 ? (
              <EmptyState
                title={t("knowledge.empty.noPatterns.title")}
                description={
                  <>
                    {t("knowledge.empty.noPatterns.body.before")}<code className="font-mono">/mustard:feature</code>{t("knowledge.empty.noPatterns.body.or")}<code className="font-mono">/mustard:bugfix</code>{t("knowledge.empty.noPatterns.body.invoke")}<code className="font-mono">/mustard:knowledge</code>{t("knowledge.empty.noPatterns.body.after")}
                  </>
                }
              />
            ) : (
              <div className="flex flex-col gap-6">
                {grouped.map(([kind, kindRows]) => (
                  <div key={kind} className="flex flex-col gap-2">
                    <div className="flex items-baseline gap-2">
                      <KnowledgeBadge
                        kind={kindFromType(kind)}
                        label={labelKind(kind)}
                      />
                      <span className="text-[11px] font-mono text-muted-foreground/50">
                        {kindRows.length}
                      </span>
                    </div>
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-2">
                      {kindRows.map((row, i) => (
                        <KnowledgeCard key={`${row.ts}-${i}`} row={row} />
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>

          {/* Atrito */}
          <FrictionSection friction={friction} />
        </div>
      )}
    </PageSurface>
  );
}

/** One friction row — measured signal from friction.json. */
function FrictionRow({ f }: { f: FrictionEntry }) {
  const t = useT();
  return (
    <li className="flex flex-col gap-1 py-2">
      <div className="flex items-baseline gap-2 flex-wrap">
        <AlertTriangle
          className="h-3.5 w-3.5 text-[--color-accent-mustard] self-center shrink-0"
          aria-hidden
        />
        <KnowledgeBadge kind="friction" />
        <span className="font-mono font-medium text-[13px]">{f.name}</span>
        {f.retry_count != null && (
          <Badge
            variant="outline"
            className="text-[10px] border-[--color-accent-mustard]/40 text-[--color-accent-mustard]"
            title={t("knowledge.friction.retriesTitle")}
          >
            {f.retry_count} {t("knowledge.friction.retries")}
          </Badge>
        )}
        {f.api_calls != null && (
          <Badge
            variant="outline"
            className="text-[10px] border-[--color-accent-mustard]/40 text-[--color-accent-mustard]"
            title={t("knowledge.friction.callsTitle")}
          >
            {f.api_calls} {t("knowledge.friction.calls")}
          </Badge>
        )}
        {f.updated_at && (
          <span className="text-[11px] text-muted-foreground/60 ml-auto">
            {relativeTime(f.updated_at)}
          </span>
        )}
      </div>
      {f.description && (
        <p className="text-[12.5px] text-muted-foreground leading-relaxed pl-6">
          {f.description}
        </p>
      )}
      {f.prescription && (
        <p className="text-[12px] text-[--color-ok]/90 leading-relaxed pl-6">
          {t("knowledge.friction.suggestion")} {f.prescription}
        </p>
      )}
    </li>
  );
}

/**
 * Friction section — measured atrito from `friction.json`, kept strictly
 * separate from real knowledge. (The legacy telemetry rows an old extractor
 * mis-wrote into the knowledge store died with that store — the event channel
 * carries only genuine decision/lesson records.)
 */
function FrictionSection({ friction }: { friction: FrictionEntry[] | undefined }) {
  const t = useT();
  const measured = friction ?? [];
  return (
    <section className="flex flex-col gap-3">
      <SectionHeader
        title={t("knowledge.friction.title")}
        description={t("knowledge.friction.description")}
        right={`${measured.length}`}
      />
      {measured.length === 0 ? (
        <EmptyState
          title={t("knowledge.friction.empty.title")}
          description={t("knowledge.friction.empty.description")}
        />
      ) : (
        <DataCard padded>
          <ul className="flex flex-col divide-y divide-border">
            {measured.map((f) => (
              <FrictionRow key={f.name} f={f} />
            ))}
          </ul>
        </DataCard>
      )}
    </section>
  );
}