import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueries } from "@tanstack/react-query";
import {
  Search,
  PlayCircle,
  Eye,
  AlertOctagon,
  CheckCircle2,
  CircleDashed,
} from "lucide-react";
import { useStore } from "@/lib/store";
import {
  useProjects,
  fetchSpecs,
  dashboardSpecCard,
  type SpecCard,
} from "@/lib/dashboard";
import {
  PageHeader,
  SectionHeader,
  EmptyState,
} from "@/components/page";
import { SpecCard as SpecCardComponent } from "@/components/specs/SpecCard";
import { SpecDrillDown } from "@/components/specs/SpecDrillDown";

// ── Phase ordering for active specs ──────────────────────────────────────────
const PHASE_ORDER = ["analyze", "plan", "execute", "qa", "close"];
function phaseRank(phase: string): number {
  const i = PHASE_ORDER.indexOf(phase.toLowerCase());
  return i === -1 ? PHASE_ORDER.length : i;
}

type StatusFilter = "ativas" | "encerradas" | "todas";
type DateFilter = "today" | "7d" | "30d" | "all";

// ── Inline SpecsTopBar ────────────────────────────────────────────────────────
interface SpecsTopBarProps {
  status: StatusFilter;
  onStatus: (v: StatusFilter) => void;
  date: DateFilter;
  onDate: (v: DateFilter) => void;
  search: string;
  onSearch: (v: string) => void;
}

function SpecsTopBar({
  status,
  onStatus,
  date,
  onDate,
  search,
  onSearch,
}: SpecsTopBarProps) {
  const btnBase =
    "px-2.5 py-1 rounded text-[12px] transition-colors duration-100";
  const active = "bg-primary/10 text-primary font-medium";
  const inactive = "text-muted-foreground hover:bg-muted/40 hover:text-foreground";

  return (
    <div className="flex items-center gap-3 flex-wrap">
      {/* Status filters */}
      <div className="flex items-center gap-1">
        {(["ativas", "encerradas", "todas"] as StatusFilter[]).map((v) => (
          <button
            key={v}
            type="button"
            onClick={() => onStatus(v)}
            aria-pressed={status === v}
            className={`${btnBase} ${status === v ? active : inactive}`}
          >
            {v.charAt(0).toUpperCase() + v.slice(1)}
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
              className={`${btnBase} ${date === v ? active : inactive}`}
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

// ── Row + drill-down combo (used by flat and grouped renderings) ────────────
interface SpecRowProps {
  s: SpecCard;
  repoPath: string | null;
  expanded: boolean;
  onToggle: () => void;
}

function SpecRow({ s, repoPath, expanded, onToggle }: SpecRowProps) {
  return (
    <div className="flex flex-col">
      {/* Clicking the card header area toggles drill-down; the FileText
          button and SpecActionMenu have stopPropagation internally. */}
      <div
        role="button"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
        className="cursor-pointer"
        aria-expanded={expanded}
      >
        <SpecCardComponent data={s} repoPath={repoPath} />
      </div>
      {expanded && (
        <div className="mt-1 ml-2 border-l-2 border-border/40 pl-3">
          <SpecDrillDown repoPath={repoPath} spec={s.spec} />
        </div>
      )}
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────
export function Specs() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;

  // Default to "ativas" — the primary use-case is "what's running now",
  // not "everything ever". The legacy default of "todas" buried current
  // work under closed history (spec 2026-05-20-dashboard-ux-honest).
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("ativas");
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);

  // Hash deep-link: auto-expand spec on mount
  useEffect(() => {
    const hash = window.location.hash.replace(/^#/, "");
    if (hash) setExpanded(hash);
  }, []);

  // Fetch spec list (SpecRow[])
  const { data: specRows, isLoading: listLoading } = useQuery({
    queryKey: ["specs", activeProject?.path],
    queryFn: () => fetchSpecs(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 10_000,
    refetchInterval: 15_000,
  });

  // Fan-out: fetch SpecCard for each spec. Wave 5 fix (2026-05-20): every
  // card polls on a 5-second cadence so an active pipeline animates without
  // the user having to refocus the window. The legacy hooks only had
  // `staleTime: 10_000` which left the UI frozen between user interactions.
  const cardQueries = useQueries({
    queries: (specRows ?? []).map((row) => ({
      queryKey: ["spec-card", activeProject?.path, row.name] as const,
      queryFn: (): Promise<SpecCard> =>
        dashboardSpecCard(activeProject!.path, row.name),
      enabled: !!activeProject,
      staleTime: 5_000,
      refetchInterval: 5_000,
      refetchIntervalInBackground: false,
    })),
  });

  const cards = useMemo<SpecCard[]>(() => {
    return cardQueries
      .map((q) => q.data)
      .filter((d): d is SpecCard => d != null);
  }, [cardQueries]);

  // Date cutoff
  const dateCutoff = useMemo<number>(() => {
    const now = Date.now();
    if (dateFilter === "today") return now - 24 * 60 * 60 * 1000;
    if (dateFilter === "7d") return now - 7 * 24 * 60 * 60 * 1000;
    if (dateFilter === "30d") return now - 30 * 24 * 60 * 60 * 1000;
    return 0;
  }, [dateFilter]);

  // Mirrors `mustard_specsdb::SpecStatus::is_active` on the client side.
  // The Wave-4 adapter emits the kebab-case status strings; the legacy
  // forms (`"active"`, `"closed"`) are accepted so an out-of-date row in
  // the `specs` table does not flicker. `"no-events"` is explicitly
  // *not* active — a spec that the harness has not touched yet doesn't
  // belong in the "Ativas" filter.
  const TERMINAL_STATUSES = new Set([
    "completed",
    "closed",
    "cancelled",
    "no-events",
  ]);
  const isActive = (c: SpecCard) => !TERMINAL_STATUSES.has(c.status);

  // Status → group bucket (used when statusFilter === "todas" so the list
  // is grouped instead of a flat 70+ row scroll). Order matches the spec:
  // Ativas → Em revisão → Bloqueadas → Concluídas → Sem eventos.
  type GroupKey = "ativas" | "revisao" | "bloqueadas" | "concluidas" | "sem-eventos";
  const GROUP_ORDER: GroupKey[] = [
    "ativas",
    "revisao",
    "bloqueadas",
    "concluidas",
    "sem-eventos",
  ];
  const GROUP_META: Record<
    GroupKey,
    { label: string; Icon: typeof PlayCircle }
  > = {
    ativas: { label: "Ativas", Icon: PlayCircle },
    revisao: { label: "Em revisão", Icon: Eye },
    bloqueadas: { label: "Bloqueadas", Icon: AlertOctagon },
    concluidas: { label: "Concluídas", Icon: CheckCircle2 },
    "sem-eventos": { label: "Sem eventos", Icon: CircleDashed },
  };
  function groupKeyForStatus(status: string): GroupKey {
    if (status === "no-events") return "sem-eventos";
    if (status === "blocked" || status === "wave-failed") return "bloqueadas";
    if (status === "reviewing" || status === "qa") return "revisao";
    if (TERMINAL_STATUSES.has(status)) return "concluidas";
    return "ativas";
  }

  const filteredSpecs = useMemo<SpecCard[]>(() => {
    return cards
      .filter((c) => {
        if (statusFilter === "ativas" && !isActive(c)) return false;
        if (statusFilter === "encerradas" && isActive(c)) return false;
        return true;
      })
      .filter((c) => {
        if (dateCutoff === 0) return true;
        const ts = c.last_event_at ?? c.started_at;
        // Wave 5 fix (2026-05-20): a spec without any harness events still
        // exists on disk (e.g. just created, not yet dispatched). Older
        // builds eliminated it from every window filter, which left "Hoje"
        // and "7d" perpetually empty. We now keep no-events specs visible
        // (treat them as "not yet placed in time") so the user can still
        // see and click them; chronological filters only drop specs that
        // *do* have a timestamp and fall outside the window.
        if (!ts) return true;
        return new Date(ts).getTime() >= dateCutoff;
      })
      .filter((c) => {
        if (!search.trim()) return true;
        return c.spec.toLowerCase().includes(search.trim().toLowerCase());
      })
      .sort((a, b) => {
        const aActive = isActive(a);
        const bActive = isActive(b);
        if (aActive !== bActive) return aActive ? -1 : 1;
        if (aActive) return phaseRank(a.phase) - phaseRank(b.phase);
        // Closed: reverse chronological
        const ta = a.last_event_at ? new Date(a.last_event_at).getTime() : 0;
        const tb = b.last_event_at ? new Date(b.last_event_at).getTime() : 0;
        return tb - ta;
      });
  }, [cards, statusFilter, dateCutoff, search]);

  // When the user explicitly asks for "todas", surface the spec mix as
  // semantic groups instead of a flat list. The flat list still wins for
  // "ativas" / "encerradas" since those filters are already single-bucket.
  const groupedByStatus = useMemo<[GroupKey, SpecCard[]][]>(() => {
    if (statusFilter !== "todas") return [];
    const map = new Map<GroupKey, SpecCard[]>();
    for (const key of GROUP_ORDER) map.set(key, []);
    for (const c of filteredSpecs) {
      const key = groupKeyForStatus(c.status);
      map.get(key)!.push(c);
    }
    return GROUP_ORDER.map((k) => [k, map.get(k) ?? []] as [GroupKey, SpecCard[]])
      .filter(([, list]) => list.length > 0);
  }, [statusFilter, filteredSpecs]);

  // ── Gate cascade ─────────────────────────────────────────────────────────
  if (!projectsRoot) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: "Workspace" }, { label: "Specs" }]}
          title="Specs"
          subtitle="Lista e drill-down por spec"
        />
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
        <PageHeader
          breadcrumb={[{ label: "Workspace" }, { label: "Specs" }]}
          title="Specs"
          subtitle="Lista e drill-down por spec"
        />
        <EmptyState
          title="Selecione um workspace"
          description="Use o seletor na sidebar para escolher um projeto."
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={[{ label: "Workspace" }, { label: "Specs" }]}
        title="Specs"
        subtitle="Lista e drill-down por spec"
      />

      <SpecsTopBar
        status={statusFilter}
        onStatus={setStatusFilter}
        date={dateFilter}
        onDate={setDateFilter}
        search={search}
        onSearch={setSearch}
      />

      <section className="flex flex-col gap-3">
        <SectionHeader
          title="Specs"
          right={listLoading ? undefined : String(filteredSpecs.length)}
        />

        {listLoading ? (
          <ul className="flex flex-col gap-2">
            {[0, 1, 2].map((i) => (
              <li key={i} className="h-20 bg-muted/40 rounded-lg animate-pulse" />
            ))}
          </ul>
        ) : filteredSpecs.length === 0 ? (
          <EmptyState
            title="Nenhuma spec encontrada"
            description="Ajuste os filtros ou rode uma pipeline com /mustard:feature."
          />
        ) : statusFilter === "todas" ? (
          <div className="flex flex-col gap-5">
            {groupedByStatus.map(([key, list]) => {
              const meta = GROUP_META[key];
              const Icon = meta.Icon;
              return (
                <section key={key} className="flex flex-col gap-2">
                  <header className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-muted-foreground">
                    <Icon className="h-3.5 w-3.5" aria-hidden />
                    <span className="font-medium">{meta.label}</span>
                    <span className="tabular-nums text-muted-foreground/60"
                      style={{ fontVariantNumeric: "tabular-nums" }}
                    >
                      {list.length}
                    </span>
                  </header>
                  <div className="flex flex-col gap-2">
                    {list.map((s) => (
                      <SpecRow
                        key={s.spec}
                        s={s}
                        repoPath={activeProject?.path ?? null}
                        expanded={expanded === s.spec}
                        onToggle={() =>
                          setExpanded((prev) => (prev === s.spec ? null : s.spec))
                        }
                      />
                    ))}
                  </div>
                </section>
              );
            })}
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {filteredSpecs.map((s) => (
              <SpecRow
                key={s.spec}
                s={s}
                repoPath={activeProject?.path ?? null}
                expanded={expanded === s.spec}
                onToggle={() =>
                  setExpanded((prev) => (prev === s.spec ? null : s.spec))
                }
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
