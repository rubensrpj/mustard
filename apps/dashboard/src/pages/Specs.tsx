import { useEffect, useMemo, useRef, useState } from "react";
import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocation, useNavigate } from "react-router";
import { Search } from "lucide-react";
import { useStore } from "@/lib/store";
import {
  useProjects,
  fetchSpecCards,
  fetchWorkspaceHealth,
  type SpecCard,
} from "@/lib/dashboard";
import { useT } from "@/lib/i18n";
import {
  SectionHeader,
  EmptyState,
  PageSurface,
  EditorialBand,
} from "@/components/page";
import { SpecRow, SpecRowColumnsHeader } from "@/features/specs/SpecRow";
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

// "Specs paradas" (stale) threshold — active specs gone quiet for this long
// (spec `redesenho-rota-visao-geral-dashboard`). Constant, revisable later;
// mirrors the SpecAlertsBand derivation. Module-scoped so it stays a stable
// reference across renders.
const STALE_CUTOFF_MS = 7 * 24 * 60 * 60 * 1000;

// ── Stage grouping ────────────────────────────────────────────────────────────
// Active specs group by their `state.stage`; terminal specs split into their
// own outcome buckets so cleanup of cancelled/abandoned stays meaningful.
type GroupKey =
  | "analyze"
  | "plan"
  | "execute"
  | "qa_review"
  | "awaiting_close"
  | "close"
  | "cancelled"
  | "abandoned"
  | "superseded"
  | "absorbed";

// Render order — earliest active stage first, terminal buckets last.
// `awaiting_close` (waves done, QA/close pending) sits after qa_review and
// before the terminal `close` bucket — near-done but still active.
const GROUP_ORDER: GroupKey[] = [
  "analyze",
  "plan",
  "execute",
  "qa_review",
  "awaiting_close",
  "close",
  "cancelled",
  "abandoned",
  "superseded",
  "absorbed",
];

// Terminal groups stay collapsed by default so current work isn't buried.
const COLLAPSED_BY_DEFAULT = new Set<GroupKey>([
  "close",
  "cancelled",
  "abandoned",
  "superseded",
  "absorbed",
]);

function groupKeyForCard(card: SpecCard): GroupKey {
  // "awaiting-close" — waves done, QA/close gate pending — gets its own group
  // (labelled "Aguardando fechamento"), distinct from the execute and completed
  // buckets. Checked off the raw status word since `stateFromStatus` folds it
  // onto the qa-review stage for the bullet/filters.
  if (card.status === "awaiting-close") return "awaiting_close";
  const state = stateFromStatus(card.status);
  if (state.outcome === "completed") return "close";
  if (state.outcome === "cancelled") return "cancelled";
  if (state.outcome === "abandoned") return "abandoned";
  if (state.outcome === "superseded") return "superseded";
  if (state.outcome === "absorbed") return "absorbed";
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
  const t = useT();
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
          <DialogTitle>{t("specs.quickOpen.title")}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          <div className="relative">
            <Search
              className="absolute left-2.5 inset-y-0 my-auto h-3 w-3 text-muted-foreground"
              aria-hidden
            />
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("specs.quickOpen.placeholder")}
              aria-label={t("specs.quickOpen.searchAria")}
              className="w-full pl-7 pr-3 py-1.5 bg-card border border-border rounded-md text-[12px] outline-none placeholder:text-muted-foreground focus:border-primary focus-visible:ring-2 focus-visible:ring-[--color-accent-mustard] transition-colors"
            />
          </div>
          <div className="max-h-[360px] overflow-y-auto flex flex-col gap-0.5">
            {filtered.length === 0 ? (
              <p className="px-2 py-4 text-center text-[12px] text-muted-foreground">
                {t("specs.quickOpen.empty")}
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
          const label = v === "today" ? t("specs.filterBar.date.today") : v === "all" ? t("specs.filterBar.date.all") : v;
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
          className="absolute left-2.5 inset-y-0 my-auto h-3 w-3 text-muted-foreground"
          aria-hidden
        />
        <input
          value={search}
          onChange={(e) => onSearch(e.target.value)}
          placeholder={t("specs.quickOpen.placeholder")}
          aria-label={t("specs.filterBar.searchAria")}
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
  const navigate = useNavigate();

  // Sub-filter by lifecycle stage, layered on top of the `ativas` bucket. The
  // Visão Geral stage cards (spec `redesenho-rota-visao-geral-dashboard`) deep-
  // link here with `?filter=planejando|executando`. `null` = no stage narrowing.
  type StageFilter = "planning" | "executing" | null;

  // Wave-6: read `?filter=` query param so deep-links from WorkspaceHealthCard
  // and the Visão Geral stage cards / alerts band work. Returns the resolved
  // bucket plus an optional stage sub-filter and the `stale` (specs paradas) flag.
  const { initialBucket, initialStage, initialStale } = useMemo<{
    initialBucket: SpecFilterBucket;
    initialStage: StageFilter;
    initialStale: boolean;
  }>(() => {
    const params = new URLSearchParams(location.search);
    const f = params.get("filter");
    if (f === "suspects" || f === "suspeitas")
      return { initialBucket: "suspeitas", initialStage: null, initialStale: false };
    if (f === "encerradas" || f === "finalizadas")
      return { initialBucket: "encerradas", initialStage: null, initialStale: false };
    if (f === "planejando")
      return { initialBucket: "ativas", initialStage: "planning", initialStale: false };
    if (f === "executando")
      return { initialBucket: "ativas", initialStage: "executing", initialStale: false };
    if (f === "stale")
      return { initialBucket: "ativas", initialStage: null, initialStale: true };
    return { initialBucket: "ativas", initialStage: null, initialStale: false };
  }, [location.search]);

  const [bucket, setBucket] = useState<SpecFilterBucket>(initialBucket);
  // Stage sub-filter + stale flag come from the deep-link only; clicking a
  // bucket pill clears them so the manual filters stay intuitive.
  const [stageFilter, setStageFilter] = useState<StageFilter>(initialStage);
  const [staleOnly, setStaleOnly] = useState<boolean>(initialStale);

  function selectBucket(b: SpecFilterBucket) {
    setBucket(b);
    setStageFilter(null);
    setStaleOnly(false);
  }

  // Re-sync from the URL when a deep-link arrives while the page is already
  // mounted (router keeps the component, only `location.search` changes).
  useEffect(() => {
    setBucket(initialBucket);
    setStageFilter(initialStage);
    setStaleOnly(initialStale);
  }, [initialBucket, initialStage, initialStale]);
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
  // Monotonic nonce bumped on every wave-bearing `openSpec` call. It rides on
  // the tab as `initialWaveNonce` so `SpecDetailDashboard` re-selects the wave
  // even when it is the SAME wave the user clicked before (e.g. they closed the
  // split panel and clicked the same wave-child again). Keying the detail's
  // effect on the wave value alone would no-op on a same-wave re-click.
  const waveNonceRef = useRef(0);

  // `wave` (optional) pre-selects a wave in the spec's Ondas tab — set when the
  // user clicks a wave-child row in the list tree so the wave panel opens
  // straight away. Re-opening an already-open spec tab with a fresh wave bumps
  // both `initialWave` and `initialWaveNonce` so `SpecDetailDashboard` re-selects
  // that wave (the nonce makes a same-wave re-click fire too).
  function openSpec(slug: string, wave?: number) {
    const nonce = wave != null ? (waveNonceRef.current += 1) : undefined;
    setTabs((prev) => {
      const idx = prev.findIndex(
        (t) => t.kind === "spec" && t.specName === slug,
      );
      if (idx === -1) {
        return [
          ...prev,
          { id: slug, kind: "spec", specName: slug, initialWave: wave, initialWaveNonce: nonce },
        ];
      }
      // Tab already open — refresh its initialWave only when a wave was given,
      // so a plain re-open (no wave) never clobbers a wave the user has since
      // navigated to inside the tab.
      if (wave == null) return prev;
      const next = [...prev];
      next[idx] = {
        ...next[idx],
        kind: "spec",
        specName: slug,
        initialWave: wave,
        initialWaveNonce: nonce,
      };
      return next;
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
    const path = activeProject?.path;
    const active = tabs.find((t) => t.id === activeTabId);
    if (!active || active.kind === "list") {
      // List view renders from the batch `spec-cards`; refresh that + its
      // source + the health badges, not every detail card.
      queryClient.invalidateQueries({ queryKey: ["spec-cards", path] });
      queryClient.invalidateQueries({ queryKey: ["specs", path] });
      queryClient.invalidateQueries({ queryKey: ["workspace-health", path] });
      queryClient.invalidateQueries({ queryKey: ["spec-children-tree"] });
      return;
    }
    const slug = active.specName;
    // `["spec-card", path, slug]` — the leaf must carry the repo path, which
    // is the key shape the spec-detail card query registers (the old
    // `undefined` never matched).
    queryClient.invalidateQueries({ queryKey: ["spec-card", path, slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-cards", path] });
    queryClient.invalidateQueries({ queryKey: ["spec-waves", slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-quality", slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-children", slug] });
    queryClient.invalidateQueries({ queryKey: ["spec-events", slug] });
  }

  // Hash deep-link (`/specs#{slug}`, from Atividade) is rendered below as a
  // clean, list-free spec drill-in (see `deepLinkSpec`). No tab/effect needed:
  // the router's `location.hash` drives it directly, so a hash change re-renders.

  // ONE batch query replaces the old list query + per-spec fan-out (spec
  // `sidebar-lento-lista-specs-dispara`): `dashboard_spec_cards` resolves the
  // cached spec list backend-side and folds the workspace events ONCE for all
  // cards, instead of N parallel `dashboard_spec_card` calls each re-folding
  // the whole slice. spec-cards is refreshed by the `dashboard:specs-snapshot`
  // push (watcher) and by mutations (useSpecAction / onRefresh); the 60s poll
  // is just the live fallback, like the per-card queries had.
  const cardsQuery = useQuery({
    queryKey: ["spec-cards", activeProject?.path],
    queryFn: (): Promise<SpecCard[]> => fetchSpecCards(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 10_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
    // Keep the previous project's cards on screen while a switch/refetch is in
    // flight, so the list never blanks to a full-page skeleton after first
    // paint (spec `melhorias-no-dashboard-destacar-projeto`, wave 1).
    placeholderData: keepPreviousData,
  });

  const cards = useMemo<SpecCard[]>(
    () => cardsQuery.data ?? [],
    [cardsQuery.data],
  );

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

  // Loading gate — only the genuine first paint (no cached cards yet) shows
  // the skeleton. With `keepPreviousData`, refetches and project switches keep
  // the prior list on screen (`isPlaceholderData`) instead of blanking the
  // whole section, so the skeleton no longer "trava a tela inteira" on every
  // refresh (spec `melhorias-no-dashboard-destacar-projeto`, wave 1).
  const specsLoading = cardsQuery.isLoading && cards.length === 0;

  const dateCutoff = useMemo<number>(() => {
    const now = Date.now();
    if (dateFilter === "today") return now - 24 * 60 * 60 * 1000;
    if (dateFilter === "7d") return now - 7 * 24 * 60 * 60 * 1000;
    if (dateFilter === "30d") return now - 30 * 24 * 60 * 60 * 1000;
    return 0;
  }, [dateFilter]);

  const filteredSpecs = useMemo<SpecCard[]>(() => {
    const staleCutoff = Date.now() - STALE_CUTOFF_MS;
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
        // Stage sub-filter (deep-link from the Visão Geral stage cards). Only
        // narrows within `ativas`; terminal specs already left the bucket.
        if (!stageFilter) return true;
        const { stage } = stateFromStatus(c.status);
        if (stageFilter === "executing") return stage === "execute";
        // "planning" = everything active that isn't executing yet.
        return stage !== "execute";
      })
      .filter((c) => {
        // "Specs paradas" deep-link: keep only active specs gone quiet for >= 7d.
        if (!staleOnly) return true;
        if (stateFromStatus(c.status).outcome !== "active") return false;
        const ts = c.last_event_at ?? c.started_at;
        if (!ts) return false;
        const ms = Date.parse(ts);
        return Number.isFinite(ms) && ms < staleCutoff;
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
  }, [cards, bucket, stageFilter, staleOnly, dateCutoff, search, suspectSpecs]);

  // Group filtered specs by Stage, dropping empty groups. Within every group,
  // sort by creation date (`started_at`) newest-first; specs with no
  // `started_at` sort last. `filteredSpecs` keeps its source order, so the sort
  // is stable for ties / nulls (the `??` keeps null-vs-null at 0).
  const grouped = useMemo<[GroupKey, SpecCard[]][]>(() => {
    const map = new Map<GroupKey, SpecCard[]>();
    for (const key of GROUP_ORDER) map.set(key, []);
    for (const c of filteredSpecs) map.get(groupKeyForCard(c))!.push(c);
    const createdAt = (c: SpecCard): number => {
      if (!c.started_at) return Number.NEGATIVE_INFINITY; // nulls last (desc)
      const ms = Date.parse(c.started_at);
      return Number.isFinite(ms) ? ms : Number.NEGATIVE_INFINITY;
    };
    for (const list of map.values()) {
      list.sort((a, b) => createdAt(b) - createdAt(a));
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
      <PageSurface>
        <EmptyState
          title={t("empty.noRoot.title")}
          description={t("empty.noRoot.description")}
        />
      </PageSurface>
    );
  }

  if (!activeWorkspaceId) {
    return (
      <PageSurface>
        <EmptyState
          title={t("empty.noWorkspace.title")}
          description={t("empty.noWorkspace.description")}
        />
      </PageSurface>
    );
  }

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];
  const repoPath = activeProject?.path ?? null;

  // A `/specs#{slug}` deep-link (from Atividade) opens ONE spec as a clean,
  // list-free drill-in: a back button + the spec detail, with NO "Specs" header
  // and NO tab bar / "Lista" tab. Read the ROUTER hash — under HashRouter
  // `window.location.hash` holds the whole route (`#/specs#slug`), so reading it
  // raw yields a bogus slug. No hash → the normal list page below.
  const deepLinkSpec = location.hash ? decodeURIComponent(location.hash.replace(/^#/, "")) : "";
  if (deepLinkSpec) {
    return (
      <PageSurface>
        <button
          type="button"
          onClick={() => navigate(-1)}
          className="self-start inline-flex items-center gap-1 text-[13px] text-muted-foreground hover:text-foreground -ml-1 px-1 py-0.5 rounded"
        >
          ← {t("common.back", "Voltar")}
        </button>
        <SpecDetailDashboard repoPath={repoPath} spec={deepLinkSpec} />
      </PageSurface>
    );
  }

  return (
    <PageSurface>
      <div className="flex flex-col gap-2">
        <button
          type="button"
          onClick={() => navigate(-1)}
          className="self-start inline-flex items-center gap-1 text-[13px] text-muted-foreground hover:text-foreground -ml-1 px-1 py-0.5 rounded"
        >
          ← {t("common.back", "Voltar")}
        </button>
        <EditorialBand
          eyebrow="Specs"
          title={t("specs.editorialTitle")}
          subtitle={t("specs.editorialSubtitle")}
        />
      </div>
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
            onBucket={selectBucket}
            date={dateFilter}
            onDate={setDateFilter}
            search={search}
            onSearch={setSearch}
          />

          <section className="flex flex-col gap-2">
            <SectionHeader
              title={t("specs.section.specs")}
              right={specsLoading ? undefined : String(filteredSpecs.length)}
            />

            {specsLoading ? (
              <ul className="flex flex-col gap-1">
                {[0, 1, 2, 3, 4].map((i) => (
                  <li key={i} className="h-8 bg-muted rounded-md animate-pulse" />
                ))}
              </ul>
            ) : filteredSpecs.length === 0 ? (
              <EmptyState
                title={t("specs.empty.noneFound.title")}
                description={t("specs.empty.noneFound.description")}
              />
            ) : (
              <div className="flex flex-col gap-3">
                {grouped.map(([key, list]) => {
                  const open = isGroupOpen(key);
                  // The "Planejando" group ran nothing yet — render the
                  // created/idle columns + Reanalisar instead of the metric
                  // columns (spec `melhorias-pagina-specs`, item 3).
                  const rowVariant = key === "plan" ? "planning" : "default";
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
                          <SpecRowColumnsHeader variant={rowVariant} />
                          {list.map((s) => {
                            const isExpanded = expandedSpecs.has(s.spec);
                            return (
                              <div key={s.spec} className="flex flex-col">
                                <SpecRow
                                  data={s}
                                  expanded={isExpanded}
                                  onToggle={toggleSpec}
                                  onOpen={openSpec}
                                  variant={rowVariant}
                                  repoPath={repoPath}
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
        <SpecDetailDashboard
          repoPath={repoPath}
          spec={activeTab.specName}
          initialWave={activeTab.initialWave}
          initialWaveNonce={activeTab.initialWaveNonce}
        />
      )}
    </PageSurface>
  );
}
