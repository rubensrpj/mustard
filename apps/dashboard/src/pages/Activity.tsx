import { useState, useMemo, useDeferredValue } from "react";
import { AmendActivityBlock } from "@/components/amend/AmendActivityBlock";
import { useQuery } from "@tanstack/react-query";
import { useStore } from "@/lib/store";
import { discoverProjects } from "@/api/discovery";
import { useActivityFeed } from "@/hooks/useActivityFeed";
import { fetchActivityAggregated, type RecentEvent } from "@/lib/dashboard";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { relativeTime } from "@/lib/time";
import { parseQaOverall } from "@/lib/qa";
import { shortSpecName } from "@/lib/phaseTheme";
import {
  PageHeader,
  SectionHeader,
  EmptyState,
  DataCard,
  EventChip,
  PhaseChip,
} from "@/components/page";
import { cn } from "@/lib/utils";

const PAGE_SIZE = 50;
const LIMIT_PER_PROJECT = 100;

/** Status dot variant inferred from event semantics. */
function eventVariant(event: RecentEvent): StatusDotVariant {
  switch (event.event_type) {
    case "tool.use":
    case "commit-gate.check":
      return "idle";
    case "pipeline.phase":
      return "planning";
    case "qa.result": {
      const overall = parseQaOverall(event.summary);
      if (overall === "pass") return "success";
      if (overall === "fail") return "error";
      if (overall === "skip") return "planning";
      return "idle";
    }
    case "agent.start":
    case "session.start":
      return "active";
    case "dispatch.failure":
      return "error";
    default:
      return "idle";
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

/**
 * Filter section: label on the left, scrollable row of chips on the right.
 * Used four times in the Raw tab (agent / wave / spec / type).
 */
function FilterChips<T extends string | number>({
  label,
  options,
  active,
  onToggle,
  format = (v) => String(v),
  hint,
}: {
  label: string;
  options: T[];
  active: Set<T>;
  onToggle: (v: T) => void;
  format?: (v: T) => string;
  hint?: string;
}) {
  if (options.length === 0) return null;
  return (
    <div className="flex items-start gap-2 min-w-0">
      <div className="flex flex-col shrink-0 w-14 pt-0.5">
        <span className="text-[10px] uppercase tracking-wider text-muted-foreground/70">
          {label}
        </span>
        {hint && (
          <span className="text-[9.5px] text-muted-foreground/50">{hint}</span>
        )}
      </div>
      <div className="flex flex-wrap gap-1 min-w-0 flex-1">
        {options.map((opt) => {
          const isActive = active.has(opt);
          return (
            <button
              key={String(opt)}
              type="button"
              onClick={() => onToggle(opt)}
              className="cursor-pointer transition-transform hover:scale-105 active:scale-95"
            >
              <Badge
                variant={isActive ? "default" : "outline"}
                className={cn(
                  "text-[11px] py-0 font-mono",
                  isActive && "shadow-sm",
                )}
              >
                {format(opt)}
              </Badge>
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function Activity() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);

  const { data: projects } = useQuery({
    queryKey: ["discover", projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const activeProject = (projects ?? []).find((p) => p.id === activeWorkspaceId) ?? null;

  const { events, types, loading } = useActivityFeed(
    activeProject ? [activeProject] : [],
    LIMIT_PER_PROJECT,
  );

  const [activeTypes, setActiveTypes] = useState<Set<string>>(new Set());
  const [activeAgents, setActiveAgents] = useState<Set<string>>(new Set());
  const [activeWaves, setActiveWaves] = useState<Set<number>>(new Set());
  const [activeSpecs, setActiveSpecs] = useState<Set<string>>(new Set());
  const [searchText, setSearchText] = useState("");
  const deferredSearch = useDeferredValue(searchText);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);

  const { data: aggGroups, isLoading: aggLoading } = useQuery({
    queryKey: ["activity-agg", activeProject?.path],
    queryFn: () => fetchActivityAggregated(activeProject!.path, 200),
    enabled: !!activeProject,
    staleTime: 15_000,
  });

  // Filter options derived from feed
  const agents = useMemo(
    () => Array.from(new Set(events.map((r) => r.event.actor_id).filter(Boolean) as string[])).sort(),
    [events],
  );
  const waves = useMemo(
    () =>
      Array.from(new Set(events.map((r) => r.event.wave).filter((w): w is number => w != null)))
        .sort((a, b) => a - b),
    [events],
  );
  const specs = useMemo(
    () => Array.from(new Set(events.map((r) => r.event.spec).filter(Boolean) as string[])).sort(),
    [events],
  );

  const filtered = useMemo(() => {
    const q = deferredSearch.trim().toLowerCase();
    return events.filter((row) => {
      if (activeTypes.size > 0 && !activeTypes.has(row.event.event_type)) return false;
      if (activeAgents.size > 0 && !activeAgents.has(row.event.actor_id ?? "")) return false;
      if (activeWaves.size > 0 && (row.event.wave == null || !activeWaves.has(row.event.wave))) return false;
      if (activeSpecs.size > 0 && !activeSpecs.has(row.event.spec ?? "")) return false;
      if (q) {
        const target = (row.event.target ?? "").toLowerCase();
        const summary = (row.event.summary ?? "").toLowerCase();
        if (!target.includes(q) && !summary.includes(q)) return false;
      }
      return true;
    });
  }, [events, activeTypes, activeAgents, activeWaves, activeSpecs, deferredSearch]);

  const visible = filtered.slice(0, visibleCount);
  const hasMore = filtered.length > visible.length;

  const toggle = <T,>(s: Set<T>, v: T): Set<T> => {
    const next = new Set(s);
    if (next.has(v)) next.delete(v);
    else next.add(v);
    return next;
  };

  if (!projectsRoot) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader breadcrumb={["Mustard", "Atividade"]} title="Atividade" />
        <EmptyState
          title="Configure o diretório de projetos"
          description="Vá em Configurações e aponte para a pasta onde estão seus repos."
        />
      </div>
    );
  }

  if (!activeWorkspaceId) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader breadcrumb={["Mustard", "Atividade"]} title="Atividade" />
        <EmptyState
          title="Selecione um workspace"
          description="Use o seletor no topo para escolher um projeto e ver os eventos."
        />
      </div>
    );
  }

  if (!activeProject) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader breadcrumb={["Mustard", "Atividade"]} title="Atividade" />
        <p className="text-[13px] text-muted-foreground">Carregando…</p>
      </div>
    );
  }

  const hasAnyFilter =
    activeTypes.size > 0 ||
    activeAgents.size > 0 ||
    activeWaves.size > 0 ||
    activeSpecs.size > 0 ||
    deferredSearch.trim().length > 0;

  return (
    <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={["Mustard", "Atividade", { label: activeProject.name, mono: true }]}
        title="Atividade"
        subtitle={activeProject.name}
        description={
          <>
            Eventos do harness Mustard em ordem cronológica. Cada evento é um
            sinal de algo que aconteceu — ferramenta usada, agente despachado,
            QA validado, fase trocada. <strong className="text-foreground">Timeline</strong>{" "}
            agrupa eventos por spec/wave/ação. <strong className="text-foreground">Eventos</strong>{" "}
            mostra o stream cru com filtros.
          </>
        }
      />

      <Tabs defaultValue="timeline" className="w-full">
        <TabsList>
          <TabsTrigger value="timeline">Timeline</TabsTrigger>
          <TabsTrigger value="raw">Eventos ({events.length})</TabsTrigger>
        </TabsList>

        {/* ── Timeline tab ─────────────────────────────────────────────────── */}
        <TabsContent value="timeline" className="mt-4">
          <section className="flex flex-col gap-3 w-full">
            <SectionHeader
              title="Grupos agregados"
              description="Ações similares (mesmo spec + wave + tipo de ação) agrupadas em uma linha só. Útil pra ver o panorama sem afogar em eventos individuais."
              right={aggGroups ? `${aggGroups.length} grupos` : ""}
            />
            {aggLoading ? (
              <DataCard padded>
                <ul className="flex flex-col gap-2">
                  {[0, 1, 2, 3].map((i) => (
                    <li key={i} className="h-10 bg-muted/40 rounded animate-pulse" />
                  ))}
                </ul>
              </DataCard>
            ) : !aggGroups || aggGroups.length === 0 ? (
              <EmptyState
                title="Nenhum grupo de atividade ainda"
                description="Quando você rodar uma pipeline, eventos vão aparecer aqui agrupados por spec e wave."
              />
            ) : (
              <DataCard>
                <ul className="flex flex-col">
                  {aggGroups.map((g, i) => (
                    <li
                      key={i}
                      className="flex flex-wrap items-baseline gap-2 px-4 py-2 hover:bg-muted/15 text-[13px] border-t border-border/40 first:border-t-0 transition-colors"
                    >
                      {g.spec && (
                        <Badge variant="secondary" className="text-[11px] py-0 font-mono">
                          {shortSpecName(g.spec)}
                        </Badge>
                      )}
                      {g.wave != null && (
                        <span
                          className="inline-flex items-center rounded-md px-1.5 py-0 text-[10px] font-medium bg-primary/15 text-primary border border-primary/30 tabular-nums"
                          title={`Wave ${g.wave}`}
                        >
                          W{g.wave}
                        </span>
                      )}
                      <span className="font-mono text-foreground/90">{g.action_kind}</span>
                      <span className="text-muted-foreground/80 text-[12px]">
                        {g.count} {g.count === 1 ? "ação" : "ações"}
                      </span>
                      <span className="text-muted-foreground font-mono text-[11.5px] tabular-nums">
                        {g.tokens_total.toLocaleString()} tok
                      </span>
                      <span className="text-muted-foreground/70 text-[11.5px]">
                        {g.files_touched} {g.files_touched === 1 ? "arquivo" : "arquivos"}
                      </span>
                      {g.min_ts && g.max_ts && (
                        <span className="text-muted-foreground/60 text-[11.5px] ml-auto whitespace-nowrap">
                          {relativeTime(g.min_ts)} → {relativeTime(g.max_ts)}
                        </span>
                      )}
                      {g.spec && <AmendActivityBlock specId={g.spec} />}
                    </li>
                  ))}
                </ul>
              </DataCard>
            )}
          </section>
        </TabsContent>

        {/* ── Raw events tab ───────────────────────────────────────────────── */}
        <TabsContent value="raw" className="mt-4">
          <section className="flex flex-col gap-3 w-full">
            <SectionHeader
              title="Filtros"
              description="Combine search + chips pra refinar o stream. Quanto mais chips ativos, mais estreita a busca."
              right={
                hasAnyFilter && (
                  <button
                    type="button"
                    onClick={() => {
                      setActiveTypes(new Set());
                      setActiveAgents(new Set());
                      setActiveWaves(new Set());
                      setActiveSpecs(new Set());
                      setSearchText("");
                      setVisibleCount(PAGE_SIZE);
                    }}
                    className="text-[11px] text-primary hover:text-primary underline-offset-2 hover:underline"
                  >
                    limpar filtros
                  </button>
                )
              }
            />

            <DataCard padded>
              <div className="flex flex-col gap-3 w-full">
                <Input
                  placeholder="Buscar em target (arquivo, comando, padrão) ou summary…"
                  value={searchText}
                  onChange={(e) => {
                    setSearchText(e.target.value);
                    setVisibleCount(PAGE_SIZE);
                  }}
                  className="text-[13px] h-8"
                />
                <div className="flex flex-col gap-2.5">
                  <FilterChips
                    label="Tipo"
                    hint={`${types.length}`}
                    options={types}
                    active={activeTypes}
                    onToggle={(t) => {
                      setActiveTypes((p) => toggle(p, t));
                      setVisibleCount(PAGE_SIZE);
                    }}
                  />
                  <FilterChips
                    label="Agente"
                    hint={`${agents.length}`}
                    options={agents}
                    active={activeAgents}
                    onToggle={(a) => {
                      setActiveAgents((p) => toggle(p, a));
                      setVisibleCount(PAGE_SIZE);
                    }}
                  />
                  <FilterChips
                    label="Wave"
                    hint={`${waves.length}`}
                    options={waves}
                    active={activeWaves}
                    onToggle={(w) => {
                      setActiveWaves((p) => toggle(p, w));
                      setVisibleCount(PAGE_SIZE);
                    }}
                    format={(w) => `W${w}`}
                  />
                  <FilterChips
                    label="Spec"
                    hint={`${specs.length}`}
                    options={specs.slice(0, 10)}
                    active={activeSpecs}
                    onToggle={(s) => {
                      setActiveSpecs((p) => toggle(p, s));
                      setVisibleCount(PAGE_SIZE);
                    }}
                    format={(s) => shortSpecName(s)}
                  />
                </div>
              </div>
            </DataCard>

            <SectionHeader
              title="Stream de eventos"
              description="Newest first. Cada linha = um evento individual emitido por algum hook do Mustard."
              right={
                <span className="font-mono tabular-nums">
                  {visible.length} / {filtered.length}
                </span>
              }
            />

            {loading ? (
              <DataCard padded>
                <ul className="flex flex-col gap-1">
                  {[0, 1, 2, 3, 4].map((i) => (
                    <li key={i} className="h-6 bg-muted/40 rounded animate-pulse" />
                  ))}
                </ul>
              </DataCard>
            ) : filtered.length === 0 ? (
              <EmptyState
                title={hasAnyFilter ? "Nenhum evento bate com os filtros atuais" : "Sem eventos"}
                description={
                  hasAnyFilter
                    ? "Tente limpar alguns filtros ou alargar a busca."
                    : "Eventos aparecem aqui à medida que pipelines rodam."
                }
              />
            ) : (
              <DataCard>
                <ul className="flex flex-col">
                  {visible.map((row, i) => {
                    const variant = eventVariant(row.event);
                    const overall =
                      row.event.event_type === "qa.result"
                        ? parseQaOverall(row.event.summary)
                        : null;
                    return (
                      <li
                        key={`${row.projectId}-${i}-${row.event.ts ?? ""}`}
                        className="flex items-baseline gap-2 px-4 py-1.5 hover:bg-muted/15 border-t border-border/40 first:border-t-0 transition-colors min-w-0"
                      >
                        <StatusDot
                          variant={variant}
                          pulse={variant === "active"}
                          className="self-center shrink-0"
                        />
                        <EventChip eventType={row.event.event_type} overall={overall ?? undefined} size="sm" />
                        {row.event.tool_name && (
                          <Badge variant="outline" className="text-[10px] py-0 font-mono shrink-0">
                            {row.event.tool_name}
                          </Badge>
                        )}
                        {row.event.phase && (
                          <PhaseChip phase={row.event.phase} size="sm" />
                        )}
                        {row.event.target && (
                          <code
                            className="text-[11.5px] text-muted-foreground/80 font-mono truncate min-w-0 flex-1"
                            title={row.event.target}
                          >
                            {row.event.target}
                          </code>
                        )}
                        {!row.event.target && row.event.summary && (
                          <span
                            className="text-muted-foreground text-[12px] truncate min-w-0 flex-1"
                            title={row.event.summary}
                          >
                            {truncate(row.event.summary, 200)}
                          </span>
                        )}
                        {row.event.spec && (
                          <span
                            className="text-[11px] text-muted-foreground/70 font-mono shrink-0"
                            title={row.event.spec}
                          >
                            {shortSpecName(row.event.spec)}
                          </span>
                        )}
                        {row.event.wave != null && (
                          <span
                            className="inline-flex items-center rounded-md px-1 py-0 text-[10px] font-medium bg-primary/15 text-primary border border-primary/30 tabular-nums shrink-0"
                            title={`Wave ${row.event.wave}`}
                          >
                            W{row.event.wave}
                          </span>
                        )}
                        {row.event.ts && (
                          <span className="text-muted-foreground/60 text-[11px] ml-auto shrink-0 tabular-nums whitespace-nowrap">
                            {relativeTime(row.event.ts)}
                          </span>
                        )}
                      </li>
                    );
                  })}
                </ul>
                {hasMore && (
                  <div className="border-t border-border/40 p-2 flex justify-center">
                    <button
                      type="button"
                      onClick={() => setVisibleCount((n) => n + PAGE_SIZE)}
                      className="text-[12px] text-muted-foreground hover:text-foreground border border-border/60 rounded px-3 py-1 transition-colors"
                    >
                      Carregar mais {PAGE_SIZE} eventos
                    </button>
                  </div>
                )}
              </DataCard>
            )}
          </section>
        </TabsContent>
      </Tabs>
    </div>
  );
}
