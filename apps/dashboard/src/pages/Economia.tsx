// Economia — didactic rewrite (spec
// `.claude/spec/2026-05-22-economia-didatica-e-economias-reais/wave-3-ui`).
//
// The Wave-3 brief is plain: every card needs a PT title + one-line caption
// that says what it measures and why it matters. Internal DTO field names
// and raw module names MUST NOT appear as user-facing labels — AC-3 greps
// this file for those literals (see spec) and fails the build if any slip
// through. Keep this comment paraphrased to avoid tripping the grep.
//
// The data hook (`useEconomySummary`) is unchanged from Wave 7: it routes
// through the typed `lib/dashboard.ts` wrappers and the `<ScopeBar>` drives
// the same query key. The new wire we now consume is `SessionCost.last_at_ms`
// + `SessionCost.specs`, populated by Wave 1 of this spec.

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import dayjs from "dayjs";
import { AlertTriangle, Info } from "lucide-react";
import { cn } from "@/lib/utils";
import { useStore } from "@/lib/store";
import { EmptyState, KPICard } from "@/components/page";
import { MetricsPill } from "@/components/ds";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { relativeTime } from "@/lib/time";
import { useProjects } from "@/lib/dashboard";
import {
  fetchEconomySavingsBreakdown,
  fetchEconomyContextRouting,
  fetchEconomyPerSpecCosts,
  fetchEconomyPerWaveCosts,
} from "@/lib/dashboard";
import { useEconomySummary } from "@/hooks/useEconomySummary";
import { useCollectorHealth } from "@/hooks/usePromptEconomy";
import type { CollectorHealth } from "@/api/promptEconomy";
import { ScopeBar } from "@/components/economy/ScopeBar";
import { PerAgentTable } from "@/components/economy/PerAgentTable";
import { SavingsBreakdownCard } from "@/components/economy/SavingsBreakdownCard";
import type { EconomyScope, SpecCost, WaveCost } from "@/lib/types/economy";
import { projectScope, formatTokens, formatUsd } from "@/lib/types/economy";


export function Economia() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;
  const repoPath = activeProject?.path ?? null;

  // Initial scope = the active workspace as a Project scope. The user can
  // switch to Spec/Wave/Comparar via `<ScopeBar>`.
  const [scope, setScope] = useState<EconomyScope | null>(() =>
    repoPath ? projectScope(repoPath) : null,
  );

  // Re-seed the scope when the workspace changes (project switch in sidebar).
  // We compare on `repoPath`, not the whole project object, so a benign
  // rerender from React Query doesn't wipe a Spec/Wave selection.
  useEffect(() => {
    if (repoPath && (scope === null || scopeProjectKey(scope) !== repoPath)) {
      setScope(projectScope(repoPath));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [repoPath]);

  const summary = useEconomySummary(scope);

  // Two extra typed wrappers — both fail-soft on the backend, so the React
  // Query layer never surfaces a hard error for missing data, just empty.
  const breakdown = useQuery({
    queryKey: ["economy-savings", scope && scopeKey(scope)],
    queryFn: () => fetchEconomySavingsBreakdown(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 15_000,
    refetchInterval: 30_000,
  });

  const routing = useQuery({
    queryKey: ["economy-routing", scope && scopeKey(scope)],
    queryFn: () => fetchEconomyContextRouting(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 15_000,
    refetchInterval: 30_000,
  });

  const perSpec = useQuery({
    queryKey: ["economy-per-spec", scope && scopeKey(scope)],
    queryFn: () => fetchEconomyPerSpecCosts(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 15_000,
    refetchInterval: 30_000,
  });

  const perWave = useQuery({
    queryKey: ["economy-per-wave", scope && scopeKey(scope)],
    queryFn: () => fetchEconomyPerWaveCosts(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 15_000,
    refetchInterval: 30_000,
  });

  // Collector-health badge — tells the user the cost number is CURRENT, not a
  // ghost from a crashed collector. Same hook every other economy page uses.
  const collectorHealth = useCollectorHealth(repoPath);

  // ── Empty / config states ────────────────────────────────────────────────
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

  if (!activeWorkspaceId || !repoPath || !scope) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <EmptyState
          title="Selecione um workspace"
          description="Use o seletor na sidebar para escolher um projeto."
        />
      </div>
    );
  }

  // ── Derived KPI numbers ──────────────────────────────────────────────────
  const data = summary.data;
  const cacheRatio = (routing.data?.cache_hit_ratio_permille ?? 0) / 10; // -> percent

  // ── Freshness / collector-health badge ───────────────────────────────────
  // `last_updated_ms` is epoch-ms of the last MEASURED counter (project scope
  // only). The badge label maps the unified collector state to PT and the
  // relative-time tail reads from the measured timestamp so it tracks the
  // headline cost, not the badge's own 60s poll.
  const health = collectorHealth.data;
  const lastUpdatedIso =
    typeof data?.last_updated_ms === "number"
      ? dayjs(data.last_updated_ms).toISOString()
      : null;
  const updatedAgo = lastUpdatedIso ? relativeTime(lastUpdatedIso) : null;
  const { badgeLabel, badgeVariant } = collectorBadge(health);
  const sessions = data?.by_session ?? [];

  // ── Ingestion staleness signal ───────────────────────────────────────────
  // The estimated path (`run_usage`) and the measured path (`usage_totals`)
  // are fed by different writers. When the OTEL collector daemon stops or
  // Claude Code is not exporting OTEL, the estimated table freezes while the
  // measured counters keep advancing. We surface a banner once the gap
  // crosses STALENESS_HOURS so the user knows the per-spec estimates are
  // out of date — without having to compare timestamps by hand.
  const STALENESS_HOURS = 6;
  const lastMeasuredMs = data?.last_updated_ms ?? null;
  const lastEstimatedMs = data?.last_estimated_ms ?? null;
  const ingestionStaleHours =
    lastMeasuredMs != null && lastEstimatedMs != null
      ? (lastMeasuredMs - lastEstimatedMs) / 3_600_000
      : null;
  const showStaleBanner =
    (scope.kind === "project" || scope.kind === "all_projects") &&
    ingestionStaleHours != null &&
    ingestionStaleHours > STALENESS_HOURS;

  // ── Distribuição por agente (light, horizontal-bar style w/o chart lib) ─
  // We render the top agents as proportional bars sized by `tokens`. No
  // recharts/d3 dependency — pure flex + Tailwind widths.
  const topAgents = data?.top_agents_by_cost ?? [];
  const tokensMax = topAgents.reduce((acc, a) => Math.max(acc, a.tokens), 0);

  return (
    <div className="flex flex-col gap-6 w-full">
      <ScopeBar projectPath={repoPath} scope={scope} onScopeChange={setScope} />

      {showStaleBanner && ingestionStaleHours != null && (
        <IngestionStaleBanner hours={ingestionStaleHours} />
      )}

      {/* ── KPI cards: custo, economia, cache hit ──────────────────────── */}
      <section className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <KPICard
          label="Custo do projeto (medido)"
          value={summary.isLoading ? "…" : formatUsd(data?.total_cost_usd_micros ?? 0)}
          hint={`${(data?.span_count ?? 0).toLocaleString()} execuções · ${formatTokens(data?.total_tokens ?? 0)} tokens`}
          accent={data && data.total_cost_usd_micros > 0 ? "indigo" : "zinc"}
          caption={
            <div className="flex flex-col gap-1">
              <div className="flex items-center gap-1.5">
                <StatusDot
                  variant={badgeVariant}
                  pulse={badgeVariant === "active"}
                  size="sm"
                />
                <span>{badgeLabel}</span>
                {updatedAgo ? <span>· atualizado {updatedAgo}</span> : null}
              </div>
              <span>cobrado pela Anthropic, somado por sessão</span>
            </div>
          }
        />
        <KPICard
          label="Economia total (tokens)"
          value={summary.isLoading ? "…" : `${formatTokens(data?.total_tokens_saved ?? 0)} tok`}
          hint="abaixo, o detalhe por origem"
          accent={data && data.total_tokens_saved > 0 ? "emerald" : "zinc"}
          caption="tokens que a ferramenta evitou de gastar — abaixo, o detalhe por origem"
        />
        <KPICard
          label="Cache hit"
          value={
            routing.isLoading ? "…" : routing.data ? `${cacheRatio.toFixed(1)}%` : "—"
          }
          hint={routing.data ? cacheHitTier(cacheRatio) : "sem dados nesta janela"}
          accent={cacheRatio >= 80 ? "emerald" : cacheRatio >= 50 ? "amber" : "zinc"}
          caption={
            <div className="flex flex-col gap-1">
              <span>
                tokens servidos do cache ÷ (cache + escrita no cache + input novo).
                Acima de 80% é ótimo — a Anthropic cobra só 10% do preço normal nesses tokens.
              </span>
              {scope.kind === "wave" && (
                <span className="text-amber-500/80">
                  ⓘ no filtro de Wave, o número é da spec inteira — o cache da
                  Anthropic não distingue waves dentro de uma mesma spec.
                </span>
              )}
            </div>
          }
        />
      </section>

      {summary.error ? (
        <EmptyState
          variant="warning"
          title="Falha ao ler os dados de economia"
          description={String((summary.error as Error)?.message ?? summary.error)}
        />
      ) : null}

      {/* ── Por agente (top-N) ─────────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium">
            {topAgents.length > 0 ? `Por agente (top ${topAgents.length})` : "Por agente"}
          </h2>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            agentes que mais consumiram tokens nesta janela
          </p>
        </header>
        <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] overflow-hidden">
          <PerAgentTable agents={topAgents} />
        </div>
      </section>

      {/* ── Distribuição por agente (horizontal bars sem chart lib) ────── */}
      {topAgents.length > 0 && (
        <section className="flex flex-col gap-3">
          <header className="flex flex-col gap-0.5">
            <h2 className="text-sm font-medium">Distribuição de tokens por agente</h2>
            <p className="text-[11px] text-[--ds-text-tertiary]">
              cada barra é proporcional aos tokens consumidos pelo agente
            </p>
          </header>
          <div className="flex flex-col gap-1.5">
            {topAgents.map((a) => {
              const pct = tokensMax > 0 ? (a.tokens / tokensMax) * 100 : 0;
              return (
                <div
                  key={a.agent_id}
                  className="flex items-center gap-3 px-3 py-2 rounded-[--ds-radius-md] bg-[--ds-surface-base]"
                >
                  <span className="font-mono text-[12px] text-[--ds-text-primary] truncate w-[180px] shrink-0">
                    {a.agent_id || "—"}
                  </span>
                  <div className="flex-1 h-2 rounded bg-[--ds-surface-hover] overflow-hidden">
                    <div
                      className="h-full bg-[--ds-accent-primary]/60"
                      style={{ width: `${pct.toFixed(2)}%` }}
                    />
                  </div>
                  <MetricsPill value={formatTokens(a.tokens)} unit="tok" />
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* ── Por sessão (custo medido por sessão do Claude Code) ───────── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium">Por sessão</h2>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            uma linha por sessão do Claude Code — compare o custo com <code className="font-mono">/cost</code> para conferir
          </p>
        </header>
        {sessions.length > 0 ? (
          <div className="flex flex-col gap-1">
            {sessions.map((s) => (
              <SessionRow
                key={`sess-${s.session_id}`}
                sessionId={s.session_id}
                usd={s.usd}
                lastAtMs={s.last_at_ms}
                specs={s.specs}
              />
            ))}
          </div>
        ) : scope.kind === "spec" || scope.kind === "wave" ? (
          <EmptyState
            title="Não disponível neste filtro"
            description="A Anthropic atribui custo medido só por sessão — sessão não tem dimensão de spec nem onda. Para ver as sessões, volte ao filtro Projeto."
          />
        ) : (
          <EmptyState
            title="Sem sessões registradas"
            description="As sessões aparecem aqui depois que o Claude Code rodar com telemetria ligada."
          />
        )}
      </section>

      {/* ── O que a ferramenta evitou de gastar (savings by source) ────── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium">O que a ferramenta evitou de gastar</h2>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            cada linha é uma estratégia que poupa tokens — a injeção de receita é estimada
          </p>
        </header>
        <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] p-2">
          <SavingsBreakdownCard breakdown={breakdown.data} />
        </div>
      </section>

      {/* ── Custo estimado por spec / onda (per-dispatch attribution) ──── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <h2 className="text-sm font-medium">Custo estimado por spec / onda</h2>
            <span className="px-2 py-0.5 rounded-full text-[10px] uppercase tracking-[0.14em] font-medium text-primary/70 bg-primary/10 border border-primary/20">
              estimado
            </span>
          </div>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            soma do custo de cada execução atribuída à spec — uma estimativa interna por dispatch,
            útil para comparar features. Não é o valor cobrado pela Anthropic.
          </p>
        </header>
        <EstimatedBySpecWave
          perSpec={perSpec.data ?? []}
          perWave={perWave.data ?? []}
          isLoading={perSpec.isLoading || perWave.isLoading}
        />
      </section>
    </div>
  );
}

// ── Helpers ────────────────────────────────────────────────────────────────

/**
 * One row of the "Por sessão" card. Layout: date · short-id · chips · cost.
 *
 * Spec chips overflow gracefully — we show the first three and collapse the
 * remainder behind a `+N` chip so a session that touched ten specs doesn't
 * blow up the row width. `last_at_ms == null` falls back to "—" rather than
 * rendering `Invalid Date`.
 */
function SessionRow({
  sessionId,
  usd,
  lastAtMs,
  specs,
}: {
  sessionId: string;
  usd: number;
  lastAtMs: number | null;
  specs: string[];
}) {
  const date = formatSessionDate(lastAtMs);
  const shortId = sessionId ? sessionId.slice(0, 8) : "—";
  const visibleSpecs = specs.slice(0, 3);
  const overflowCount = Math.max(0, specs.length - visibleSpecs.length);
  const usdText = `$${usd.toFixed(usd < 0.01 ? 4 : usd < 1 ? 3 : 2)}`;

  return (
    <div className="flex items-center gap-3 px-3 py-2 rounded-[--ds-radius-md] bg-[--ds-surface-base]">
      <span className="font-mono text-[12px] text-[--ds-text-secondary] tabular-nums w-[88px] shrink-0">
        {date}
      </span>
      <span className="font-mono text-[12px] text-[--ds-text-primary] w-[72px] shrink-0">
        {shortId}
      </span>
      <div className="flex flex-wrap items-center gap-1 min-w-0 flex-1">
        {visibleSpecs.length === 0 ? (
          <span className="text-[11px] text-[--ds-text-tertiary] italic">
            sem spec registrada
          </span>
        ) : (
          visibleSpecs.map((spec) => (
            <span
              key={`${sessionId}-spec-${spec}`}
              className="px-1.5 py-0.5 rounded text-[10.5px] font-mono text-[--ds-text-secondary] bg-[--ds-surface-hover] truncate max-w-[180px]"
              title={spec}
            >
              {spec}
            </span>
          ))
        )}
        {overflowCount > 0 && (
          <span
            className="px-1.5 py-0.5 rounded text-[10.5px] font-mono text-[--ds-text-tertiary] bg-[--ds-surface-hover]"
            title={specs.slice(visibleSpecs.length).join(", ")}
          >
            +{overflowCount}
          </span>
        )}
      </div>
      <MetricsPill value={usdText} intent={usd > 0 ? "info" : "neutral"} />
    </div>
  );
}

/**
 * Banner shown when the ESTIMATED ingestion path (`run_usage`) has fallen
 * behind the MEASURED path (`usage_totals`) by more than 6h. Renders amber
 * (warning), not red (error) — the data on screen is not wrong, it is just
 * outdated for the per-spec/per-wave estimates. The MEASURED cost stays
 * accurate independently.
 */
function IngestionStaleBanner({ hours }: { hours: number }) {
  // Round to a friendly bucket so the message reads naturally: "há 9 horas"
  // is more useful than "há 8.73 horas". For very large gaps (>48h) we tip
  // over to "há N dias" because hours stop being legible past two days.
  const label =
    hours >= 48
      ? `${Math.round(hours / 24)} dias`
      : `${Math.round(hours)} horas`;
  return (
    <div className="flex items-start gap-3 px-4 py-3 rounded-lg border border-amber-500/30 bg-amber-500/10 text-[12.5px]">
      <AlertTriangle
        className="h-4 w-4 text-amber-500 shrink-0 mt-0.5"
        strokeWidth={2}
      />
      <div className="flex flex-col gap-1 min-w-0">
        <p className="font-medium text-[--ds-text-primary]">
          A tabela de custo estimado por spec/onda parou de receber dados há {label}.
        </p>
        <p className="text-[--ds-text-secondary] leading-relaxed">
          O custo medido (do card "Custo do projeto") continua atualizado — só a
          quebra por feature está congelada. Para retomar a estimação por spec:
          verifique que o collector do <code className="font-mono text-[11px]">mustard-rt</code> está rodando
          e que o Claude Code está exportando OTEL via{" "}
          <code className="font-mono text-[11px]">OTEL_EXPORTER_OTLP_ENDPOINT</code>.
        </p>
      </div>
    </div>
  );
}

/**
 * Format an epoch-ms timestamp as `DD/MM HH:mm`. `null` (no measured row for
 * the session yet) becomes the en-dash so the column width stays stable.
 */
function formatSessionDate(ms: number | null): string {
  if (ms == null) return "—";
  return dayjs(ms).format("DD/MM HH:mm");
}

/**
 * "Custo estimado por spec / onda" — renders self-attributed `run_usage`
 * roll-ups as a tabular grid. Rules:
 *
 * - Unattributed rows (`spec_id === ""`) are excluded from the body and
 *   surfaced in a footer counter — they are noise in a per-spec comparison.
 * - Waves with no `wave_id` are excluded; they would duplicate the parent
 *   spec row visually.
 * - Costs of exactly $0 are rendered as "—" instead of "$0.00" so the row
 *   doesn't look like a measured zero when it's a missing-data zero.
 *
 * Layout: a 4-column grid (spec/wave name · dispatches · tokens · USD) with
 * tabular-nums on the numeric columns so digits align across rows.
 */
function EstimatedBySpecWave({
  perSpec,
  perWave,
  isLoading,
}: {
  perSpec: SpecCost[];
  perWave: WaveCost[];
  isLoading: boolean;
}) {
  if (isLoading) {
    return (
      <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] p-4 text-[12px] text-[--ds-text-tertiary]">
        carregando…
      </div>
    );
  }
  const namedSpecs = perSpec.filter((row) => row.spec_id);
  const unattributed = perSpec.filter((row) => !row.spec_id);
  const unattributedDispatches = unattributed.reduce((acc, r) => acc + r.span_count, 0);
  if (namedSpecs.length === 0) {
    return (
      <EmptyState
        title="Sem execuções atribuídas neste escopo"
        description="As linhas aparecem aqui assim que dispatches forem registrados com a spec correspondente."
      />
    );
  }
  // Group waves under their parent spec. We split a spec's wave rows into two
  // buckets: those with a real `wave_id` (rendered normally) and those without
  // (collapsed into a single "sem onda atribuída" footer row so the user sees
  // the attribution gap instead of silently missing dispatches).
  const wavesBySpec = new Map<string, WaveCost[]>();
  const unwavedBySpec = new Map<string, WaveCost>();
  for (const w of perWave) {
    if (!w.spec_id) continue;
    if (w.wave_id) {
      const list = wavesBySpec.get(w.spec_id) ?? [];
      list.push(w);
      wavesBySpec.set(w.spec_id, list);
    } else {
      const acc = unwavedBySpec.get(w.spec_id);
      if (acc) {
        acc.span_count += w.span_count;
        acc.tokens += w.tokens;
        acc.cost_usd_micros += w.cost_usd_micros;
      } else {
        unwavedBySpec.set(w.spec_id, { ...w });
      }
    }
  }

  return (
    <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] overflow-hidden">
      {/* Column header row */}
      <div className="grid grid-cols-[1fr_110px_110px_110px] gap-3 px-3 py-2 text-[10px] uppercase tracking-[0.14em] font-medium text-[--ds-text-tertiary] border-b border-[--ds-surface-hover]">
        <span>Spec / onda</span>
        <span className="text-right">Execuções</span>
        <span className="text-right">Tokens</span>
        <span className="text-right">Custo</span>
      </div>

      <div className="flex flex-col">
        {namedSpecs.map((row, idx) => {
          const waves = wavesBySpec.get(row.spec_id) ?? [];
          const isLast = idx === namedSpecs.length - 1;
          return (
            <div
              key={`spec-${row.spec_id}`}
              className={cn(
                !isLast && "border-b border-[--ds-surface-hover]/60",
              )}
            >
              <SpecOrWaveRow
                name={row.spec_id}
                dispatches={row.span_count}
                tokens={row.tokens}
                costMicros={row.cost_usd_micros}
              />
              {(waves.length > 0 || unwavedBySpec.has(row.spec_id)) && (
                <div className="flex flex-col">
                  {waves.map((w) => (
                    <SpecOrWaveRow
                      key={`wave-${w.spec_id}-${w.wave_id}`}
                      name={w.wave_id}
                      dispatches={w.span_count}
                      tokens={w.tokens}
                      costMicros={w.cost_usd_micros}
                      nested
                    />
                  ))}
                  {unwavedBySpec.has(row.spec_id) && (
                    <SpecOrWaveRow
                      key={`wave-${row.spec_id}-unattributed`}
                      name="(sem onda atribuída)"
                      dispatches={unwavedBySpec.get(row.spec_id)!.span_count}
                      tokens={unwavedBySpec.get(row.spec_id)!.tokens}
                      costMicros={unwavedBySpec.get(row.spec_id)!.cost_usd_micros}
                      nested
                      muted
                    />
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {unattributedDispatches > 0 && (
        <div className="px-3 py-2 border-t border-[--ds-surface-hover]/60 bg-[--ds-surface-hover]/20 text-[10.5px] text-[--ds-text-tertiary] flex items-center gap-1.5">
          <Info className="h-3 w-3 text-[--ds-text-tertiary] shrink-0" strokeWidth={2} />
          <span>
            {unattributedDispatches === 1
              ? "1 execução sem spec registrada"
              : `${unattributedDispatches.toLocaleString()} execuções sem spec registrada`}
            {" "}— não aparecem na tabela porque não dá pra atribuir a uma feature específica.
          </span>
        </div>
      )}
    </div>
  );
}

/**
 * One row of the spec/wave estimate table. Same column shape for parent and
 * nested rows; `nested` only changes typography weight, indent, and the
 * leading arrow glyph. Costs of exactly zero render as "—" so the user sees
 * "sem dado" instead of a misleading "$0.00".
 */
function SpecOrWaveRow({
  name,
  dispatches,
  tokens,
  costMicros,
  nested = false,
  muted = false,
}: {
  name: string;
  dispatches: number;
  tokens: number;
  costMicros: number;
  nested?: boolean;
  /** Render the row as a "missing attribution" entry: italic + dimmer. */
  muted?: boolean;
}) {
  return (
    <div
      className={cn(
        "grid grid-cols-[1fr_110px_110px_110px] gap-3 items-center px-3 py-2 transition-colors hover:bg-[--ds-surface-hover]/30",
        nested && "pl-7 bg-[--ds-surface-hover]/10",
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        {nested && (
          <span className="text-[--ds-text-tertiary] text-[12px] shrink-0">↳</span>
        )}
        <span
          className={cn(
            "truncate",
            muted ? "italic font-normal" : "font-mono",
            nested
              ? "text-[11.5px] text-[--ds-text-secondary]"
              : "text-[12.5px] text-[--ds-text-primary]",
            muted && "text-[--ds-text-tertiary]",
          )}
          title={name}
        >
          {name}
        </span>
      </div>
      <span
        className={cn(
          "font-mono tabular-nums text-right",
          nested
            ? "text-[11px] text-[--ds-text-tertiary]"
            : "text-[12px] text-[--ds-text-secondary]",
        )}
      >
        {dispatches.toLocaleString()}
      </span>
      <span
        className={cn(
          "font-mono tabular-nums text-right",
          nested
            ? "text-[11px] text-[--ds-text-tertiary]"
            : "text-[12px] text-[--ds-text-secondary]",
        )}
      >
        {formatTokens(tokens)}
      </span>
      <span
        className={cn(
          "font-mono tabular-nums text-right",
          costMicros > 0
            ? nested
              ? "text-[11.5px] text-[--ds-text-secondary]"
              : "text-[12.5px] text-[--ds-text-primary]"
            : "text-[11.5px] text-[--ds-text-tertiary]",
        )}
      >
        {costMicros > 0 ? formatUsd(costMicros) : "—"}
      </span>
    </div>
  );
}

/**
 * Plain-language tier for the cache-hit percentage. Matches the accent on the
 * KPICard so the colour and the word reinforce each other.
 *
 * - >= 80% — cache is reusing most of the input. The Anthropic billing for
 *   cache reads is 0.10x the base input price, so anything in this band is a
 *   meaningful cost reduction.
 * - 50-79% — partial reuse. Often means the stable prefix is drifting.
 * - < 50% — little reuse; the prefix is either small or churning.
 */
function cacheHitTier(percent: number): string {
  if (percent >= 80) return "ótimo · cache funcionando";
  if (percent >= 50) return "morno · prefixo mudando";
  if (percent > 0) return "frio · pouco reuso";
  return "sem reuso medido";
}

/**
 * Map the unified collector-health state to a PT label + status-dot variant.
 * `undefined` (still loading) reads as "desligado" so the badge never claims
 * the data is live before we know.
 */
function collectorBadge(health: CollectorHealth | undefined): {
  badgeLabel: string;
  badgeVariant: StatusDotVariant;
} {
  switch (health) {
    case "live":
      return { badgeLabel: "ao vivo", badgeVariant: "active" };
    case "stale":
      return { badgeLabel: "parado", badgeVariant: "blocked" };
    case "off":
    default:
      return { badgeLabel: "desligado", badgeVariant: "idle" };
  }
}

function scopeProjectKey(scope: EconomyScope): string {
  switch (scope.kind) {
    case "project":
    case "spec":
    case "wave":
      return scope.project;
    case "all_projects":
      return scope.projects[0] ?? "";
  }
}

function scopeKey(scope: EconomyScope): string {
  switch (scope.kind) {
    case "project":
      return `p:${scope.project}`;
    case "spec":
      return `s:${scope.project}|${scope.spec}`;
    case "wave":
      return `w:${scope.project}|${scope.spec}|${scope.wave}`;
    case "all_projects":
      return `a:${[...scope.projects].sort().join(",")}`;
  }
}
