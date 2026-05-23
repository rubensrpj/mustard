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
import { useStore } from "@/lib/store";
import { EmptyState, KPICard } from "@/components/page";
import { MetricsPill } from "@/components/ds";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { relativeTime } from "@/lib/time";
import { useProjects } from "@/lib/dashboard";
import {
  fetchEconomySavingsBreakdown,
  fetchEconomyContextRouting,
} from "@/lib/dashboard";
import { useEconomySummary } from "@/hooks/useEconomySummary";
import { useCollectorHealth } from "@/hooks/usePromptEconomy";
import type { CollectorHealth } from "@/api/promptEconomy";
import { ScopeBar } from "@/components/economy/ScopeBar";
import { PerAgentTable } from "@/components/economy/PerAgentTable";
import { SavingsBreakdownCard } from "@/components/economy/SavingsBreakdownCard";
import type { EconomyScope } from "@/lib/types/economy";
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

  // ── Distribuição por agente (light, horizontal-bar style w/o chart lib) ─
  // We render the top agents as proportional bars sized by `tokens`. No
  // recharts/d3 dependency — pure flex + Tailwind widths.
  const topAgents = data?.top_agents_by_cost ?? [];
  const tokensMax = topAgents.reduce((acc, a) => Math.max(acc, a.tokens), 0);

  return (
    <div className="flex flex-col gap-6 w-full">
      <ScopeBar projectPath={repoPath} scope={scope} onScopeChange={setScope} />

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
          caption="tokens servidos do cache ÷ (cache + escrita no cache + input novo). Acima de 80% é ótimo — a Anthropic cobra só 10% do preço normal nesses tokens."
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

      {/* ── Custo estimado por spec / onda (empty-state — Em breve) ───── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium">Custo estimado por spec / onda</h2>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            estimativa por execução — útil para comparar features
          </p>
        </header>
        <EmptyState
          title="Em breve"
          description="A quebra estimada por spec e onda aparecerá aqui assim que executarmos pipelines com receitas casadas."
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
 * Format an epoch-ms timestamp as `DD/MM HH:mm`. `null` (no measured row for
 * the session yet) becomes the en-dash so the column width stays stable.
 */
function formatSessionDate(ms: number | null): string {
  if (ms == null) return "—";
  return dayjs(ms).format("DD/MM HH:mm");
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
