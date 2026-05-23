import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueries, useQueryClient } from "@tanstack/react-query";
import { useLocation } from "react-router";
import { Search } from "lucide-react";
import { useStore } from "@/lib/store";
import {
  useProjects,
  fetchSpecs,
  dashboardSpecCard,
  fetchWorkspaceHealth,
  type SpecCard,
} from "@/lib/dashboard";
import { useT } from "@/lib/i18n";
import { SectionHeader, EmptyState } from "@/components/page";
import { SpecRow } from "@/features/specs/SpecRow";
import { SpecGroupHeader } from "@/features/specs/SpecGroupHeader";
import { SpecChildrenTree } from "@/features/specs/SpecChildrenTree";
import {
  stateFromStatus,
  filterBucket,
  type SpecFilterBucket,
} from "@/features/specs/_shared/stage-from-status";
import { SpecTabBar, type SpecTab } from "@/features/specs/SpecTabBar";
import { SpecDetailDashboard } from "@/features/specs/SpecDetailDashboard";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

type DateFilter = "today" | "7d" | "30d" | "all";

// ── Stage grouping ────────────────────────────────────────────────────────────
// Active specs group by their `state.stage`; terminal specs split into their
// own outcome buckets so cleanup of cancelled/abandoned stays meaningful.
type GroupKey =
  | "analyze"
  | "plan"
  | "execute"
  | "qa_review"
  | "close"
  | "cancelled"
  | "abandoned";

// Render order — earliest active stage first, terminal buckets last.
const GROUP_ORDER: GroupKey[] = [
  "analyze",
  "plan",
  "execute",
  "qa_review",
  "close",
  "cancelled",
  "abandoned",
];

// Terminal groups stay collapsed by default so current work isn't buried.
const COLLAPSED_BY_DEFAULT = new Set<GroupKey>(["close", "cancelled", "abandoned"]);

function groupKeyForCard(card: SpecCard): GroupKey {
  const state = stateFromStatus(card.status);
  if (state.outcome === "completed") return "close";
  if (state.outcome === "cancelled") return "cancelled";
  if (state.outcome === "abandoned") return "abandoned";
  // Active — group by stage. `close` here means the follow-up window.
  switch (state.stage) {
    case "analyze":
      return "analyze";
    case "plan":
      return "plan";
    case "execute":
      return "execute";
    case "qa-review":
      return "qa_review";
    case "close":
      return "close";
    default:
      return "plan";
  }
}

// ── Quick-open dialog ────────────────────────────────────────────────────────
interface SpecQuickOpenDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  cards: SpecCard[];
  onPick: (slug: string) => void;
}

function SpecQuickOpenDialog({
  open,
  onOpenChange,
  cards,
  onPick,
}: SpecQuickOpenDialogProps) {
  const [query, setQuery] = useState("");

  // Reset query each time the dialog opens so the previous search does not
  // leak across opens.
  useEffect(() => {
    if (open) setQuery("");
  }, [open]);

  const q = query.trim().toLowerCase();
  const filtered = q
    ? cards.filter((c) => c.spec.toLowerCase().includes(q))
    : cards;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Abrir spec em nova aba</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          <div className="relative">
            <Search
              className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground"
              aria-hidden
            />
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Buscar por nome…"
              aria-label="Buscar specs"
              className="w-full pl-7 pr-3 py-1.5 bg-card border border-border rounded-md text-[12px] outline-none placeholder:text-muted-foreground focus:border-primary focus-visible:ring-2 focus-visible:ring-[--color-accent-mustard] transition-colors"
            />
          </div>
          <div className="max-h-[360px] overflow-y-auto flex flex-col gap-0.5">
            {filtered.length === 0 ? (
              <p className="px-2 py-4 text-center text-[12px] text-muted-foreground">
                Nenhuma spec encontrada.
              </p>
            ) : (
              filtered.map((c) => (
                <button
                  key={c.spec}
                  type="button"
                  onClick={() => {
                    onPick(c.spec);
                    onOpenChange(false);
                  }}
                  className="flex items-center justify-between gap-2 px-2 py-1.5 rounded text-left text-[12px] hover:bg-muted/60 focus-visible:bg-muted/60 outline-none transition-colors"
                >
                  <span className="font-mono truncate flex-1 min-w-0" title={c.spec}>
                    {c.spec}
                  </span>
                  <span
                    className="text-[10px] uppercase tracking-wide text-muted-foreground shrink-0"
                    title={c.status}
                  >
                    {c.status}
                  </span>
                </button>
              ))
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── Filter / search bar ───────────────────────────────────────────────────────
interface SpecsFilterBarProps {
  bucket: SpecFilterBucket;
  onBucket: (v: SpecFilterBucket) => void;
  date: DateFilter;
  onDate: (v: DateFilter) => void;
  search: string;
  onSearch: (v: string) => void;
}

const BUCKETS: SpecFilterBucket[] = ["ativas", "suspeitas", "encerradas"];

function SpecsFilterBar({
  bucket,
  onBucket,
  date,
  onDate,
  search,
  onSearch,
}: SpecsFilterBarProps) {
  const t = useT();
  const pillBase = "px-2.5 py-1 rounded-md text-[12px] transition-colors duration-100";
  const active = "bg-primary/10 text-primary font-medium";
  const inactive = "text-muted-foreground hover:bg-muted/40 hover:text-foreground";

  return (
    <div className="flex items-center gap-3 flex-wrap">
      {/* Primary state pills */}
      <div className="flex items-center gap-1">
        {BUCKETS.map((b) => (
          <button
            key={b}
            type="button"
            onClick={() => onBucket(b)}
            aria-pressed={bucket === b}
            className={`${pillBase} ${bucket === b ? active : inactive}`}
          >
            {t(`route.specs.filter.${b}`, b)}
          </button>
        ))}
      </div>

      {/* Date filters */}
      <div className="flex items-center gap-1">
        {(["today", "7d", "30d", "all"] as DateFilter[]).map((v) => {
          const label = v === "today" ? "Hoje" : v === "all" ? "Todas" : v;
          return (
            <button
              key={v}
              type="button"
              onClick={() => onDate(v)}
              aria-pressed={date === v}
              className={`${pillBase} ${date === v ? active : inactive}`}
            >
              {label}
            </button>
          );
        })}
      </div>

      {/* Search */}
      <div className="relative flex-1 min-w-[160px]">
        <Search
          className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground"
          aria-hidden
        />
        <input
          value={search}
          onChange={(e) => onSearch(e.target.value)}
          placeholder="Buscar por nome…"
          aria-label="Buscar specs por nome"
          className="w-full pl-7 pr-3 py-1 bg-card border border-border rounded-md text-[12px] outline-none placeholder:text-muted-foreground focus:border-primary focus-visible:ring-2 focus-visible:ring-[--color-accent-mustard] transition-colors"
        />
      </div>
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────
export function Specs() {
  const t = useT();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;
  const queryClient = useQueryClient();
  const location = useLocation();

  // Wave-6: read `?filter=` query param so deep-links from WorkspaceHealthCard work.
  const initialBucket = useMemo<SpecFilterBucket>(() => {
    const params = new URLSearchParams(location.search);
    const f = params.get("filter");
    if (f === "suspects" || f === "suspeitas") return "suspeitas";
    if (f === "encerradas") return "encerradas";
    return "ativas";
  }, [location.search]);

  const [bucket, setBucket] = useState<SpecFilterBucket>(initialBucket);
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");
  const [search, setSearch] = useState("");

  // Expand state — which specs show their children tree, which Stage groups
  // are open. Both reset on unmount (route-local).
  const [expandedSpecs, setExpandedSpecs] = useState<Set<string>>(new Set());
  const [expandedGroups, setExpandedGroups] = useState<Set<GroupKey>>(new Set());
  // Tracks whether the user has manually toggled groups yet — until then the
  // default-open heuristic drives the open set.
  const [groupsTouched, setGroupsTouched] = useState(false);

  function toggleSpec(slug: string) {
    setExpandedSpecs((prev) => {
      const next = new Set(prev);
      if (next.has(slug)) next.delete(slug);
      else next.add(slug);
      return next;
    });
  }

  function toggleGroup(key: GroupKey) {
    setGroupsTouched(true);
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  // Tab state (spec `2026-05-21-dashboard-spec-tabs`): route-local.
  const [tabs, setTabs] = useState<SpecTab[]>([{ id: "list", kind: "list" }]);
  const [activeTabId, setActiveTabId] = useState<string>("list");
  const [quickOpenOpen, setQuickOpenOpen] = useState(false);

  function openSpec(slug: string) {
    setTabs((prev) => {
      const exists = prev.some((t) => t.kind === "spec" && t.specName === slug);
      if (exists) return prev;
      return [...prev, { id: slug, kind: "spec", specName: slug }];
    });
    setActiveTabId(slug);
  }

  function closeSpec(id: string) {
    if (id === "list") return; // never closable
    setTabs((prev) => {
      const idx = prev.findIndex((t) => t.id === id);
      if (idx === -1) return prev;
      const next = prev.filter((t) => t.id !== id);
      if (activeTabId === id) {
        const leftIdx = Math.max(0, idx - 1);
        const leftTab = next[leftIdx] ?? next[0] ?? { id: "list" };
        setActiveTabId(leftTab.id);
      }
      return next;
    });
  }

  function onRefresh() {
    const active = tabs.find((t) => t.id === activeTabId);
    if (!active || active.kind === "list") {
      queryClient.invalidateQueries({ queryKey: ["specs"] });
      queryClient.invalidateQueries({ queryKey: ["spec-card"] });
      queryClient.invalidateQueries({ queryKey: ["spec-children-tree"] });
      return;
    }
    const slug = active.specName;
    queryClient.invalidateQueries({ queryKey: ["spec-card", undefined, slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-card"] });
    queryClient.invalidateQueries({ queryKey: ["spec-waves", slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-quality", slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-children", slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-events", slug] });
  }

  // Hash deep-link: auto-open spec on mount only when the hash looks like a
  // spec slug (date-prefixed).
  useEffect(() => {
    const hash = window.location.hash.replace(/^#/, "");
    if (hash && /^\d{4}-\d{2}-\d{2}-/.test(hash)) openSpec(hash);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Fetch spec list (SpecRow names) then fan out one SpecCard per spec.
  // Wave 3 (2026-05-22): ["specs"] is invalidated by the FS watcher on
  // "spec"/"pipeline-state" changes — drop the 15s poll, keep staleTime.
  const { data: specRows, isLoading: listLoading } = useQuery({
    queryKey: ["specs", activeProject?.path],
    queryFn: () => fetchSpecs(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 10_000,
  });

  // spec-card has no dedicated watcher kind; events arrive via mutations
  // (useSpecAction invalidates it). Keep a long 60s fallback instead of 5s.
  const cardQueries = useQueries({
    queries: (specRows ?? []).map((row) => ({
      queryKey: ["spec-card", activeProject?.path, row.name] as const,
      queryFn: (): Promise<SpecCard> =>
        dashboardSpecCard(activeProject!.path, row.name),
      enabled: !!activeProject,
      staleTime: 5_000,
      refetchInterval: 60_000,
      refetchIntervalInBackground: false,
    })),
  });

  const cards = useMemo<SpecCard[]>(() => {
    return cardQueries
      .map((q) => q.data)
      .filter((d): d is SpecCard => d != null);
  }, [cardQueries]);

  // Wave-6: fetch hygiene health to populate suspect sets for badges + "Suspeitas" filter.
  const { data: healthData } = useQuery({
    queryKey: ["workspace-health", activeProject?.path],
    queryFn: () => fetchWorkspaceHealth(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 10_000,
    // No dedicated watcher kind — long 60s fallback instead of 12s.
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });

  const suspectSpecs = useMemo<ReadonlySet<string>>(
    () => new Set(healthData?.suspect_specs ?? []),
    [healthData],
  );

  // Two-stage loading cascade — see prior spec note: don't flash the empty
  // state while the per-card fan-out is mid-flight.
  const cardsLoading = cardQueries.some((q) => q.isLoading);
  const specsLoading = listLoading || cardsLoading;

  const dateCutoff = useMemo<number>(() => {
    const now = Date.now();
    if (dateFilter === "today") return now - 24 * 60 * 60 * 1000;
    if (dateFilter === "7d") return now - 7 * 24 * 60 * 60 * 1000;
    if (dateFilter === "30d") return now - 30 * 24 * 60 * 60 * 1000;
    return 0;
  }, [dateFilter]);

  const filteredSpecs = useMemo<SpecCard[]>(() => {
    return cards
      .filter((c) => {
        const state = stateFromStatus(c.status);
        const fb = filterBucket(state);
        if (bucket === "suspeitas") {
          // Wave-6: "Suspeitas" shows flag-bearing specs (blocked/wave-failed)
          // UNION hygiene suspects from workspace_health.
          return fb === "suspeitas" || suspectSpecs.has(c.spec);
        }
        return fb === bucket;
      })
      .filter((c) => {
        if (dateCutoff === 0) return true;
        const ts = c.last_event_at ?? c.started_at;
        if (!ts) return true;
        return new Date(ts).getTime() >= dateCutoff;
      })
      .filter((c) => {
        if (!search.trim()) return true;
        return c.spec.toLowerCase().includes(search.trim().toLowerCase());
      });
  }, [cards, bucket, dateCutoff, search, suspectSpecs]);

  // Group filtered specs by Stage, dropping empty groups. Within a group,
  // newest activity first.
  const grouped = useMemo<[GroupKey, SpecCard[]][]>(() => {
    const map = new Map<GroupKey, SpecCard[]>();
    for (const key of GROUP_ORDER) map.set(key, []);
    for (const c of filteredSpecs) map.get(groupKeyForCard(c))!.push(c);
    for (const list of map.values()) {
      list.sort((a, b) => {
        const ta = a.last_event_at ? new Date(a.last_event_at).getTime() : 0;
        const tb = b.last_event_at ? new Date(b.last_event_at).getTime() : 0;
        return tb - ta;
      });
    }
    return GROUP_ORDER.map((k) => [k, map.get(k) ?? []] as [GroupKey, SpecCard[]]).filter(
      ([, list]) => list.length > 0,
    );
  }, [filteredSpecs]);

  // A group renders open when the user has explicitly toggled it open, or —
  // before any manual toggle — when it isn't a terminal bucket.
  function isGroupOpen(key: GroupKey): boolean {
    if (groupsTouched) return expandedGroups.has(key);
    return !COLLAPSED_BY_DEFAULT.has(key);
  }

  // ── Gate cascade ─────────────────────────────────────────────────────────
  if (!projectsRoot) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <EmptyState
          title="Diretório de projetos não configurado"
          description="Vá em Configurações e aponte para a pasta onde estão seus repos."
        />
      </div>
    );
  }

  if (!activeWorkspaceId) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <EmptyState
          title="Selecione um workspace"
          description="Use o seletor na sidebar para escolher um projeto."
        />
      </div>
    );
  }

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];
  const repoPath = activeProject?.path ?? null;

  return (
    <div className="flex flex-col gap-4 w-full">
      <SpecTabBar
        tabs={tabs}
        activeId={activeTabId}
        onActivate={setActiveTabId}
        onClose={closeSpec}
        onAddRequest={() => setQuickOpenOpen(true)}
        onRefresh={onRefresh}
      />

      <SpecQuickOpenDialog
        open={quickOpenOpen}
        onOpenChange={setQuickOpenOpen}
        cards={cards}
        onPick={openSpec}
      />

      {activeTab.kind === "list" ? (
        <div className="flex flex-col gap-6">
          <SpecsFilterBar
            bucket={bucket}
            onBucket={setBucket}
            date={dateFilter}
            onDate={setDateFilter}
            search={search}
            onSearch={setSearch}
          />

          <section className="flex flex-col gap-2">
            <SectionHeader
              title="Specs"
              right={specsLoading ? undefined : String(filteredSpecs.length)}
            />

            {specsLoading ? (
              <ul className="flex flex-col gap-1">
                {[0, 1, 2, 3, 4].map((i) => (
                  <li key={i} className="h-8 bg-muted/40 rounded-md animate-pulse" />
                ))}
              </ul>
            ) : filteredSpecs.length === 0 ? (
              <EmptyState
                title="Nenhuma spec encontrada"
                description="Ajuste os filtros ou rode uma pipeline com /mustard:feature."
              />
            ) : (
              <div className="flex flex-col gap-3">
                {grouped.map(([key, list]) => {
                  const open = isGroupOpen(key);
                  return (
                    <section key={key} className="flex flex-col">
                      <SpecGroupHeader
                        label={t(`route.specs.groups.${key}`, key)}
                        count={list.length}
                        expanded={open}
                        onToggle={() => toggleGroup(key)}
                      />
                      {open && (
                        <div className="flex flex-col">
                          {list.map((s) => {
                            const isExpanded = expandedSpecs.has(s.spec);
                            return (
                              <div key={s.spec} className="flex flex-col">
                                <SpecRow
                                  data={s}
                                  expanded={isExpanded}
                                  onToggle={toggleSpec}
                                  onOpen={openSpec}
                                  suspectSpecs={suspectSpecs}
                                />
                                {isExpanded && repoPath && (
                                  <SpecChildrenTree
                                    spec={s.spec}
                                    projectPath={repoPath}
                                    onOpenParent={openSpec}
                                  />
                                )}
                              </div>
                            );
                          })}
                        </div>
                      )}
                    </section>
                  );
                })}
              </div>
            )}
          </section>
        </div>
      ) : (
        <SpecDetailDashboard repoPath={repoPath} spec={activeTab.specName} />
      )}
    </div>
  );
}
