import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link, useSearchParams } from "react-router";
import { useStore } from "@/lib/store";
import {
  useProjects,
  fetchTelemetry,
  fetchActivePipelines,
  fetchQualityMetrics,
  fetchLiveActivity,
  fetchRecentEvents,
  fetchSpecMarkdown,
  fetchSpecs,
} from "@/lib/dashboard";
import { usePromptEconomy, useCollectorHealth } from "@/hooks/usePromptEconomy";
import type { CollectorHealth } from "@/api/promptEconomy";
import { parseQaOverall } from "@/lib/qa";
import type {
  HookFireCount,
  RoutingBlock,
  PhaseCount,
  ToolCount,
  ActivePipeline,
  QualityMetrics,
  LiveActivity,
  PhaseActivity,
  RecentEvent,
  AgentActivityBlock,
} from "@/lib/dashboard";
import { formatNumber, formatTokens, formatPct, formatDurationMs, formatUsd } from "@/lib/format";
import { StatusDot } from "@/components/StatusDot";
import { LivePipelineCard } from "@/components/LivePipelineCard";
import { Markdown } from "@/components/Markdown";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { SplitDetail } from "@/components/layout/SplitDetail";
import { WaveNav } from "@/components/WaveNav";
import { resolveWaveFamily } from "@/lib/waves";
import { relativeTime } from "@/lib/time";
import { cn } from "@/lib/utils";
import { RefreshCw, X } from "lucide-react";

// Canonical pipeline vocabulary — must match refs/canonical-phases.md.
// REVIEW sits between EXECUTE and QA: it already emits real events but was
// invisible while older vocabularies omitted it.
const PHASES = ["ANALYZE", "PLAN", "EXECUTE", "REVIEW", "QA", "CLOSE"] as const;
const REFRESH_FAST = 3_000;
const REFRESH_SLOW = 30_000;

function useTicker(intervalMs = 1_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}


export function Telemetry() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const [searchParams, setSearchParams] = useSearchParams();
  const projects = useProjects();
  const selectedProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;
  const path = selectedProject?.path ?? null;

  const live = useQuery({
    queryKey: ["live", path],
    queryFn: () => fetchLiveActivity(path!),
    enabled: !!path,
    staleTime: REFRESH_FAST,
    refetchInterval: REFRESH_FAST,
  });
  const pipelines = useQuery({
    queryKey: ["active-pipelines", path],
    queryFn: () => fetchActivePipelines(path!),
    enabled: !!path,
    staleTime: REFRESH_FAST,
    refetchInterval: REFRESH_FAST,
  });
  const tele = useQuery({
    queryKey: ["telemetry", path],
    queryFn: () => fetchTelemetry(path!),
    enabled: !!path,
    staleTime: REFRESH_SLOW,
    refetchInterval: REFRESH_SLOW,
  });
  const quality = useQuery({
    queryKey: ["quality", path],
    queryFn: () => fetchQualityMetrics(path!),
    enabled: !!path,
    staleTime: REFRESH_SLOW,
    refetchInterval: REFRESH_SLOW,
  });
  // Recent qa.result events for the real QA pass/fail card. fetchRecentEvents
  // returns the project's last N events; we filter on the client. Limit 200
  // is enough to surface "last hour of QA" without bloating the response.
  const recentEvents = useQuery({
    queryKey: ["recent-events-qa", path],
    queryFn: () => fetchRecentEvents(path!, 200),
    enabled: !!path,
    staleTime: REFRESH_SLOW,
    refetchInterval: REFRESH_SLOW,
  });
  const promptEconomy = usePromptEconomy(path);
  // Unified collector badge — same hook/source as the Prompt Economy page, so
  // both screens always render the identical OTEL state at the same time.
  const collectorHealth = useCollectorHealth(path);

  const [selectedPipeline, setSelectedPipeline] = useState<ActivePipeline | null>(null);
  const [selectedPhase, setSelectedPhase] = useState<PhaseActivity | null>(null);

  const hasSelection = !!selectedPipeline || !!selectedPhase;

  function selectPipeline(p: ActivePipeline) {
    setSelectedPhase(null);
    setSelectedPipeline(p);
  }
  function selectPhase(p: PhaseActivity) {
    setSelectedPipeline(null);
    setSelectedPhase(p);
  }
  function closeAll() {
    setSelectedPipeline(null);
    setSelectedPhase(null);
  }

  const livePipelines = useMemo(() => {
    const list = pipelines.data ?? [];
    const now = Date.now();
    return list
      .filter((p) => (p.phase ?? "").toUpperCase() === "EXECUTE")
      .filter((p) => {
        if (!p.updated_at) return false;
        const t = new Date(p.updated_at).getTime();
        return Number.isFinite(t) && now - t < 24 * 3600 * 1000;
      })
      .slice(0, 3);
  }, [pipelines.data]);

  if (!projectsRoot) {
    return (
      <EmptyState
        title="Configure o diretório de projetos"
        hint="Vá em Settings e aponte para a pasta onde estão seus repos."
      />
    );
  }
  if (!activeWorkspaceId || !selectedProject) {
    return (
      <EmptyState
        title="Selecione um workspace"
        hint="Use o seletor no topo da sidebar para escolher um projeto."
      />
    );
  }

  const isLive = (live.data?.is_fresh ?? false) || livePipelines.length > 0;

  // Tab is reflected in the URL (?tab=economia) so the old /prompt-economy
  // route can redirect straight into the Economia tab.
  const tabParam = searchParams.get("tab");
  const activeTab = tabParam === "economia" ? "economia" : "atividade";
  const setTab = (v: string) => {
    const next = new URLSearchParams(searchParams);
    if (v === "atividade") next.delete("tab");
    else next.set("tab", v);
    setSearchParams(next, { replace: true });
  };

  const hasError = !!(live.error || pipelines.error || tele.error);

  const mainContent = (
    <>
      <Header
        projectName={selectedProject.name}
        isLive={isLive}
        collectorHealth={collectorHealth.data ?? "off"}
        updatedAt={live.dataUpdatedAt}
        onRefresh={() => {
          void live.refetch();
          void pipelines.refetch();
          void tele.refetch();
          void quality.refetch();
        }}
        refreshing={live.isFetching || pipelines.isFetching}
      />

      {hasError ? (
        <Card size="sm">
          <CardContent>
            <p className="text-[13px] text-destructive">
              Erro ao carregar telemetria:{" "}
              {(live.error || pipelines.error || tele.error)?.toString()}
            </p>
          </CardContent>
        </Card>
      ) : (
        <Tabs value={activeTab} onValueChange={setTab} className="gap-5">
          <TabsList>
            <TabsTrigger value="atividade">Atividade</TabsTrigger>
            <TabsTrigger value="economia">Economia</TabsTrigger>
          </TabsList>
          <TabsContent value="atividade" className="flex flex-col gap-7">
            <ExecutingSection
              pipelines={livePipelines}
              totalActive={pipelines.data?.length ?? 0}
              onPipelineClick={selectPipeline}
            />
            <PhaseGridSection
              live={live.data}
              loading={live.isLoading}
              onPhaseClick={selectPhase}
            />
            <QualitySection
              quality={quality.data}
              loading={quality.isLoading}
              workflow={tele.data?.workflow.by_phase ?? []}
              qaEvents={recentEvents.data ?? []}
            />
            <AgentActivitySection block={tele.data?.agent_activity} />
            <ToolsSection tools={tele.data?.tool_breakdown ?? []} />
          </TabsContent>
          <TabsContent value="economia" className="flex flex-col gap-7">
            <EconomySection
              rtk={{
                available: tele.data?.rtk.available ?? false,
                tokens_saved: tele.data?.rtk.tokens_saved ?? 0,
                savings_pct: tele.data?.rtk.savings_pct ?? null,
                total_commands: tele.data?.rtk.total_commands ?? 0,
              }}
              hooks={tele.data?.prevention ?? []}
              routing={tele.data?.routing}
              promptEconomy={promptEconomy.data ?? null}
              sessionStartTs={tele.data?.session_start_ts ?? null}
              loading={tele.isLoading}
            />
          </TabsContent>
        </Tabs>
      )}
    </>
  );

  const panelRender = hasSelection ? (
    <DetailPanel
      pipeline={selectedPipeline}
      phase={selectedPhase}
      repoPath={path}
      projectId={activeWorkspaceId}
      onClose={closeAll}
    />
  ) : null;

  return (
    <SplitDetail open={hasSelection} panel={panelRender}>
      <div className="flex flex-col gap-7">{mainContent}</div>
    </SplitDetail>
  );
}

// ── Header ────────────────────────────────────────────────────────────────────

/**
 * Unified collector badge — identical semantics to the Prompt Economy page's
 * StatusBadge. Both consume `collector_health` so the state never diverges.
 */
function CollectorBadge({ state }: { state: CollectorHealth }) {
  const label =
    state === "live"
      ? "OTEL ativo"
      : state === "stale"
        ? "Coletor parado"
        : "OTEL não configurado";
  const dot =
    state === "live"
      ? "bg-emerald-500"
      : state === "stale"
        ? "bg-amber-500"
        : "bg-rose-500";
  return (
    <Badge variant="outline" className="ml-1 text-[10px] gap-1.5 font-normal normal-case">
      <span className={cn("inline-block h-2 w-2 rounded-full", dot)} aria-hidden />
      {label}
    </Badge>
  );
}

function Header({
  projectName,
  isLive,
  collectorHealth,
  updatedAt,
  onRefresh,
  refreshing,
}: {
  projectName: string;
  isLive: boolean;
  collectorHealth: CollectorHealth;
  updatedAt: number;
  onRefresh: () => void;
  refreshing: boolean;
}) {
  const now = useTicker(1_000);
  const ago = updatedAt ? Math.max(0, Math.round((now - updatedAt) / 1000)) : null;
  return (
    <header className="flex items-end justify-between gap-3 flex-wrap">
      <div className="flex flex-col gap-1.5">
        <nav className="text-[12px] text-muted-foreground flex items-center gap-1.5">
          Mustard <span className="opacity-50">/</span>
          <span className="text-foreground">Telemetria</span>
          <span className="opacity-50">/</span>
          <span className="font-mono">{projectName}</span>
          <Badge
            variant="outline"
            className={cn(
              "ml-1 text-[10px] uppercase tracking-wider gap-1.5",
              isLive
                ? "border-emerald-500/40 text-emerald-600 dark:text-emerald-400"
                : "text-muted-foreground",
            )}
          >
            <StatusDot variant={isLive ? "active" : "idle"} pulse={isLive} size="sm" />
            {isLive ? "live" : "idle"}
          </Badge>
          <CollectorBadge state={collectorHealth} />
        </nav>
        <h1 className="text-xl font-medium tracking-tight">Telemetria</h1>
        <p className="text-[13px] text-muted-foreground leading-relaxed">
          Aba <strong className="text-foreground/80">Atividade</strong>: o que está
          rodando agora — pipelines, fases, QA e agentes. Aba{" "}
          <strong className="text-foreground/80">Economia</strong>: quanto o Mustard
          poupa — RTK, hooks, roteamento de modelo e os blocos honestos de Prompt
          Economy. Cada bloco mostra de onde o número vem; totais acumulados ficam
          rotulados como tal.
        </p>
      </div>
      <div className="flex items-center gap-2">
        <span className="text-[11px] text-muted-foreground tabular-nums">
          {ago == null ? "—" : ago < 5 ? "agora" : `há ${ago}s`}
        </span>
        <Button
          variant="ghost"
          size="sm"
          onClick={onRefresh}
          disabled={refreshing}
          className="gap-1.5"
        >
          <RefreshCw className={cn("size-3.5", refreshing && "animate-spin")} />
          <span className="text-[12px]">Atualizar</span>
        </Button>
      </div>
    </header>
  );
}

// ── Section primitives ────────────────────────────────────────────────────────

function SectionHeader({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <h2 className="text-[11px] uppercase tracking-[0.08em] text-muted-foreground font-medium">
        {title}
      </h2>
      <p className="text-[12px] text-muted-foreground/70 leading-snug">{hint}</p>
    </div>
  );
}

function EmptyState({ title, hint }: { title: string; hint: string }) {
  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent className="flex flex-col gap-1">
        <p className="text-sm font-medium">{title}</p>
        <p className="text-[13px] text-muted-foreground">{hint}</p>
      </CardContent>
    </Card>
  );
}

// ── 1. Em execução ────────────────────────────────────────────────────────────

function ExecutingSection({
  pipelines,
  totalActive,
  onPipelineClick,
}: {
  pipelines: ActivePipeline[];
  totalActive: number;
  onPipelineClick: (p: ActivePipeline) => void;
}) {
  const hiddenCount = totalActive - pipelines.length;
  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="Em execução"
        hint="Apenas pipelines em fase EXECUTE com atividade nas últimas 24 h. Clique numa linha para abrir a spec ao lado. Polling: 3 s."
      />
      {pipelines.length === 0 ? (
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex items-center gap-2 text-[13px]">
            <StatusDot variant="idle" />
            <span className="text-muted-foreground">
              Nenhuma pipeline em execução agora.
            </span>
            {hiddenCount > 0 && (
              <span className="ml-auto text-[11px] text-muted-foreground/70">
                {hiddenCount} pipeline(s) em outras fases ocultas
              </span>
            )}
          </CardContent>
        </Card>
      ) : (
        <Card size="sm" className="ring-foreground/5">
          <CardContent>
            <ul className="flex flex-col gap-0.5 -mx-2">
              {pipelines.map((p) => (
                <LivePipelineCard
                  key={p.spec_name}
                  pipeline={p}
                  onClick={() => onPipelineClick(p)}
                />
              ))}
            </ul>
            {hiddenCount > 0 && (
              <p className="text-[11px] text-muted-foreground/70 pt-2 mt-2 border-t border-border/40">
                {hiddenCount} pipeline(s) em outras fases não exibidas aqui.
              </p>
            )}
          </CardContent>
        </Card>
      )}
    </section>
  );
}

// ── Unified detail panel ─────────────────────────────────────────────────────

function DetailPanel({
  pipeline,
  phase,
  repoPath,
  projectId,
  onClose,
}: {
  pipeline: ActivePipeline | null;
  phase: PhaseActivity | null;
  repoPath: string | null;
  projectId: string | null;
  onClose: () => void;
}) {
  return (
    <>
      <PanelHeader
        pipeline={pipeline}
        phase={phase}
        projectId={projectId}
        onClose={onClose}
      />
      <div className="flex-1 overflow-y-auto">
        {pipeline ? (
          <SpecPanelBody specName={pipeline.spec_name} projectPath={repoPath} />
        ) : phase ? (
          <PhasePanelBody phase={phase} repoPath={repoPath} />
        ) : null}
      </div>
    </>
  );
}

function PanelHeader({
  pipeline,
  phase,
  projectId,
  onClose,
}: {
  pipeline: ActivePipeline | null;
  phase: PhaseActivity | null;
  projectId: string | null;
  onClose: () => void;
}) {
  if (pipeline) {
    return (
      <header className="px-5 py-3 border-b border-border flex flex-col gap-1.5">
        <div className="flex items-center gap-2 pr-2">
          <span className="font-mono text-sm font-medium truncate flex-1" title={pipeline.spec_name}>
            {pipeline.spec_name}
          </span>
          <CloseButton onClose={onClose} />
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          {pipeline.phase && (
            <Badge variant="secondary" className="text-[11px] py-0">
              {pipeline.phase}
            </Badge>
          )}
          {pipeline.status && (
            <Badge variant="outline" className="text-[11px] py-0">
              {pipeline.status}
            </Badge>
          )}
          {pipeline.model && (
            <Badge variant="outline" className="text-[10px] py-0">
              {pipeline.model}
            </Badge>
          )}
          {projectId && pipeline.spec_name && (
            <Link
              to={`/project/${projectId}/spec/${encodeURIComponent(pipeline.spec_name)}`}
              onClick={onClose}
              className="ml-auto text-[12px] text-primary hover:underline underline-offset-2 whitespace-nowrap"
            >
              Abrir página completa →
            </Link>
          )}
        </div>
      </header>
    );
  }
  if (phase) {
    const meta = PHASE_META[phase.phase];
    return (
      <header className="px-5 py-3 border-b border-border flex flex-col gap-1">
        <div className="flex items-center gap-2 pr-2">
          {meta && (
            <span className={cn("inline-block size-2 rounded-full", meta.bg)} />
          )}
          <span className="font-mono text-sm font-semibold">{phase.phase}</span>
          {meta?.isExecution && (
            <Badge
              variant="outline"
              className="text-[10px] border-emerald-500/40 text-emerald-600 dark:text-emerald-400"
            >
              execução
            </Badge>
          )}
          <span className="text-[11px] text-muted-foreground tabular-nums ml-2">
            {phase.last_event_ts ? `última ${relativeTime(phase.last_event_ts)}` : "—"}
          </span>
          <div className="ml-auto">
            <CloseButton onClose={onClose} />
          </div>
        </div>
        {meta?.description && (
          <p className="text-[12px] text-muted-foreground leading-snug">
            {meta.description}
          </p>
        )}
      </header>
    );
  }
  return null;
}

function CloseButton({ onClose }: { onClose: () => void }) {
  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={onClose}
      className="h-7 w-7 p-0"
      title="Fechar"
    >
      <X className="size-3.5" />
    </Button>
  );
}

function SpecPanelBody({
  specName,
  projectPath,
}: {
  specName: string;
  projectPath: string | null;
}) {
  // Qual membro da família (plano-mãe ou uma wave) está em exibição. Reseta
  // quando a pipeline selecionada muda.
  const [viewing, setViewing] = useState(specName);
  useEffect(() => setViewing(specName), [specName]);

  const specs = useQuery({
    queryKey: ["specs", projectPath],
    queryFn: () => fetchSpecs(projectPath!),
    enabled: !!projectPath,
    staleTime: 15_000,
  });
  const family = resolveWaveFamily(specs.data ?? [], specName);

  const { data: markdown, isLoading, error } = useQuery({
    queryKey: ["spec-markdown", projectPath, viewing],
    queryFn: () => fetchSpecMarkdown(projectPath!, viewing),
    enabled: !!projectPath && !!viewing,
    staleTime: 60_000,
  });
  // Empty/whitespace markdown is not content — show a message, not a blank body.
  const hasContent = typeof markdown === "string" && markdown.trim().length > 0;
  return (
    <>
      {family.isWavePlan && (
        <WaveNav
          parentName={family.parentName}
          waves={family.waves}
          current={viewing}
          onSelect={setViewing}
        />
      )}
      <div className="px-5 py-4">
        {isLoading && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2, 3, 4].map((i) => (
            <div key={i} className="h-4 bg-muted/40 rounded animate-pulse" />
          ))}
        </div>
      )}
      {error && !isLoading && (
        <div className="flex flex-col gap-1.5">
          <p className="text-[13px] text-destructive">
            Não foi possível carregar o detalhe desta spec.
          </p>
          <p className="text-[12px] text-muted-foreground">{(error as Error).message}</p>
        </div>
      )}
      {!isLoading && !error && hasContent && <Markdown content={markdown!} />}
      {!isLoading && !error && !hasContent && (
        <p className="text-[13px] text-muted-foreground">
          Esta spec não tem um <code className="font-mono">spec.md</code> com conteúdo.
        </p>
      )}
      </div>
    </>
  );
}

// ── 2. Sessão agora (events.jsonl tail) ───────────────────────────────────────

// ── Phase grid — atividade real, separada por fase ───────────────────────────

const PHASE_META: Record<
  string,
  {
    label: string;
    color: string;
    bg: string;
    border: string;
    isExecution: boolean;
    description: string;
  }
> = {
  ANALYZE: {
    label: "Analisar",
    color: "text-sky-500 dark:text-sky-400",
    bg: "bg-sky-500",
    border: "border-sky-500/30",
    isExecution: false,
    description: "Exploração inicial — Grep/Read sem editar.",
  },
  PLAN: {
    label: "Planejar",
    color: "text-amber-500 dark:text-amber-400",
    bg: "bg-amber-500",
    border: "border-amber-500/30",
    isExecution: false,
    description: "Desenhar spec/plan — não toca produção.",
  },
  EXECUTE: {
    label: "Executar",
    color: "text-emerald-500 dark:text-emerald-400",
    bg: "bg-emerald-500",
    border: "border-emerald-500/30",
    isExecution: true,
    description: "Edita o código — é aqui que a wave roda de fato.",
  },
  REVIEW: {
    label: "Revisar",
    color: "text-primary",
    bg: "bg-primary",
    border: "border-primary/30",
    isExecution: false,
    description: "Inspeção do código produzido antes do QA — busca erros e regressões.",
  },
  QA: {
    label: "QA",
    color: "text-violet-500 dark:text-violet-400",
    bg: "bg-violet-500",
    border: "border-violet-500/30",
    isExecution: false,
    description: "Verificação dos critérios de aceitação.",
  },
  CLOSE: {
    label: "Fechar",
    color: "text-slate-400 dark:text-slate-300",
    bg: "bg-slate-400",
    border: "border-slate-500/30",
    isExecution: false,
    description: "Promove spec a completed e sincroniza registros.",
  },
};

function PhaseGridSection({
  live,
  loading,
  onPhaseClick,
}: {
  live: LiveActivity | undefined;
  loading: boolean;
  onPhaseClick: (p: PhaseActivity) => void;
}) {
  const phases = live?.by_phase ?? [];
  const totalToday = phases.reduce((s, p) => s + p.events_today, 0);

  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="Atividade por fase"
        hint="Cada fase é uma modalidade diferente — PLAN é planejamento (não execução), EXECUTE é a wave de fato. Clique em um card para ver os eventos. Polling: 3 s."
      />
      {loading && phases.length === 0 ? (
        <Card size="sm" className="ring-foreground/5">
          <CardContent>
            <SkeletonRows n={3} />
          </CardContent>
        </Card>
      ) : (
        <>
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-2">
            {phases.map((p) => (
              <PhaseCard
                key={p.phase}
                data={p}
                onClick={() => onPhaseClick(p)}
              />
            ))}
          </div>
          {totalToday === 0 && (
            <p className="text-[12px] text-muted-foreground/80">
              Nenhum evento registrado hoje em qualquer fase. Os hooks gravam em{" "}
              <code className="font-mono">.claude/.harness/events.jsonl</code> a
              cada uso de ferramenta.
            </p>
          )}
        </>
      )}
    </section>
  );
}

function PhaseCard({
  data,
  onClick,
}: {
  data: PhaseActivity;
  onClick: () => void;
}) {
  const meta = PHASE_META[data.phase] ?? {
    label: data.phase,
    color: "text-foreground",
    bg: "bg-slate-400",
    border: "border-border",
    isExecution: false,
    description: "",
  };
  const hasEvents = data.events_today > 0;
  const isLive5min = data.events_last_5min > 0;
  return (
    <Card
      size="sm"
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
      className={cn(
        "ring-foreground/5 relative cursor-pointer transition-colors hover:bg-muted/30 focus-visible:ring-2 focus-visible:ring-primary/50 focus:outline-none",
        hasEvents ? "opacity-100" : "opacity-65",
      )}
    >
      <CardContent className="flex flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-1.5">
            <span className={cn("inline-block size-1.5 rounded-full", meta.bg)} />
            <span className="text-[10px] uppercase tracking-[0.08em] font-medium font-mono">
              {data.phase}
            </span>
            {meta.isExecution && (
              <Badge
                variant="outline"
                className="text-[9px] px-1.5 py-0 h-4 border-emerald-500/40 text-emerald-600 dark:text-emerald-400"
              >
                execução
              </Badge>
            )}
          </div>
          {isLive5min && (
            <StatusDot variant="active" pulse size="sm" className={meta.bg} />
          )}
        </div>
        <div className={cn("text-3xl font-mono font-medium tabular-nums leading-none", meta.color)}>
          {formatNumber(data.events_today)}
        </div>
        <div className="text-[10px] uppercase tracking-wider text-muted-foreground -mt-1">
          eventos hoje
        </div>
        <PhaseSparkline buckets={data.minute_buckets} meta={meta} />
        <div className="flex items-baseline justify-between text-[11px] text-muted-foreground">
          <span>
            <span className="font-mono tabular-nums">{data.events_last_5min}</span>
            <span className="text-muted-foreground/60"> /5min</span>
          </span>
          <span>
            <span className="font-mono tabular-nums">{data.events_last_hour}</span>
            <span className="text-muted-foreground/60"> /1h</span>
          </span>
        </div>
        <p
          className="text-[11px] text-muted-foreground/75 leading-snug border-t border-border/40 pt-1.5"
          title={meta.description}
        >
          {meta.description || "—"}
        </p>
        {data.top_tools.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {data.top_tools.map((t) => (
              <span
                key={t.tool_name}
                className="font-mono text-[10px] text-muted-foreground px-1.5 py-0.5 rounded bg-muted/50"
                title={`${t.count} chamadas em ${data.phase}`}
              >
                {t.tool_name} {formatNumber(t.count)}
              </span>
            ))}
          </div>
        )}
        {data.last_event_ts && (
          <div className="text-[10.5px] text-muted-foreground/60">
            última {relativeTime(data.last_event_ts)}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function PhaseSparkline({
  buckets,
  meta,
}: {
  buckets: number[];
  meta: (typeof PHASE_META)[string];
}) {
  const max = Math.max(1, ...buckets);
  const total = buckets.reduce((s, n) => s + n, 0);
  if (total === 0) {
    return (
      <div className="h-7 flex items-center">
        <div className="h-px w-full bg-muted/60" />
      </div>
    );
  }
  const W = 140;
  const H = 28;
  const barW = W / buckets.length;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="w-full h-7" preserveAspectRatio="none">
      {buckets.map((v, i) => {
        const h = (v / max) * (H - 2);
        const recent = i >= buckets.length - 5;
        return (
          <rect
            key={i}
            x={i * barW + 0.5}
            y={H - h}
            width={Math.max(barW - 0.6, 0.6)}
            height={Math.max(h, v > 0 ? 1 : 0)}
            className={cn(meta.bg, "fill-current", !recent && "opacity-50")}
            rx={0.5}
          />
        );
      })}
    </svg>
  );
}

// ── 3. Economia (RTK + hooks + routing) ───────────────────────────────────────

/**
 * Friendly label + tooltip + visual tone for each model-routing-gate `note`.
 * Tooltips explain in plain language what the orchestrator tried to do and
 * what the gate did about it — so the user can read the breakdown without
 * knowing the hook's internals.
 */
const NOTE_META: Record<string, { label: string; tip: string; tone: string; dot: string }> = {
  violation: {
    label: "Bloqueou upgrade explícito",
    tip: "O orquestrador pediu um modelo MAIS CARO do que a tabela permite (ex: pediu opus pra um Explore que devia ser haiku). Gate negou. Esses são os blocks mais valiosos — economia direta.",
    tone: "text-rose-500 dark:text-rose-400",
    dot: "bg-rose-500",
  },
  "no-model-denied": {
    label: "Explore sem model explícito",
    tip: "Agente Explore foi despachado sem campo model:, ia herdar opus do parent (caro). Gate negou e exigiu model: \"haiku\" no Task dispatch. Específico pra Explore.",
    tone: "text-rose-500 dark:text-rose-400",
    dot: "bg-rose-500",
  },
  "no-model-denied-sonnet": {
    label: "Não-Explore sem model → exigiu sonnet",
    tip: "Outro agente (review/audit/etc) foi despachado sem model: e o esperado era sonnet. Gate negou pra evitar herdar opus do parent. Mustard 2.5 — minha regra nova.",
    tone: "text-amber-500 dark:text-amber-400",
    dot: "bg-amber-500",
  },
  "no-model-advisory": {
    label: "Avisou (modo warn)",
    tip: "Mesma situação acima, mas com MUSTARD_MODEL_GATE_MODE=warn. Gate só avisou no console em vez de bloquear. Use warn quando quer telemetria sem fricção.",
    tone: "text-amber-500 dark:text-amber-400",
    dot: "bg-amber-400",
  },
  passed: {
    label: "Passou (modelo correto)",
    tip: "Orquestrador escolheu o modelo certo de primeira (ou um downgrade aceitável). Gate só registra e deixa passar.",
    tone: "text-emerald-500 dark:text-emerald-400",
    dot: "bg-emerald-500",
  },
  blocked: {
    label: "Bloqueio legado",
    tip: "Categoria histórica do gate antes do refactor das notas. Conta junto com 'violation' pra compatibilidade.",
    tone: "text-rose-500 dark:text-rose-400",
    dot: "bg-rose-500",
  },
};

// ── Prompt Economy honest blocks (absorbed from the former /prompt-economy
//    page — single source of truth, no divergence) ────────────────────────────

/** bytes → human-readable. Local to this page; only the economy blocks need it. */
function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  if (n < 1024) return `${Math.round(n)} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatActiveTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "0s";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}m ${s}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

/** USD measured by the Anthropic API via native OTEL — lifetime total. */
function CostBlock({ cost }: { cost: import("@/api/promptEconomy").PromptEconomy["cost"] }) {
  const topModels = cost.by_model.slice(0, 4);
  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent className="flex flex-col gap-2">
        <SubHeader
          label="Cache da API · USD medido"
          detail="O que a Anthropic API de fato cobrou, lido do stream OTEL nativo do Claude Code. Já inclui o desconto de cache — é o número real, não estimativa. Acumulado vitalício."
          accent="indigo"
        />
        <div className="flex items-baseline gap-2 mt-1">
          <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
            USD total
          </span>
          <span className="text-2xl font-mono tabular-nums text-primary leading-none">
            {formatUsd(cost.usd_total)}
          </span>
        </div>
        {topModels.length > 0 ? (
          <ul className="flex flex-col gap-1 mt-1 border-t border-border/40 pt-2">
            <li className="text-[10px] uppercase tracking-wider text-muted-foreground/70">
              por modelo
            </li>
            {topModels.map((m) => (
              <li
                key={m.model}
                className="flex items-baseline justify-between gap-3 text-[12.5px]"
              >
                <span className="font-mono text-muted-foreground truncate">{m.model}</span>
                <span className="font-mono tabular-nums text-foreground shrink-0">
                  {formatUsd(m.usd)}
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[11.5px] text-muted-foreground italic">
            Sem dados por modelo ainda.
          </p>
        )}
      </CardContent>
    </Card>
  );
}

/** Context Mustard sent to sub-agents vs. the spec it avoided sending. */
function SubtractionsBlock({
  subtractions,
}: {
  subtractions: import("@/api/promptEconomy").PromptEconomy["subtractions"];
}) {
  const waves = subtractions.by_wave;
  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent className="flex flex-col gap-2">
        <SubHeader
          label="Contexto enviado vs. evitado"
          detail="Cada sub-agente recebe só a fatia do spec da sua wave. 'Enviado' é o que de fato foi no prompt despachado; 'evitado' é o resto do spec que ele não precisou ver."
          accent="indigo"
        />
        <div className="grid grid-cols-2 gap-4 mt-1">
          <div className="flex flex-col gap-0.5">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Contexto enviado
            </span>
            <span className="text-2xl font-mono tabular-nums text-foreground leading-none">
              {formatBytes(subtractions.context_sent_bytes)}
            </span>
            {subtractions.session_known && subtractions.session_sent_bytes > 0 && (
              <span
                className="text-[11px] text-emerald-500 dark:text-emerald-400"
                title="Quanto deste acumulado veio da sessão atual (desde que o Claude Code abriu este projeto)."
              >
                +{formatBytes(subtractions.session_sent_bytes)} nesta sessão
              </span>
            )}
          </div>
          <div className="flex flex-col gap-0.5">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Contexto evitado
            </span>
            <span className="text-2xl font-mono tabular-nums text-violet-400 leading-none">
              {formatBytes(subtractions.context_avoided_bytes)}
            </span>
            {subtractions.session_known && subtractions.session_avoided_bytes > 0 && (
              <span
                className="text-[11px] text-emerald-500 dark:text-emerald-400"
                title="Quanto deste acumulado veio da sessão atual (desde que o Claude Code abriu este projeto)."
              >
                +{formatBytes(subtractions.session_avoided_bytes)} nesta sessão
              </span>
            )}
          </div>
        </div>
        {waves.length === 0 ? (
          <p className="text-[11.5px] text-muted-foreground leading-snug border-t border-border/40 pt-2">
            Sem dados ainda. Aparece quando uma pipeline roda a fase EXECUTE com waves.
          </p>
        ) : (
          <ul className="flex flex-col gap-1 mt-1 border-t border-border/40 pt-2">
            <li className="text-[10px] uppercase tracking-wider text-muted-foreground/70">
              por wave
            </li>
            {waves.map((w) => (
              <li
                key={w.wave}
                className="flex items-baseline justify-between gap-3 text-[12.5px]"
              >
                <span className="font-mono text-muted-foreground">Wave {w.wave}</span>
                <span className="flex items-baseline gap-2 shrink-0">
                  <span className="font-mono tabular-nums text-foreground">
                    enviado {formatBytes(w.sent_bytes)}
                  </span>
                  <span className="font-mono tabular-nums text-violet-400">
                    · evitado {formatBytes(w.avoided_bytes)}
                  </span>
                  <span className="text-[11px] text-muted-foreground tabular-nums">
                    ({w.count} {w.count === 1 ? "evento" : "eventos"})
                  </span>
                </span>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

/** Operational counters from Claude Code — sessions + active time. */
function ClaudeEventsBlock({
  events,
}: {
  events: import("@/api/promptEconomy").PromptEconomy["claude_events"];
}) {
  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent className="flex flex-col gap-2">
        <SubHeader
          label="Eventos Claude Code"
          detail="Telemetria operacional vinda do mesmo stream OTEL: quantas sessões começaram e quanto tempo de uso ativo somaram. Mede uso, não dinheiro. Acumulado vitalício."
          accent="indigo"
        />
        <div className="grid grid-cols-2 gap-4 mt-1">
          <div className="flex flex-col gap-1">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Sessions
            </span>
            <span className="text-2xl font-mono tabular-nums text-foreground leading-none">
              {events.session_count.toLocaleString()}
            </span>
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Tempo ativo
            </span>
            <span className="text-2xl font-mono tabular-nums text-foreground leading-none">
              {formatActiveTime(events.active_time_seconds)}
            </span>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function CanaryTail({ lines }: { lines: string[] }) {
  return (
    <Card size="sm" className="ring-rose-500/20 border-rose-500/30">
      <CardContent className="flex flex-col gap-2">
        <div className="flex items-baseline gap-2">
          <span className="text-[11px] uppercase tracking-[0.08em] font-medium text-rose-400">
            Canary tail
          </span>
          <span className="text-[11px] text-muted-foreground">
            últimas {lines.length} linhas — só aparece quando o coletor OTEL está parado
          </span>
        </div>
        <pre className="font-mono text-[11px] leading-relaxed text-muted-foreground/80 whitespace-pre-wrap break-all bg-background/40 rounded-md p-3 max-h-64 overflow-auto">
          {lines.join("\n")}
        </pre>
      </CardContent>
    </Card>
  );
}

/**
 * Prompt Economy section — the three honest blocks, formerly its own page.
 * Now a sub-section of the Economia tab so there is one source of truth and
 * no chance of the old "901KB vs 1.7MB" divergence between two pages.
 */
function PromptEconomySection({
  data,
  loading,
}: {
  data: import("@/api/promptEconomy").PromptEconomy | null;
  loading: boolean;
}) {
  const allZero =
    !!data &&
    data.cost.usd_total === 0 &&
    data.subtractions.context_sent_bytes === 0 &&
    data.subtractions.context_avoided_bytes === 0 &&
    data.claude_events.session_count === 0 &&
    data.claude_events.active_time_seconds === 0;

  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="Prompt Economy — três blocos honestos"
        hint="USD medido pela Anthropic API, bytes que o Mustard escolheu não enviar e telemetria operacional do Claude Code. Cada bloco vem de uma fonte distinta — nada de número inventado."
      />
      {loading && !data ? (
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
          {[0, 1, 2].map((i) => (
            <Card size="sm" key={i} className="ring-foreground/5">
              <CardContent>
                <SkeletonRows n={4} />
              </CardContent>
            </Card>
          ))}
        </div>
      ) : !data || allZero ? (
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-1">
            <p className="text-sm font-medium">Sem atividade ainda</p>
            <p className="text-[12.5px] text-muted-foreground leading-relaxed">
              Rode <code className="font-mono">/mustard:feature</code> ou{" "}
              <code className="font-mono">/mustard:bugfix</code> neste projeto para
              começar a alimentar estes blocos. Se a telemetria OTEL não estiver
              ligada, o badge no topo fica vermelho — confira em Settings.
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
          <CostBlock cost={data.cost} />
          <SubtractionsBlock subtractions={data.subtractions} />
          <ClaudeEventsBlock events={data.claude_events} />
        </div>
      )}
      {data && data.freshness.canary_tail && data.freshness.canary_tail.length > 0 && !data.freshness.otel_healthy && (
        <CanaryTail lines={data.freshness.canary_tail} />
      )}
    </section>
  );
}

function EconomySection({
  rtk,
  hooks,
  routing,
  promptEconomy,
  sessionStartTs,
  loading,
}: {
  rtk: {
    available: boolean;
    tokens_saved: number;
    savings_pct: number | null;
    total_commands: number;
  };
  hooks: HookFireCount[];
  routing: RoutingBlock | undefined;
  promptEconomy: import("@/api/promptEconomy").PromptEconomy | null;
  sessionStartTs: string | null;
  loading: boolean;
}) {
  const topHooks = useMemo(
    () =>
      [...hooks]
        .filter((h) => h.tokens_saved > 0)
        .sort((a, b) => b.tokens_saved - a.tokens_saved)
        .slice(0, 5),
    [hooks],
  );
  const hookTotal = useMemo(() => hooks.reduce((s, h) => s + h.tokens_saved, 0), [hooks]);
  const hookSessionTotal = useMemo(
    () => hooks.reduce((s, h) => s + h.session_tokens_saved, 0),
    [hooks],
  );
  const hookMax = topHooks[0]?.tokens_saved ?? 1;

  const blocks = routing?.blocks ?? 0;
  const allows = routing?.allows ?? 0;
  const sessionBlocks = routing?.session_blocks ?? 0;
  const sessionAllows = routing?.session_allows ?? 0;
  const preventionPct = (blocks / Math.max(blocks + allows, 1)) * 100;

  // "+N nesta sessão" suffix — only shown when the session window produced
  // something, so a fresh session doesn't render a noisy "+0".
  const sessionTag = (n: number) =>
    n > 0 ? (
      <span className="text-emerald-500 dark:text-emerald-400" title="Quanto deste total veio da sessão atual (desde que o Claude Code abriu este projeto).">
        {" "}· +{formatNumber(n)} nesta sessão
      </span>
    ) : null;

  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="Economia de tokens"
        hint="Três mecanismos para reduzir o que vai ao modelo. Os totais são ACUMULADOS desde a instalação; o trecho verde '+N nesta sessão' isola o que aconteceu na sessão atual."
      />
      {sessionStartTs == null && (
        <p className="text-[11px] text-muted-foreground/70 leading-snug">
          Nenhuma sessão detectada em <code className="font-mono">events.jsonl</code> —
          os deltas de sessão não aparecem até um pipeline rodar.
        </p>
      )}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-2">
            <SubHeader
              label="RTK · comandos"
              detail="Comprime output de comandos shell (ex.: pnpm install, git log). Calculado pelo binário rtk; mostra o ganho global do RTK, não apenas deste projeto."
              accent="emerald"
            />
            {!rtk.available ? (
              <p className="text-[12px] text-muted-foreground">
                RTK não instalado. Rode <code className="font-mono">rtk init -g</code>.
              </p>
            ) : (
              <div className="grid grid-cols-2 gap-2 mt-1">
                <BigStat
                  label="Tokens salvos"
                  value={formatTokens(rtk.tokens_saved)}
                  accent="emerald"
                />
                <BigStat
                  label="Taxa"
                  value={rtk.savings_pct != null ? formatPct(rtk.savings_pct) : "—"}
                  accent="emerald"
                />
                <div className="col-span-2 text-[11px] text-muted-foreground">
                  {formatNumber(rtk.total_commands)} comandos comprimidos
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-2">
            <SubHeader
              label="Hooks · interceptação"
              detail="Hooks que reescrevem entradas/saídas de ferramentas para evitar gastar tokens. Ex.: auto-format envia diff em vez do arquivo todo; tool-use-counter corta exploração antes que ela inche o contexto."
              accent="emerald"
            />
            <div className="flex flex-col gap-0.5">
              <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Total
              </span>
              <span className="text-xl font-mono font-medium tabular-nums leading-none text-emerald-500 dark:text-emerald-400">
                {formatTokens(hookTotal)}
              </span>
              <span className="text-[11px] text-muted-foreground">
                acumulado{sessionTag(hookSessionTotal)}
              </span>
            </div>
            {loading ? (
              <SkeletonRows n={3} />
            ) : topHooks.length === 0 ? (
              <p className="text-[12px] text-muted-foreground">
                Nenhum hook contou tokens salvos ainda.
              </p>
            ) : (
              <ul className="flex flex-col gap-1 mt-1">
                {topHooks.map((h) => (
                  <li key={h.hook} className="flex items-baseline gap-2 text-[12px]">
                    <span className="font-mono truncate flex-1" title={h.hook}>
                      {h.hook}
                    </span>
                    <div className="h-1 w-12 bg-muted rounded overflow-hidden">
                      <div
                        className="h-full bg-emerald-500/60"
                        style={{ width: `${(h.tokens_saved / hookMax) * 100}%` }}
                      />
                    </div>
                    <span className="font-mono tabular-nums text-muted-foreground w-14 text-right">
                      {formatTokens(h.tokens_saved)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-2">
            <SubHeader
              label="Roteamento de modelo"
              detail="Cada vez que o orquestrador dispara um Task, o gate compara o modelo pedido com o esperado pra aquele agente. Se for mais caro que o necessário, bloqueia (ou avisa). Os blocks aqui = tokens que NÃO foram gastos com modelo errado."
              accent="amber"
            />
            <div className="text-[11px] text-muted-foreground/80 bg-muted/30 border border-border/40 rounded px-2 py-1.5 leading-relaxed">
              <p className="text-muted-foreground">Exemplo do que esses números querem dizer:</p>
              <p className="mt-0.5">
                "<span className="font-mono">{formatNumber(blocks + allows)}</span>" dispatches o orquestrador tentou
                disparar; em "<span className="font-mono text-rose-400">{formatNumber(blocks)}</span>" o gate
                negou e exigiu modelo mais barato (=&nbsp;
                <span className="text-emerald-500">economia</span>); em
                "<span className="font-mono text-emerald-400">{formatNumber(allows)}</span>" o modelo
                estava certo de cara. <span className="font-mono text-amber-400">{formatPct(preventionPct)}</span>{" "}
                de "intervenção" é a razão bloqueados/total.
              </p>
            </div>
            <div className="grid grid-cols-2 gap-2 mt-1">
              <div title="% de dispatches em que o gate teve que intervir (negar ou avisar). MAIS alto = orquestrador errando muito de início, gate salvando. MAIS baixo = orquestrador já acerta de primeira, tabela está calibrada.">
                <BigStat label="Intervenção" value={formatPct(preventionPct)} accent="amber" />
              </div>
              <div title="Quantos dispatches o gate negou. Cada um é dinheiro que não saiu — gate forçou o orquestrador a refazer com modelo mais barato.">
                <BigStat label="Bloqueados" value={formatNumber(blocks)} />
              </div>
            </div>
            <p className="text-[11px] text-muted-foreground">
              <span title="Quantos dispatches o gate negou (precisaram ser refeitos com modelo correto)">
                {formatNumber(blocks)} bloqueados
              </span>{" "}
              ·{" "}
              <span title="Quantos dispatches passaram direto (modelo já estava correto ou downgrade aceitável)">
                {formatNumber(allows)} liberados
              </span>
              <span className="text-muted-foreground/70"> · acumulado</span>
              {sessionTag(sessionBlocks + sessionAllows)}
            </p>
            {routing && routing.by_intent.length > 0 && (
              <div className="flex flex-col gap-1 mt-1 border-t border-border/40 pt-2">
                <span
                  className="text-[10px] uppercase tracking-wider text-muted-foreground/70"
                  title="Quais tipos de agente o gate mais bloqueia. A barra mostra a proporção bloqueado/liberado."
                >
                  por tipo de agente
                </span>
                <ul className="flex flex-col gap-1">
                  {routing.by_intent.slice(0, 3).map((r) => {
                    const total = r.blocks + r.allows || 1;
                    const pct = (r.blocks / total) * 100;
                    return (
                      <li
                        key={r.intent}
                        className="flex items-baseline gap-2 text-[12px]"
                        title={`${r.intent || "outros"}: ${r.blocks} bloqueados de ${r.blocks + r.allows} dispatches (${pct.toFixed(0)}% intervenção)`}
                      >
                        <span className="truncate flex-1 font-mono">
                          {r.intent || "outros"}
                        </span>
                        <div className="flex h-1 w-16 rounded overflow-hidden">
                          <div className="bg-rose-500/50" style={{ width: `${pct}%` }} />
                          <div className="bg-emerald-500/40 flex-1" />
                        </div>
                        <span className="font-mono tabular-nums text-muted-foreground/70 w-12 text-right">
                          {r.blocks}/{r.allows}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              </div>
            )}
            {routing && routing.by_note.length > 0 && (
              <div className="flex flex-col gap-1 mt-1 border-t border-border/40 pt-2">
                <span
                  className="text-[10px] uppercase tracking-wider text-muted-foreground/70"
                  title="Como o gate reagiu. Cores: vermelho = bloqueio explícito (economia direta); âmbar = aviso ou exigência de explicitar modelo; verde = passou direto."
                >
                  por categoria de ação
                </span>
                <ul className="flex flex-col gap-0.5">
                  {routing.by_note.map((n) => {
                    const meta = NOTE_META[n.note] ?? {
                      label: n.note,
                      tip: `Categoria desconhecida: ${n.note}. Pode ser uma nota nova do gate que o dashboard ainda não rotulou.`,
                      tone: "text-muted-foreground",
                      dot: "bg-muted",
                    };
                    return (
                      <li
                        key={n.note}
                        className="flex items-baseline gap-2 text-[11.5px]"
                        title={meta.tip}
                      >
                        <span className={cn("inline-block size-1.5 rounded-full", meta.dot)} />
                        <span className={cn("flex-1 truncate", meta.tone)}>{meta.label}</span>
                        <span className="font-mono tabular-nums text-muted-foreground/80 w-10 text-right">
                          {formatNumber(n.count)}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              </div>
            )}
          </CardContent>
        </Card>

      </div>
      <PromptEconomySection data={promptEconomy} loading={loading} />
    </section>
  );
}

function SubHeader({
  label,
  detail,
  accent,
}: {
  label: string;
  detail: string;
  accent?: "emerald" | "amber" | "indigo";
}) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <span
          className={cn(
            "inline-block size-1.5 rounded-full",
            accent === "emerald"
              ? "bg-emerald-500"
              : accent === "amber"
                ? "bg-amber-500"
                : accent === "indigo"
                  ? "bg-primary"
                  : "bg-foreground",
          )}
        />
        <span className="text-[12px] font-medium">{label}</span>
      </div>
      <p className="text-[11px] text-muted-foreground/80 leading-snug">{detail}</p>
    </div>
  );
}

function BigStat({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: "emerald" | "amber" | "indigo";
}) {
  const accentClass =
    accent === "emerald"
      ? "text-emerald-500 dark:text-emerald-400"
      : accent === "amber"
        ? "text-amber-500 dark:text-amber-400"
        : accent === "indigo"
          ? "text-primary"
          : "";
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[10px] uppercase tracking-wider text-muted-foreground">{label}</span>
      <span
        className={cn(
          "text-xl font-mono font-medium tabular-nums leading-none",
          accentClass,
        )}
      >
        {value}
      </span>
    </div>
  );
}

// ── 4. Qualidade & Workflow ───────────────────────────────────────────────────

function QualitySection({
  quality,
  loading,
  workflow,
  qaEvents,
}: {
  quality: QualityMetrics | undefined;
  loading: boolean;
  workflow: PhaseCount[];
  qaEvents: RecentEvent[];
}) {
  const phaseCounts = PHASES.map((ph) => workflow.find((p) => p.phase === ph)?.count ?? 0);
  const phaseTotal = phaseCounts.reduce((s, n) => s + n, 0);
  const phaseMax = Math.max(1, ...phaseCounts);

  const hasQuality =
    !!quality &&
    (quality.pass_at_1 > 0 ||
      quality.fix_loop_rate > 0 ||
      quality.avg_phase_duration_ms > 0);

  // QA results derived from qa.result events. Distinct from quality.pass_at_1
  // which is "first-attempt" pass rate computed from the SQLite projection.
  // Here we count raw verdicts emitted by qa-run.js — useful to see how AC
  // are actually behaving right now.
  const qaCounts = useMemo(() => {
    const c = { pass: 0, fail: 0, skip: 0 };
    let lastTs: string | null = null;
    let lastOverall: "pass" | "fail" | "skip" | null = null;
    let lastSpec: string | null = null;
    for (const e of qaEvents) {
      if (e.event_type !== "qa.result") continue;
      const o = parseQaOverall(e.summary);
      if (!o) continue;
      c[o]++;
      // qaEvents come newest-first from fetchRecentEvents
      if (!lastTs) {
        lastTs = e.ts;
        lastOverall = o;
        lastSpec = e.spec ?? null;
      }
    }
    return { ...c, lastTs, lastOverall, lastSpec };
  }, [qaEvents]);

  const qaTotal = qaCounts.pass + qaCounts.fail + qaCounts.skip;
  const qaPassRate = qaTotal > 0 ? qaCounts.pass / qaTotal : 0;

  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="Como o projeto está indo"
        hint="Três visões: (1) histórico do projeto — quão limpas são as pipelines; (2) AC reais — quantos critérios passaram nos últimos QAs; (3) atividade por fase — onde o esforço foi gasto."
      />
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-2">
            <SubHeader
              label="Histórico do projeto"
              detail="Progresso e ritmo das pipelines deste projeto: quantas specs já fecharam e quanto tempo cada fase costuma levar. Para acerto de QA, veja o card ao lado."
            />
            {loading ? (
              <SkeletonRows n={3} />
            ) : !hasQuality ? (
              <p className="text-[12px] text-muted-foreground">
                Ainda não rodou QA suficiente neste projeto. Rode uma pipeline com{" "}
                <code className="font-mono">## Acceptance Criteria</code> pra começar a alimentar este card.
              </p>
            ) : (
              <div className="grid grid-cols-2 gap-2 mt-1">
                <div
                  title="Quantas specs deste projeto já foram concluídas, sobre o total de specs. NÃO é pass@1 de QA — para acerto real de QA veja o card 'Critérios de aceitação' ao lado."
                >
                  <BigStat
                    label="Specs concluídas"
                    value={formatPct(quality!.pass_at_1 * 100)}
                    accent={quality!.pass_at_1 >= 0.7 ? "emerald" : "amber"}
                  />
                </div>
                <div
                  title="% de waves que precisaram de pelo menos 1 fix-loop. Quanto MENOR melhor — alto significa que código sai com bug e precisa correção depois."
                >
                  <BigStat
                    label="Precisou refazer"
                    value={formatPct(quality!.fix_loop_rate * 100)}
                    accent={quality!.fix_loop_rate < 0.2 ? "emerald" : "amber"}
                  />
                </div>
                <div title="Tempo médio que cada fase (ANALYZE/PLAN/EXECUTE/QA) leva. Útil pra detectar regressão de performance entre pipelines.">
                  <BigStat
                    label="Tempo médio / fase"
                    value={
                      quality!.avg_phase_duration_ms
                        ? formatDurationMs(quality!.avg_phase_duration_ms)
                        : "—"
                    }
                  />
                </div>
                <div title="Quantas waves ficaram acima da média de duração (top-5 lentas). Quanto MAIS, mais alvos pra investigar.">
                  <BigStat label="Waves lentas" value={String(quality!.slowest_waves?.length ?? 0)} />
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-2">
            <SubHeader
              label="Critérios de aceitação (últimos QAs)"
              detail="Cada QA roda os comandos definidos em ## Acceptance Criteria do spec e marca cada AC como passou (✓), falhou (✗) ou foi pulado (⊘). Este card mostra a contagem crua dos últimos 200 eventos."
              accent="emerald"
            />
            {qaTotal === 0 ? (
              <p className="text-[12px] text-muted-foreground">
                Nenhum QA rodou ainda neste projeto. Aparece aqui quando você completar uma pipeline que tenha AC na spec.
              </p>
            ) : (
              <>
                <div className="grid grid-cols-2 gap-2 mt-1">
                  <div title="% dos QAs que terminaram com TODOS os AC passando (overall=pass). Quanto MAIOR, mais confiável o código que está saindo.">
                    <BigStat
                      label="Taxa de aprovação"
                      value={formatPct(qaPassRate * 100)}
                      accent={qaPassRate >= 0.7 ? "emerald" : "amber"}
                    />
                  </div>
                  <div title="Quantos QAs foram executados na janela observada. Pipelines sem AC na spec NÃO contam aqui — só aparece quando há critério rodável.">
                    <BigStat label="QAs rodados" value={formatNumber(qaTotal)} />
                  </div>
                </div>
                <div className="flex items-baseline gap-3 text-[11.5px] mt-1">
                  <span
                    className="text-emerald-500 font-mono tabular-nums"
                    title="QAs onde todos os AC passaram (overall=pass)"
                  >
                    ✓ {qaCounts.pass} passou
                  </span>
                  {qaCounts.fail > 0 && (
                    <span
                      className="text-rose-400 font-mono tabular-nums"
                      title="QAs com pelo menos 1 AC falho (overall=fail). Pipeline NÃO consegue fechar até resolver."
                    >
                      ✗ {qaCounts.fail} falhou
                    </span>
                  )}
                  {qaCounts.skip > 0 && (
                    <span
                      className="text-amber-400 font-mono tabular-nums"
                      title="QAs sem AC executável (overall=skip) — spec não definiu critérios. Pipeline fecha com warning."
                    >
                      ⊘ {qaCounts.skip} pulou
                    </span>
                  )}
                </div>
                {qaCounts.lastTs && qaCounts.lastOverall && (
                  <div className="text-[11px] text-muted-foreground/80 mt-1 border-t border-border/40 pt-1.5">
                    Último QA:{" "}
                    <span
                      className={cn(
                        "font-mono",
                        qaCounts.lastOverall === "pass" && "text-emerald-500",
                        qaCounts.lastOverall === "fail" && "text-rose-400",
                        qaCounts.lastOverall === "skip" && "text-amber-400",
                      )}
                    >
                      {qaCounts.lastOverall === "pass" ? "passou" : qaCounts.lastOverall === "fail" ? "falhou" : "pulou"}
                    </span>{" "}
                    {qaCounts.lastSpec && (
                      <span className="font-mono text-muted-foreground/60">
                        · {qaCounts.lastSpec.replace(/^\d{4}-\d{2}-\d{2}-/, "")}
                      </span>
                    )}{" "}
                    {relativeTime(qaCounts.lastTs)}
                  </div>
                )}
              </>
            )}
          </CardContent>
        </Card>

        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-2">
            <SubHeader
              label="Onde o esforço acontece"
              detail="Cada barra mostra quantos eventos (uso de ferramenta + transições de fase) aconteceram em cada uma das 5 fases do pipeline. Útil pra ver se o projeto gasta tempo demais em ANALYZE/PLAN ou se executa rápido. NÃO mede pass/fail — pra isso veja o card do meio."
            />
            {phaseTotal === 0 ? (
              <p className="text-[12px] text-muted-foreground">
                Nenhum evento por fase ainda. Os hooks registram em{" "}
                <code className="font-mono">.harness/events.jsonl</code> a cada uso de ferramenta dentro de uma pipeline.
              </p>
            ) : (
              <div className="flex flex-col gap-1 mt-1">
                {PHASES.map((ph, i) => {
                  const phaseDesc = PHASE_TIP[ph];
                  return (
                    <div
                      key={ph}
                      className="flex items-baseline gap-2 text-[12.5px]"
                      title={phaseDesc}
                    >
                      <span className="font-mono w-16 text-muted-foreground">{ph}</span>
                      <div className="flex-1 h-1.5 bg-muted rounded overflow-hidden">
                        <div
                          className="h-full bg-amber-500/50"
                          style={{ width: `${(phaseCounts[i] / phaseMax) * 100}%` }}
                        />
                      </div>
                      <span className="w-8 text-right font-mono tabular-nums text-muted-foreground">
                        {phaseCounts[i]}
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </section>
  );
}

/** Friendly explanation of each pipeline phase for hover tooltips. */
const PHASE_TIP: Record<string, string> = {
  ANALYZE: "Exploração inicial: Grep/Read sem editar. Onde o agente entende o problema antes de planejar.",
  PLAN: "Desenho da spec/plan. Não toca código de produção; só decide o quê e onde mexer.",
  EXECUTE: "Onde o código é editado de fato. Cada wave (impl/backend/frontend) roda aqui.",
  REVIEW: "Inspeção do código produzido — correção, convenções e regressões — antes do QA.",
  QA: "Verificação dos critérios de aceitação (## Acceptance Criteria). Bloqueia CLOSE se algum AC falhar.",
  CLOSE: "Promove a spec a completed, sincroniza registry e fecha o pipeline.",
};

// ── Agent activity (span-lite — events.jsonl agent.start/stop pairs) ─────────

function AgentActivitySection({ block }: { block: AgentActivityBlock | undefined }) {
  if (!block || block.total_dispatches === 0) {
    return (
      <section className="flex flex-col gap-2">
        <SectionHeader
          title="Agentes despachados"
          hint="Pares agent.start/agent.stop do events.jsonl. Surface da Phase 2 (sem tokens — esses vivem na tabela `spans` SQLite que os hooks ainda não escrevem)."
        />
        <Card size="sm" className="ring-foreground/5">
          <CardContent>
            <p className="text-[12px] text-muted-foreground">
              Nenhum agent.start registrado ainda em <code className="font-mono">.harness/events.jsonl</code>.
            </p>
          </CardContent>
        </Card>
      </section>
    );
  }
  const maxStarts = Math.max(1, ...block.agents.map((a) => a.starts));
  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="Agentes despachados"
        hint="Top 10 agent_types nesta sessão. Erros = agent.stop com isError=true. Duração derivada de start→stop pareados por (sessionId, actor.id)."
      />
      <Card size="sm" className="ring-foreground/5">
        <CardContent className="flex flex-col gap-2">
          <div className="flex items-baseline gap-4">
            <BigStat label="Total dispatches" value={formatNumber(block.total_dispatches)} accent="indigo" />
            <BigStat
              label="Erros"
              value={formatNumber(block.total_errors)}
              accent={block.total_errors === 0 ? "emerald" : "amber"}
            />
          </div>
          <ul className="flex flex-col gap-1 mt-1">
            {block.agents.map((a) => {
              const errPct = a.starts > 0 ? (a.errors / a.starts) * 100 : 0;
              return (
                <li key={a.agent_type} className="flex items-baseline gap-2 text-[12.5px]">
                  <span className="font-mono w-32 truncate" title={a.agent_type}>
                    {a.agent_type}
                  </span>
                  <div className="flex-1 h-1.5 bg-muted rounded overflow-hidden flex">
                    <div
                      className="h-full bg-primary/60"
                      style={{ width: `${((a.starts - a.errors) / maxStarts) * 100}%` }}
                    />
                    {a.errors > 0 && (
                      <div
                        className="h-full bg-rose-500/70"
                        style={{ width: `${(a.errors / maxStarts) * 100}%` }}
                      />
                    )}
                  </div>
                  <span className="font-mono tabular-nums text-muted-foreground w-10 text-right">
                    {a.starts}
                  </span>
                  {a.errors > 0 && (
                    <span
                      className={cn(
                        "font-mono tabular-nums w-10 text-right text-[11px]",
                        errPct > 20 ? "text-rose-400" : "text-amber-400",
                      )}
                      title={`${a.errors} erro(s) de ${a.starts}`}
                    >
                      {errPct.toFixed(0)}%
                    </span>
                  )}
                  <span className="font-mono tabular-nums text-muted-foreground/70 w-14 text-right text-[11px]">
                    {a.avg_duration_ms > 0 ? formatDurationMs(a.avg_duration_ms) : "—"}
                  </span>
                </li>
              );
            })}
          </ul>
        </CardContent>
      </Card>
    </section>
  );
}

// ── 5. Ferramentas (uso acumulado) ────────────────────────────────────────────

function ToolsSection({ tools }: { tools: ToolCount[] }) {
  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="Ferramentas — uso acumulado"
        hint="Quantas vezes cada ferramenta foi chamada neste projeto desde sempre. Isso é uso, não economia. Útil para ver onde o Claude gasta o esforço."
      />
      {tools.length === 0 ? (
        <Card size="sm" className="ring-foreground/5">
          <CardContent>
            <p className="text-[12px] text-muted-foreground">Sem uso registrado.</p>
          </CardContent>
        </Card>
      ) : (
        <Card size="sm" className="ring-foreground/5">
          <CardContent>
            <ToolBars tools={tools} />
          </CardContent>
        </Card>
      )}
    </section>
  );
}

function ToolBars({ tools }: { tools: ToolCount[] }) {
  const max = tools[0]?.count ?? 1;
  return (
    <ul className="grid grid-cols-1 md:grid-cols-2 gap-x-6 gap-y-1">
      {tools.slice(0, 12).map((t) => (
        <li key={t.tool_name} className="flex items-baseline gap-2 text-[12.5px]">
          <span className="font-mono truncate w-28">{t.tool_name}</span>
          <div className="flex-1 h-1 bg-muted rounded overflow-hidden">
            <div
              className="h-full bg-slate-400/50"
              style={{ width: `${(t.count / max) * 100}%` }}
            />
          </div>
          <span className="font-mono tabular-nums text-muted-foreground w-12 text-right">
            {formatNumber(t.count)}
          </span>
        </li>
      ))}
    </ul>
  );
}

// ── Skeleton ──────────────────────────────────────────────────────────────────

function SkeletonRows({ n }: { n: number }) {
  return (
    <div className="flex flex-col gap-1">
      {Array.from({ length: n }).map((_, i) => (
        <div key={i} className="h-4 bg-muted/40 rounded animate-pulse" />
      ))}
    </div>
  );
}

// ── Phase drill-down sheet ────────────────────────────────────────────────────

function PhasePanelBody({
  phase,
  repoPath,
}: {
  phase: PhaseActivity;
  repoPath: string | null;
}) {
  const phaseName = phase.phase;
  const meta = PHASE_META[phaseName] ?? null;

  const recent = useQuery({
    queryKey: ["recent-events-phase", repoPath, phaseName],
    queryFn: () => fetchRecentEvents(repoPath!, 1000),
    enabled: !!repoPath,
    staleTime: REFRESH_FAST,
    refetchInterval: REFRESH_FAST,
  });

  const filtered = useMemo<RecentEvent[]>(() => {
    if (!recent.data) return [];
    const target = phaseName.toUpperCase();
    return recent.data.filter((e) => (e.phase ?? "").toUpperCase() === target);
  }, [recent.data, phaseName]);

  const specsTouched = useMemo(() => {
    const set = new Map<string, { count: number; lastTs: string | null }>();
    for (const e of filtered) {
      if (!e.spec) continue;
      const cur = set.get(e.spec) ?? { count: 0, lastTs: null };
      cur.count++;
      if (e.ts && (!cur.lastTs || e.ts > cur.lastTs)) cur.lastTs = e.ts;
      set.set(e.spec, cur);
    }
    return [...set.entries()].sort((a, b) => b[1].count - a[1].count);
  }, [filtered]);

  return (
    <div className="px-5 py-4 flex flex-col gap-5">
      {/* KPIs */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-0.5 py-2">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">5min</span>
            <span className="text-lg font-mono font-medium tabular-nums">
              {phase.events_last_5min}
            </span>
          </CardContent>
        </Card>
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-0.5 py-2">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">1h</span>
            <span className="text-lg font-mono font-medium tabular-nums">
              {phase.events_last_hour}
            </span>
          </CardContent>
        </Card>
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col gap-0.5 py-2">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">hoje</span>
            <span className="text-lg font-mono font-medium tabular-nums">
              {phase.events_today}
            </span>
          </CardContent>
        </Card>
      </div>

      {phase.top_tools.length > 0 && (
        <div className="flex flex-col gap-2">
          <h3 className="text-[10px] uppercase tracking-wider text-muted-foreground font-medium">
            Ferramentas nesta fase (hoje)
          </h3>
          <ul className="flex flex-col gap-1">
            {phase.top_tools.map((t) => (
              <li key={t.tool_name} className="flex items-baseline gap-2 text-[12.5px]">
                <span className="font-mono w-24 truncate">{t.tool_name}</span>
                <div className="flex-1 h-1 bg-muted rounded overflow-hidden">
                  <div
                    className={cn("h-full", meta?.bg ?? "bg-primary/60")}
                    style={{
                      width: `${
                        phase.top_tools[0]?.count
                          ? (t.count / phase.top_tools[0].count) * 100
                          : 0
                      }%`,
                    }}
                  />
                </div>
                <span className="font-mono tabular-nums text-muted-foreground w-10 text-right">
                  {t.count}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {specsTouched.length > 0 && (
        <div className="flex flex-col gap-2">
          <h3 className="text-[10px] uppercase tracking-wider text-muted-foreground font-medium">
            Specs etiquetadas ({specsTouched.length})
          </h3>
          <ul className="flex flex-col gap-1">
            {specsTouched.slice(0, 8).map(([spec, info]) => (
              <li key={spec} className="flex items-baseline gap-2 text-[12.5px]">
                <span className="font-mono truncate flex-1" title={spec}>
                  {spec}
                </span>
                <span className="font-mono tabular-nums text-muted-foreground w-8 text-right">
                  {info.count}
                </span>
                <span className="text-[11px] text-muted-foreground/70 w-16 text-right">
                  {info.lastTs ? relativeTime(info.lastTs) : "—"}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="flex flex-col gap-2">
        <h3 className="text-[10px] uppercase tracking-wider text-muted-foreground font-medium">
          Eventos recentes ({filtered.length})
        </h3>
        {recent.isLoading ? (
          <SkeletonRows n={5} />
        ) : filtered.length === 0 ? (
          <p className="text-[12px] text-muted-foreground">
            Sem eventos recentes nesta fase (busca limitada aos últimos 1.000
            eventos do projeto).
          </p>
        ) : (
          <ul className="flex flex-col gap-1">
            {filtered.slice(0, 50).map((e, i) => (
              <li
                key={`${e.ts}-${i}`}
                className="flex items-baseline gap-2 text-[12px] border-b border-border/30 last:border-0 py-1"
              >
                <span className="font-mono text-muted-foreground w-16">
                  {e.tool_name ?? e.event_type}
                </span>
                <span className="truncate flex-1 text-muted-foreground/80" title={e.target ?? ""}>
                  {e.target ?? e.summary ?? ""}
                </span>
                <span className="text-[10.5px] text-muted-foreground/70 w-14 text-right">
                  {e.ts ? relativeTime(e.ts) : "—"}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
