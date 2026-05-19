import { Fragment, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { ChevronRight, ChevronDown } from "lucide-react";
import { useStore } from "@/lib/store";
import type { Project as DiscoveryProject } from "@/api/discovery";
import {
  fetchQualityMetrics,
  fetchSpecs,
  useProjects,
  type QualityMetrics,
  type RoleQuality,
  type SlowestWave,
  type PhaseTokens,
  type SpecRow,
} from "@/lib/dashboard";
import { useActivityFeed } from "@/hooks/useActivityFeed";
import { SpecSidePanel } from "@/components/SpecSidePanel";
import { SplitDetail } from "@/components/layout/SplitDetail";
import { parseQaOverall } from "@/lib/qa";
import { phaseTheme, PHASE_ORDER, shortSpecName } from "@/lib/phaseTheme";
import {
  PageHeader,
  SectionHeader,
  KPICard,
  EmptyState,
  DataCard,
  PhaseChip,
  AcBreakdown,
  WaveRowLabel,
  CollapsibleGroup,
} from "@/components/page";
import { cn } from "@/lib/utils";

// Inline helpers — single-use formatters tied to this page
function fmtPct(ratio: number): string {
  return `${(ratio * 100).toFixed(1)}%`;
}
function fmtSec(ms: number): string {
  return `${(ms / 1000).toFixed(1)}s`;
}
function fmtSecExact(ms: number): string {
  return `${(ms / 1000).toFixed(2)}s`;
}
function pctClass(ratio: number, invert = false): string {
  const good = invert ? ratio < 0.2 : ratio >= 0.8;
  const mid = invert ? ratio < 0.5 : ratio >= 0.5;
  if (good) return "text-emerald-400";
  if (mid) return "text-amber-400";
  return "text-rose-400";
}

const Skeleton = () => (
  <div className="flex flex-col gap-2">
    {[0, 1, 2, 3].map((i) => (
      <div key={i} className="h-6 bg-muted/40 rounded animate-pulse" />
    ))}
  </div>
);

/**
 * Column legend for the specs table. Spells out what each abbreviation means
 * AND why a column may be blank — so an empty WAVES/AC/RETRIES cell reads as
 * "nothing happened yet" instead of "the dashboard is broken".
 */
function QualityLegend() {
  const items: { term: string; meaning: string; empty: string }[] = [
    {
      term: "FASE",
      meaning: "Etapa atual do pipeline (ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE).",
      empty: "Em branco = a spec ainda não tem pipeline-state — nunca foi iniciada.",
    },
    {
      term: "WAVES",
      meaning: "Número de waves de um wave plan (roadmap). Specs simples não têm waves.",
      empty: "— significa que não é um wave plan, ou o wave-plan.md ainda não foi escrito.",
    },
    {
      term: "AC",
      meaning: "Critérios de Aceitação do QA: passou / falhou / pulou. Esta é a medida real de acerto.",
      empty: "— significa que nenhum QA rodou ainda; preenche quando a pipeline chega na fase QA.",
    },
    {
      term: "RETRIES",
      meaning: "Quantas vezes uma wave teve que ser refeita (evento retry.attempt).",
      empty: "0 = a wave rodou sem precisar refazer. Bom sinal.",
    },
  ];
  return (
    <div className="rounded-lg border border-border bg-card/30 px-4 py-3 flex flex-col gap-2">
      <span className="text-[10px] uppercase tracking-wider font-medium text-muted-foreground">
        Como ler a tabela
      </span>
      <dl className="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-2">
        {items.map((it) => (
          <div key={it.term} className="flex flex-col gap-0.5">
            <dt className="text-[11px] font-mono font-medium text-foreground tracking-wider">
              {it.term}
            </dt>
            <dd className="text-[12px] text-muted-foreground leading-snug">
              {it.meaning}{" "}
              <span className="text-muted-foreground/60">{it.empty}</span>
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

type WaveStats = { qaPass: number; qaFail: number; qaSkip: number; retries: number };

export function Quality() {
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const [selectedSpec, setSelectedSpec] = useState<SpecRow | null>(null);
  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;

  const { data: specsData } = useQuery({
    queryKey: ["specs", activeProject?.path],
    queryFn: () => fetchSpecs(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 15_000,
  });

  const projectsForFeed = activeProject ? [activeProject as unknown as DiscoveryProject] : [];
  const { events: feedEvents } = useActivityFeed(projectsForFeed, 500);

  /**
   * Per-spec aggregation with wave breakdown nested inside.
   *   total      — soma de todas as waves (mostrado na linha da spec-mãe)
   *   byWave     — Map<waveNumber, WaveStats> pra drilldown
   *   waveActors — actor.id por wave (backend-impl, frontend-impl, etc.)
   */
  const specStats = useMemo(() => {
    type Acc = {
      total: WaveStats;
      byWave: Map<number, WaveStats>;
      waveActors: Map<number, Set<string>>;
    };
    const map = new Map<string, Acc>();
    const blank = (): WaveStats => ({ qaPass: 0, qaFail: 0, qaSkip: 0, retries: 0 });
    const ensure = (spec: string): Acc => {
      if (!map.has(spec)) map.set(spec, { total: blank(), byWave: new Map(), waveActors: new Map() });
      return map.get(spec)!;
    };
    const bumpQa = (s: WaveStats, v: "pass" | "fail" | "skip") => {
      if (v === "pass") s.qaPass++;
      else if (v === "fail") s.qaFail++;
      else s.qaSkip++;
    };

    for (const row of feedEvents) {
      const spec = row.event.spec;
      if (!spec) continue;
      const acc = ensure(spec);
      const wave = typeof row.event.wave === "number" ? row.event.wave : null;
      if (wave !== null) {
        if (!acc.byWave.has(wave)) acc.byWave.set(wave, blank());
        if (row.event.actor_id) {
          if (!acc.waveActors.has(wave)) acc.waveActors.set(wave, new Set());
          acc.waveActors.get(wave)!.add(row.event.actor_id);
        }
      }
      if (row.event.event_type === "qa.result") {
        const v = parseQaOverall(row.event.summary);
        if (v) {
          bumpQa(acc.total, v);
          if (wave !== null) bumpQa(acc.byWave.get(wave)!, v);
        }
      }
      if (row.event.event_type === "retry.attempt") {
        acc.total.retries++;
        if (wave !== null) acc.byWave.get(wave)!.retries++;
      }
    }
    return map;
  }, [feedEvents]);

  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const toggleExpand = (name: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  const { data, isLoading, error, refetch } = useQuery<QualityMetrics>({
    queryKey: ["quality", activeProject?.path],
    queryFn: () => fetchQualityMetrics(activeProject!.path),
    enabled: !!activeProject,
    staleTime: 30_000,
  });

  useEffect(() => {
    if (error) toast.error("Failed to load quality metrics");
  }, [error]);

  const isEmpty = data && data.by_role.length === 0 && data.slowest_waves.length === 0;

  const maxPhaseTokens = Math.max(
    ...(data?.tokens_by_phase.map((p) => Math.max(p.input_avg + p.output_avg, 1)) ?? [1]),
    1,
  );

  return (
    <SplitDetail
      open={!!selectedSpec}
      panel={
        selectedSpec ? (
          <SpecSidePanel
            projectId={activeWorkspaceId}
            projectPath={activeProject?.path ?? null}
            spec={selectedSpec}
            allSpecs={specsData ?? []}
            onClose={() => setSelectedSpec(null)}
          />
        ) : null
      }
    >
      <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={[
          "Mustard",
          "Qualidade",
          ...(activeProject ? [{ label: activeProject.name, mono: true }] : []),
        ]}
        title="Qualidade"
        subtitle={activeProject?.name}
        description="Cada spec é uma feature, bugfix ou roadmap deste projeto. As métricas do topo somam todo o histórico; a tabela abaixo mostra spec-por-spec em qual fase está, quantos critérios de aceitação passaram, e quantas waves precisaram refazer. Wave plans (roadmaps com várias waves) podem ser expandidos pra ver cada wave individualmente."
      />

      {!activeWorkspaceId && (
        <EmptyState
          title="Selecione um workspace"
          description="Use o seletor no topo para escolher um projeto e ver os dados desta página."
        />
      )}

      {error && activeProject && (
        <EmptyState
          variant="error"
          title="Falha ao carregar qualidade"
          description={(error as Error).message}
          right={
            <button
              onClick={() => refetch()}
              className="text-xs border border-destructive/40 rounded px-2 py-0.5 hover:bg-destructive/10 transition-colors"
            >
              Retry
            </button>
          }
        />
      )}

      {isLoading && activeProject && <Skeleton />}

      {/* KPI ribbon — historical aggregate */}
      {data && !isEmpty && !isLoading && (
        <section className="grid grid-cols-1 sm:grid-cols-3 gap-3 w-full">
          <KPICard
            label="Specs concluídas"
            value={fmtPct(data.pass_at_1)}
            accent={data.pass_at_1 >= 0.8 ? "emerald" : data.pass_at_1 >= 0.5 ? "amber" : "rose"}
            valueClassName={cn("text-2xl font-mono font-medium tabular-nums leading-tight", pctClass(data.pass_at_1))}
            hint="specs concluídas sobre o total"
            tooltip="Fração das specs deste projeto que já foram concluídas (status=completed) sobre o total. NÃO é pass@1 de QA — para acerto real de QA veja a coluna AC da tabela abaixo."
          />
          <KPICard
            label="Precisou refazer"
            value={fmtPct(data.fix_loop_rate)}
            accent={data.fix_loop_rate < 0.2 ? "emerald" : data.fix_loop_rate < 0.5 ? "amber" : "rose"}
            valueClassName={cn("text-2xl font-mono font-medium tabular-nums leading-tight", pctClass(data.fix_loop_rate, true))}
            hint="waves que entraram em fix-loop"
            tooltip="% das waves que precisaram de pelo menos 1 fix-loop. BAIXO é melhor — alto significa bug saiu no review."
          />
          <KPICard
            label="Tempo médio por fase"
            value={data.avg_phase_duration_ms > 0 ? fmtSec(data.avg_phase_duration_ms) : "—"}
            accent="indigo"
            valueClassName="text-2xl font-mono font-medium tabular-nums leading-tight text-foreground"
            hint="média entre todas as fases"
            tooltip="Tempo médio que cada fase (ANALYZE/PLAN/EXECUTE/QA) leva. Útil pra detectar regressão de performance."
          />
        </section>
      )}

      {isEmpty && !isLoading && (
        <EmptyState
          title="Sem dados de QA históricos"
          description={
            <>
              Pipelines precisam rodar com Mustard ≥ Phase 1 e emitir eventos{" "}
              <code className="font-mono">qa.result</code>.
            </>
          }
        />
      )}

      {/* Specs table — main content */}
      {activeProject && (
        <section className="flex flex-col gap-3 w-full">
          <SectionHeader
            title="Specs deste projeto"
            description="Specs em andamento ficam agrupadas pela fase atual. Wave plans aparecem como uma linha-mãe com triângulo ▸ — clique pra expandir e ver as waves individuais. Cada wave mostra de qual spec-mãe veio, mesmo quando você rola a tabela."
          />

          <QualityLegend />

          {!specsData || specsData.length === 0 ? (
            <p className="text-[13px] text-muted-foreground">Nenhuma spec encontrada.</p>
          ) : (() => {
            // Build parent → children tree (backend sets `parent` field).
            const all = specsData;
            const parents = all.filter((s) => !s.parent);
            const childrenByParent = new Map<string, SpecRow[]>();
            for (const s of all) {
              if (s.parent) {
                if (!childrenByParent.has(s.parent)) childrenByParent.set(s.parent, []);
                childrenByParent.get(s.parent)!.push(s);
              }
            }
            for (const list of childrenByParent.values()) {
              list.sort((a, b) => {
                const na = parseInt(a.name.match(/wave-?(\d+)/i)?.[1] ?? "0", 10);
                const nb = parseInt(b.name.match(/wave-?(\d+)/i)?.[1] ?? "0", 10);
                return na - nb;
              });
            }
            const sorted = parents.slice().sort((a, b) => a.name.localeCompare(b.name));
            // Bucket from filesystem is ground truth — more reliable than status strings.
            const draft = sorted.filter((s) => s.status === "draft");
            const closed = sorted.filter((s) => s.bucket === "completed" && s.status !== "draft");
            const cancelled = sorted.filter((s) => s.bucket === "cancelled");
            const active = sorted.filter((s) => {
              if (s.status === "draft") return false;
              if (s.bucket === "completed" || s.bucket === "cancelled") return false;
              return true;
            });

            // Group active by phase
            const byPhase = new Map<string, SpecRow[]>();
            for (const s of active) {
              const p = (s.phase ?? "").toUpperCase().trim() || "—";
              if (!byPhase.has(p)) byPhase.set(p, []);
              byPhase.get(p)!.push(s);
            }
            const orderedPhases = [
              ...PHASE_ORDER.filter((p) => byPhase.has(p)),
              ...Array.from(byPhase.keys()).filter((p) => !PHASE_ORDER.includes(p)),
            ];

            const renderSpecTable = (rows: SpecRow[]) => (
              <DataCard>
                <table className="w-full text-[13px]">
                  <thead className="bg-muted/20 border-b border-border">
                    <tr className="text-left text-[10px] uppercase tracking-wider text-muted-foreground/80">
                      <th className="px-4 py-2 font-medium">
                        <span title="Nome da spec (sem o prefixo de data) ou número da wave dentro dela.">
                          Spec / Wave
                        </span>
                      </th>
                      <th className="px-2 py-2 font-medium w-[100px]">
                        <span title="Em qual fase do pipeline a spec está agora">Fase</span>
                      </th>
                      <th className="px-2 py-2 font-medium w-[70px] text-center">
                        <span title="Quantas waves o pipeline tem (wave plans só)">Waves</span>
                      </th>
                      <th className="px-2 py-2 font-medium w-[220px]">
                        <span title="Critérios de Aceitação: passou / falhou / pulou">AC</span>
                      </th>
                      <th className="px-2 py-2 font-medium w-[80px] text-right">
                        <span title="Quantas vezes precisou refazer (fix-loop)">Retries</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((spec) => {
                      const stats = specStats.get(spec.name);
                      const t = stats?.total ?? { qaPass: 0, qaFail: 0, qaSkip: 0, retries: 0 };
                      const fsChildren = childrenByParent.get(spec.name) ?? [];
                      const eventWaveCount = stats?.byWave.size ?? 0;
                      const waveCount = fsChildren.length > 0 ? fsChildren.length : eventWaveCount;
                      const isExpanded = expanded.has(spec.name);
                      const hasChildren = waveCount > 0;
                      const ptheme = phaseTheme(spec.phase);
                      const eventWavesSorted = stats
                        ? Array.from(stats.byWave.entries()).sort(([a], [b]) => a - b)
                        : [];

                      return (
                        <Fragment key={spec.name}>
                          <tr
                            className={cn(
                              "border-t border-border/60 hover:bg-muted/20 transition-colors group",
                              isExpanded && "bg-muted/10",
                            )}
                          >
                            <td className="px-4 py-2.5 relative">
                              <div className={cn("absolute left-0 top-0 bottom-0 w-0.5", ptheme.stripe)} />
                              <div className="flex items-center gap-2 min-w-0">
                                {hasChildren ? (
                                  <button
                                    type="button"
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      toggleExpand(spec.name);
                                    }}
                                    className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
                                    aria-label={isExpanded ? "Recolher waves" : "Expandir waves"}
                                  >
                                    {isExpanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
                                  </button>
                                ) : (
                                  <span className="w-3.5 shrink-0" />
                                )}
                                <button
                                  type="button"
                                  className="font-mono text-[12.5px] truncate cursor-pointer hover:text-primary transition-colors text-left min-w-0"
                                  title={spec.name}
                                  onClick={() => setSelectedSpec(spec)}
                                >
                                  {shortSpecName(spec.name)}
                                </button>
                              </div>
                            </td>
                            <td className="px-2 py-2.5">
                              <PhaseChip phase={spec.phase} />
                            </td>
                            <td className="px-2 py-2.5 text-center">
                              {waveCount > 0 ? (
                                <span
                                  className="inline-flex items-center justify-center min-w-[28px] rounded-md px-1.5 py-0.5 text-[11px] font-medium bg-primary/15 text-primary border border-primary/30 tabular-nums"
                                  title={
                                    fsChildren.length > 0
                                      ? `${fsChildren.length} wave(s) definida(s) no wave-plan.md`
                                      : `${eventWaveCount} wave(s) detectada(s) nos eventos`
                                  }
                                >
                                  {waveCount}
                                </span>
                              ) : (
                                <span className="text-muted-foreground/40">—</span>
                              )}
                            </td>
                            <td className="px-2 py-2.5">
                              <AcBreakdown pass={t.qaPass} fail={t.qaFail} skip={t.qaSkip} />
                            </td>
                            <td className="px-2 py-2.5 font-mono text-[12px] text-right tabular-nums">
                              {t.retries > 0 ? (
                                <span className="text-amber-400">{t.retries}</span>
                              ) : (
                                <span className="text-muted-foreground/40">0</span>
                              )}
                            </td>
                          </tr>

                          {/* Wave children — FS-backed */}
                          {isExpanded && fsChildren.length > 0 &&
                            fsChildren.map((child, idx) => {
                              const num = parseInt(child.name.match(/wave-?(\d+)/i)?.[1] ?? "", 10);
                              const w = !isNaN(num) ? stats?.byWave.get(num) : undefined;
                              const wStats = w ?? { qaPass: 0, qaFail: 0, qaSkip: 0, retries: 0 };
                              const role = child.name.replace(/^wave-?\d+-?/i, "") || "—";
                              const isLast = idx === fsChildren.length - 1;
                              return (
                                <tr
                                  key={`${spec.name}/${child.name}`}
                                  className="hover:bg-muted/15 cursor-pointer transition-colors"
                                  onClick={() => setSelectedSpec(child)}
                                >
                                  <td className="px-4 py-1.5 relative">
                                    <div
                                      className={cn(
                                        "absolute left-0 top-0 bottom-0 w-0.5",
                                        ptheme.stripe,
                                        isLast && "bottom-1",
                                      )}
                                    />
                                    <div className="pl-5">
                                      <WaveRowLabel
                                        waveNumber={!isNaN(num) ? num : null}
                                        role={role}
                                        parentName={spec.name}
                                      />
                                    </div>
                                  </td>
                                  <td className="px-2 py-1.5">
                                    <PhaseChip phase={child.phase} size="sm" />
                                  </td>
                                  <td className="px-2 py-1.5" />
                                  <td className="px-2 py-1.5">
                                    <AcBreakdown pass={wStats.qaPass} fail={wStats.qaFail} skip={wStats.qaSkip} />
                                  </td>
                                  <td className="px-2 py-1.5 font-mono text-[11.5px] text-right tabular-nums">
                                    {wStats.retries > 0 ? (
                                      <span className="text-amber-400/80">{wStats.retries}</span>
                                    ) : (
                                      <span className="text-muted-foreground/40">0</span>
                                    )}
                                  </td>
                                </tr>
                              );
                            })}

                          {/* Event-only waves (no FS dir but events tagged with wave) */}
                          {isExpanded && fsChildren.length === 0 &&
                            eventWavesSorted.map(([waveNum, w], idx) => {
                              const actors = stats!.waveActors.get(waveNum);
                              const actorLabel = actors && actors.size > 0 ? Array.from(actors).join(" + ") : "—";
                              const isLast = idx === eventWavesSorted.length - 1;
                              return (
                                <tr
                                  key={`${spec.name}-w${waveNum}`}
                                  className="hover:bg-muted/15 transition-colors"
                                >
                                  <td className="px-4 py-1.5 relative">
                                    <div
                                      className={cn(
                                        "absolute left-0 top-0 bottom-0 w-0.5",
                                        ptheme.stripe,
                                        isLast && "bottom-1",
                                      )}
                                    />
                                    <div className="pl-5">
                                      <WaveRowLabel
                                        waveNumber={waveNum}
                                        role={actorLabel}
                                        parentName={spec.name}
                                      />
                                    </div>
                                  </td>
                                  <td className="px-2 py-1.5">
                                    <span
                                      className="text-[10.5px] text-muted-foreground/60 italic"
                                      title="Wave detectada apenas nos eventos (sem dir wave-N no FS)"
                                    >
                                      apenas eventos
                                    </span>
                                  </td>
                                  <td className="px-2 py-1.5" />
                                  <td className="px-2 py-1.5">
                                    <AcBreakdown pass={w.qaPass} fail={w.qaFail} skip={w.qaSkip} />
                                  </td>
                                  <td className="px-2 py-1.5 font-mono text-[11.5px] text-right tabular-nums">
                                    {w.retries > 0 ? (
                                      <span className="text-amber-400/80">{w.retries}</span>
                                    ) : (
                                      <span className="text-muted-foreground/40">0</span>
                                    )}
                                  </td>
                                </tr>
                              );
                            })}
                        </Fragment>
                      );
                    })}
                  </tbody>
                </table>
              </DataCard>
            );

            return (
              <div className="flex flex-col gap-5 w-full">
                {active.length > 0 && (
                  <div className="flex flex-col gap-3">
                    <div className="flex items-baseline gap-3 flex-wrap">
                      <h3 className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground/80">
                        Em andamento ({active.length})
                      </h3>
                      <span className="text-[11px] text-muted-foreground/50">agrupado por fase</span>
                    </div>
                    <div className="flex flex-col gap-4">
                      {orderedPhases.map((phaseKey) => {
                        const rows = byPhase.get(phaseKey) ?? [];
                        const t = phaseTheme(phaseKey);
                        return (
                          <div key={phaseKey} className="flex flex-col gap-1.5">
                            <div className="flex items-baseline gap-2.5 flex-wrap">
                              <span
                                className={cn(
                                  "inline-flex items-center rounded-md px-2 py-0.5 text-[11px] font-medium border",
                                  t.text,
                                  t.bg,
                                  t.border,
                                )}
                              >
                                {phaseKey === "—" ? "Sem fase" : phaseKey}
                              </span>
                              <span className="text-[12px] text-muted-foreground/80">{t.detail}</span>
                              <span className="text-[11px] text-muted-foreground/50 ml-auto tabular-nums">
                                {rows.length} {rows.length === 1 ? "spec" : "specs"}
                              </span>
                            </div>
                            {renderSpecTable(rows)}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}
                {draft.length > 0 && (
                  <div className="flex flex-col gap-2">
                    <h3 className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground/70">
                      Rascunho ({draft.length})
                    </h3>
                    {renderSpecTable(draft)}
                  </div>
                )}
                <CollapsibleGroup
                  label="Fechadas"
                  count={closed.length}
                  hint={
                    <>
                      Specs já concluídas (em <code className="font-mono">.claude/spec/completed/</code>). Colapsado por padrão pra não poluir; clique pra ver a lista.
                    </>
                  }
                >
                  {renderSpecTable(closed)}
                </CollapsibleGroup>
                <CollapsibleGroup
                  label="Canceladas"
                  count={cancelled.length}
                  hint="Specs que foram canceladas via /mustard:complete --cancel."
                >
                  {renderSpecTable(cancelled)}
                </CollapsibleGroup>
              </div>
            );
          })()}
        </section>
      )}

      {/* Historical breakdowns */}
      {data && !isEmpty && !isLoading && (
        <div className="flex flex-col gap-6 w-full">
          <section className="flex flex-col gap-2">
            <SectionHeader
              title="Qualidade por papel de agente"
              description="Cada role (backend, frontend, db, etc) tem sua própria estatística — útil pra detectar qual área acumula mais fix-loops. A coluna Pass@1 por papel foi removida: o backend a fixava em 0,0% (não era calculada), e exibir um número falso é pior do que não exibir."
            />
            <DataCard>
              <table className="w-full text-[13px]">
                <thead className="bg-muted/20">
                  <tr className="text-left text-[10px] uppercase tracking-wider text-muted-foreground/80">
                    <th className="px-4 py-2 font-medium">Papel</th>
                    <th className="px-2 py-2 font-medium text-right">
                      <span title="Quantos fix-loops (correções após review/QA) este papel acumulou. Vem da tabela `spans` do SQLite — pode estar desatualizada pós-Wave 4.">
                        Fix loops
                      </span>
                    </th>
                    <th className="px-2 py-2 font-medium text-right">
                      <span title="Quantas amostras (waves) sustentam os números desta linha. Poucas amostras = leia com cautela.">
                        Amostras
                      </span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {data.by_role.map((row: RoleQuality) => (
                    <tr key={row.role} className="border-t border-border/60 hover:bg-muted/10">
                      <td className="px-4 py-1.5 font-mono">{row.role}</td>
                      <td className="px-2 py-1.5 font-mono text-right text-muted-foreground tabular-nums">
                        {row.fix_loops.toLocaleString()}
                      </td>
                      <td className="px-2 py-1.5 font-mono text-right text-muted-foreground tabular-nums">
                        {row.samples.toLocaleString()}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </DataCard>
          </section>

          {data.slowest_waves.length > 0 && (
            <section className="flex flex-col gap-2">
              <SectionHeader
                title="Waves mais lentas"
                description="Top 5 waves que demoraram mais — alvos pra investigar gargalos ou prompts inflados."
              />
              <DataCard>
                <ul className="flex flex-col">
                  {data.slowest_waves.slice(0, 5).map((w: SlowestWave, i: number) => (
                    <li
                      key={`${w.spec}-${w.wave}-${i}`}
                      className="flex items-baseline gap-3 px-4 py-2 hover:bg-muted/15 text-[13px] border-t border-border/40 first:border-t-0 transition-colors"
                    >
                      <span className="text-muted-foreground/50 font-mono w-4 text-xs tabular-nums">
                        {i + 1}
                      </span>
                      <span className="font-mono truncate flex-1 text-foreground/90">{w.spec ?? "—"}</span>
                      <span className="text-muted-foreground text-xs">wave {w.wave ?? "—"}</span>
                      <span className="font-mono text-xs tabular-nums text-amber-400">
                        {fmtSecExact(w.duration_ms)}
                      </span>
                    </li>
                  ))}
                </ul>
              </DataCard>
            </section>
          )}

          {data.tokens_by_phase.length > 0 && (
            <section className="flex flex-col gap-2">
              <SectionHeader
                title="Tokens por fase"
                description="Quanto de prompt (input) e resposta (output) cada fase consome em média. Indigo = entrada, esmeralda = saída."
              />
              <DataCard padded>
                <div className="flex flex-col gap-2">
                  {data.tokens_by_phase.map((p: PhaseTokens) => {
                    const total = p.input_avg + p.output_avg;
                    const t = phaseTheme(p.phase);
                    return (
                      <div key={p.phase} className="flex items-center gap-3 text-[12.5px]">
                        <span className={cn("font-mono w-20 text-xs", t.text)}>{p.phase}</span>
                        <div className="flex-1 flex h-1.5 rounded-full overflow-hidden bg-muted/20">
                          <div
                            className="bg-primary/60"
                            style={{ width: `${(p.input_avg / maxPhaseTokens) * 100}%` }}
                            title={`input avg: ${Math.round(p.input_avg).toLocaleString()}`}
                          />
                          <div
                            className="bg-emerald-500/60"
                            style={{ width: `${(p.output_avg / maxPhaseTokens) * 100}%` }}
                            title={`output avg: ${Math.round(p.output_avg).toLocaleString()}`}
                          />
                        </div>
                        <span className="font-mono text-xs text-muted-foreground tabular-nums w-32 text-right whitespace-nowrap">
                          in {Math.round(p.input_avg).toLocaleString()} / out{" "}
                          {Math.round(p.output_avg).toLocaleString()}
                        </span>
                        <span className="font-mono text-xs text-foreground/80 tabular-nums w-20 text-right whitespace-nowrap">
                          ≈{Math.round(total).toLocaleString()}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </DataCard>
            </section>
          )}
        </div>
      )}
      </div>
    </SplitDetail>
  );
}
