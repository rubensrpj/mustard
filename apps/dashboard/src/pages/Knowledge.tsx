import { useEffect, useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Search, AlertTriangle } from "lucide-react";
import { useStore } from "@/lib/store";
import {
  useProjects,
  fetchKnowledgeBrowse,
  fetchSearchKnowledge,
  fetchFriction,
  type KnowledgeBrowseRow,
  type KnowledgeRow,
  type FrictionEntry,
} from "@/lib/dashboard";
import { Badge } from "@/components/ui/badge";
import { KnowledgeCard } from "@/features/knowledge/KnowledgeCard";
import {
  KnowledgeBadge,
  KIND_BADGE,
  kindFromType,
} from "@/features/knowledge/KnowledgeBadge";
import {
  SectionHeader,
  EmptyState,
  DataCard,
  CollapsibleGroup,
  PageSurface,
  EditorialBand,
} from "@/components/page";
import { relativeTime } from "@/lib/time";
import { useT } from "@/lib/i18n";

/**
 * Knowledge type → i18n key map. Only `convention` is rendered as
 * "CONVENTION" — and only for rows whose backend type is literally
 * `convention`. Friction signals (hook-retry, heavy pipeline) are NOT
 * knowledge: they come from a separate source (friction.json) and render in
 * their own section below.
 */
const TYPE_LABEL_KEYS: Record<string, string> = {
  "entity-cluster": "knowledge.types.entityCluster",
  "naming-pattern": "knowledge.types.namingPattern",
  decision: "knowledge.types.decision",
  lesson: "knowledge.types.lesson",
  convention: "knowledge.types.convention",
  pattern: "knowledge.types.pattern",
};

/** Sort order so "real knowledge" types lead and noisier ones trail. */
const TYPE_ORDER = [
  "decision",
  "pattern",
  "naming-pattern",
  "entity-cluster",
  "convention",
  "lesson",
];
function typeRank(t: string): number {
  const i = TYPE_ORDER.indexOf(t);
  return i === -1 ? TYPE_ORDER.length : i;
}

// Page-level alias of the kind→colour lookup from `KnowledgeBadge`, used to
// theme container styles (not just inline badges) so the friction sub-section
// frame stays consistent with the friction badge swatch.
const typeColor = KIND_BADGE;

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

/**
 * Defensive friction classifier. A legacy `session-knowledge` extractor wrote
 * telemetry rows into `knowledge.json` with a knowledge `type` (`convention` /
 * `pattern`). We classify by the row's real nature — its `name` — not by the
 * stored type, so those rows never pollute "Padrões e decisões".
 */
const FRICTION_NAME_PATTERNS = [/^heavy-pipeline-/, /^high-hook-retry-/, /\.metrics$/];
function isFrictionEntry(row: KnowledgeBrowseRow): boolean {
  return FRICTION_NAME_PATTERNS.some((re) => re.test(row.name));
}

/**
 * Normalize a legacy friction row from `knowledge.json` into the `FrictionEntry`
 * shape used by `friction.json`. Measured counts (`retry_count` / `api_calls`)
 * are not present on `KnowledgeRow`, so they are left null — never invented.
 */
function toFrictionEntry(row: KnowledgeBrowseRow): FrictionEntry {
  return {
    name: row.name,
    description: row.description,
    source: row.source,
    tags: [],
    retry_count: null,
    api_calls: null,
    prescription: null,
    updated_at: null,
  };
}

export function Knowledge() {
  const t = useT();
  const labelType = (typ: string): string => {
    const key = TYPE_LABEL_KEYS[typ];
    return key ? t(key) : typ;
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

  // Browse: all knowledge rows for the active workspace. Event-driven — the
  // FS watcher invalidates ["knowledge-browse"] on NDJSON event-shard writes
  // (kind "events") and on knowledge-file writes (kind "knowledge"), so the
  // 10s poll is gone. staleTime + window-focus refetch remain as a safety net.
  const { data: browseRows, isLoading: browseLoading } = useQuery({
    queryKey: ["knowledge-browse", activeProject?.path],
    queryFn: () => fetchKnowledgeBrowse(activeProject!.path, 500),
    enabled: !!activeProject && !hasQuery,
    staleTime: 60_000,
    refetchOnWindowFocus: true,
  });

  // Search: when query >= 2 chars. Event-driven via the same watcher kinds.
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

  // Split browse rows by real nature: legacy friction telemetry written into
  // knowledge.json (wrong type) is segregated from genuine reusable knowledge.
  const realRows = useMemo<KnowledgeBrowseRow[]>(
    () => (browseRows ?? []).filter((r) => !isFrictionEntry(r)),
    [browseRows],
  );
  // Wave 5 fix (2026-05-20): the legacy `knowledge.json` extractor appended
  // one row per friction event without deduplicating, so `high-hook-retry-*`
  // and `heavy-pipeline-*` series produced 10+ visually identical rows.
  // Dedup by `name` here at the read path — same shape, fewer rows. We keep
  // whichever row has the most recent `updated_at` (lexicographic compare
  // works for ISO-8601 strings; missing dates lose to present ones).
  const legacyFriction = useMemo<FrictionEntry[]>(() => {
    const byName = new Map<string, FrictionEntry>();
    for (const row of browseRows ?? []) {
      if (!isFrictionEntry(row)) continue;
      const entry = toFrictionEntry(row);
      const prev = byName.get(entry.name);
      const newTs = entry.updated_at ?? "";
      const oldTs = prev?.updated_at ?? "";
      if (!prev || newTs > oldTs) {
        byName.set(entry.name, entry);
      }
    }
    return Array.from(byName.values()).sort((a, b) => a.name.localeCompare(b.name));
  }, [browseRows]);

  // Instant in-memory refinement of the browse list when a query is typed.
  const refinedBrowse = useMemo<KnowledgeBrowseRow[]>(() => {
    if (!hasQuery) return realRows;
    const q = trimmed.toLowerCase();
    return realRows.filter(
      (r) =>
        r.name.toLowerCase().includes(q) ||
        r.description?.toLowerCase().includes(q) ||
        r.type.toLowerCase().includes(q),
    );
  }, [realRows, hasQuery, trimmed]);

  // Group browse results by type, real-knowledge types first.
  const grouped = useMemo<[string, KnowledgeBrowseRow[]][]>(() => {
    const source = hasQuery ? refinedBrowse : realRows;
    const map = source.reduce<Record<string, KnowledgeBrowseRow[]>>((acc, row) => {
      (acc[row.type] ??= []).push(row);
      return acc;
    }, {});
    return Object.entries(map).sort(([a], [b]) => typeRank(a) - typeRank(b));
  }, [realRows, refinedBrowse, hasQuery]);

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
                {searchResults.map((row) => (
                  <li
                    key={row.id}
                    className="flex items-baseline gap-2 flex-wrap px-2 py-1.5 rounded hover:bg-muted/40"
                  >
                    <KnowledgeBadge
                      kind={kindFromType(row.type)}
                      label={labelType(row.type)}
                    />
                    <span className="font-mono font-medium text-[13px]">{row.name}</span>
                    {row.description && (
                      <span className="text-muted-foreground text-[12.5px] basis-full pl-1">
                        {truncate(row.description, 160)}
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
              right={browseRows ? `${realRows.length}` : undefined}
            />
            {browseLoading ? (
              <ul className="flex flex-col gap-1">
                {[0, 1, 2].map((i) => (
                  <li key={i} className="h-8 bg-muted rounded animate-pulse" />
                ))}
              </ul>
            ) : realRows.length === 0 ? (
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
                {grouped.map(([type, rows]) => (
                  <div key={type} className="flex flex-col gap-2">
                    <div className="flex items-baseline gap-2">
                      <KnowledgeBadge
                        kind={kindFromType(type)}
                        label={labelType(type)}
                      />
                      <span className="text-[11px] font-mono text-muted-foreground/50">
                        {rows.length}
                      </span>
                    </div>
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-2">
                      {rows.map((row) => (
                        <KnowledgeCard key={row.id} row={row} />
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>

          {/* Atrito */}
          <FrictionSection friction={friction} legacyFriction={legacyFriction} />
        </div>
      )}
    </PageSurface>
  );
}

/** One friction row — measured signal or legacy telemetry. */
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
 * Friction section — measured atrito, kept strictly separate from real
 * knowledge. Two sources feed it: `friction.json` (measured, prescriptive,
 * rare) and legacy telemetry rows that an old `session-knowledge` extractor
 * mis-wrote into `knowledge.json`. The legacy rows are collapsed by default
 * since they carry no measured counts — they are noise, not diagnosis.
 */
function FrictionSection({
  friction,
  legacyFriction,
}: {
  friction: FrictionEntry[] | undefined;
  legacyFriction: FrictionEntry[];
}) {
  const t = useT();
  const measured = friction ?? [];
  const total = measured.length + legacyFriction.length;
  return (
    <section className="flex flex-col gap-3">
      <SectionHeader
        title={t("knowledge.friction.title")}
        description={t("knowledge.friction.description")}
        right={`${total}`}
      />
      {total === 0 ? (
        <EmptyState
          title={t("knowledge.friction.empty.title")}
          description={t("knowledge.friction.empty.description")}
        />
      ) : (
        <DataCard padded>
          {measured.length > 0 && (
            <ul className="flex flex-col divide-y divide-border">
              {measured.map((f) => (
                <FrictionRow key={f.name} f={f} />
              ))}
            </ul>
          )}
          {legacyFriction.length > 0 && (
            <div
              className={
                "rounded bg-muted/30 p-3 mt-3 " +
                typeColor.friction +
                (measured.length > 0 ? " border-t" : "")
              }
            >
              <div className="flex items-baseline gap-2 mb-2">
                <KnowledgeBadge kind="friction" label={t("knowledge.friction.legacy.label")} />
                <span className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground">
                  {t("knowledge.friction.legacy.tag")}
                </span>
              </div>
              <CollapsibleGroup
                label={t("knowledge.friction.legacy.collapse")}
                count={legacyFriction.length}
                hint={t("knowledge.friction.legacy.hint")}
              >
                <ul className="flex flex-col divide-y divide-border mt-2">
                  {legacyFriction.map((f) => (
                    <FrictionRow key={f.name} f={f} />
                  ))}
                </ul>
              </CollapsibleGroup>
            </div>
          )}
        </DataCard>
      )}
    </section>
  );
}
