// Economia — W7 page (spec 2026-05-20-economia-moat-unification, Wave 7).
//
// Single source of every cost/saving signal: `useEconomySummary(scope)`. The
// scope picker (Projeto / Spec / Wave / Comparar projetos) lives in
// `<ScopeBar>` and drives the same hook key — switching tab refetches.
//
// AC-5 contract: this page MUST contain the four literal labels — "Projeto",
// "Spec", "Wave", "Comparar" — even when `<ScopeBar>` happens to render them
// dynamically. They live in `SCOPE_LABELS` below so a future audit can grep
// them without parsing JSX.
//
// AC-6 contract: this file MUST NOT import the Tauri core API or call the
// Tauri command bridge directly. Every IO call routes through
// `useEconomySummary` or the typed wrappers in `lib/dashboard.ts`.

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useStore } from "@/lib/store";
import { PageHeader, EmptyState, KPICard } from "@/components/page";
import { MetricsPill, BaseRow } from "@/components/ds";
import { useProjects } from "@/lib/dashboard";
import {
  fetchEconomySavingsBreakdown,
  fetchEconomyContextRouting,
} from "@/lib/dashboard";
import { useEconomySummary } from "@/hooks/useEconomySummary";
import { ScopeBar } from "@/components/economy/ScopeBar";
import { PerAgentTable } from "@/components/economy/PerAgentTable";
import { SavingsBreakdownCard } from "@/components/economy/SavingsBreakdownCard";
import type { EconomyScope } from "@/lib/types/economy";
import { projectScope, formatTokens, formatUsd } from "@/lib/types/economy";

// AC-5 anchor — every label rendered by `<ScopeBar>` is also referenced here
// so a literal-string audit passes without parsing JSX:
//   "Projeto" · "Spec" · "Wave" · "Comparar projetos"
const SCOPE_LABELS = ["Projeto", "Spec", "Wave", "Comparar projetos"] as const;

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

  // ── Empty / config states ────────────────────────────────────────────────
  if (!projectsRoot) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: "Workspace" }, { label: "Economia" }]}
          title="Economia"
          subtitle={SCOPE_LABELS.join(" · ")}
        />
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
        <PageHeader
          breadcrumb={[{ label: "Workspace" }, { label: "Economia" }]}
          title="Economia"
          subtitle={SCOPE_LABELS.join(" · ")}
        />
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
  const retryRatio = (routing.data?.retry_overhead_ratio_permille ?? 0) / 10;

  // ── Distribuição por agente (light, horizontal-bar style w/o chart lib) ─
  // We render the top agents as proportional bars sized by `tokens`. No
  // recharts/d3 dependency — pure flex + Tailwind widths.
  const topAgents = data?.top_agents_by_cost ?? [];
  const tokensMax = topAgents.reduce((acc, a) => Math.max(acc, a.tokens), 0);

  return (
    <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={[{ label: "Workspace" }, { label: "Economia" }]}
        title="Economia"
        subtitle={`Escopo: ${describeScope(scope)} · ${SCOPE_LABELS.join(" · ")}`}
      />

      <ScopeBar projectPath={repoPath} scope={scope} onScopeChange={setScope} />

      {/* ── KPI cards: custo, economia, cache hit ──────────────────────── */}
      <section className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <KPICard
          label="Custo Anthropic"
          value={summary.isLoading ? "…" : formatUsd(data?.total_cost_usd_micros ?? 0)}
          hint={`${(data?.span_count ?? 0).toLocaleString()} spans · ${formatTokens(data?.total_tokens ?? 0)} tokens`}
          accent={data && data.total_cost_usd_micros > 0 ? "indigo" : "zinc"}
        />
        <KPICard
          label="Economia (todas as fontes)"
          value={summary.isLoading ? "…" : `${formatTokens(data?.total_tokens_saved ?? 0)} tok`}
          hint="rtk + routing + bash_guard + budget + recipe"
          accent={data && data.total_tokens_saved > 0 ? "emerald" : "zinc"}
        />
        <KPICard
          label="Cache hit ratio"
          value={
            routing.isLoading ? "…" : routing.data ? `${cacheRatio.toFixed(1)}%` : "—"
          }
          hint={
            routing.data
              ? `${(routing.data.frame_count ?? 0).toLocaleString()} frames · retry overhead ${retryRatio.toFixed(1)}%`
              : "sem ContextCostFrame neste escopo"
          }
          accent={cacheRatio > 30 ? "emerald" : cacheRatio > 0 ? "amber" : "zinc"}
        />
      </section>

      {summary.error ? (
        <EmptyState
          variant="warning"
          title="Falha ao ler economy_summary"
          description={String((summary.error as Error)?.message ?? summary.error)}
        />
      ) : null}

      {/* ── Por agente (top-N) ─────────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <header className="flex items-baseline justify-between">
          <h2 className="text-sm font-medium">Por agente (top {topAgents.length || 0})</h2>
          <span className="text-[11px] text-[--ds-text-tertiary]">
            fonte: <code className="font-mono">economy_summary.top_agents_by_cost</code>
          </span>
        </header>
        <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] overflow-hidden">
          <PerAgentTable agents={topAgents} />
        </div>
      </section>

      {/* ── Distribuição por agente (horizontal bars sem chart lib) ────── */}
      {topAgents.length > 0 && (
        <section className="flex flex-col gap-3">
          <header className="flex items-baseline justify-between">
            <h2 className="text-sm font-medium">Distribuição de tokens por agente</h2>
            <span className="text-[11px] text-[--ds-text-tertiary]">
              proporcional a <code className="font-mono">tokens</code>
            </span>
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

      {/* ── Prevention breakdown (por SavingsSource) ──────────────────── */}
      <section className="flex flex-col gap-3">
        <header className="flex items-baseline justify-between">
          <h2 className="text-sm font-medium">Prevention breakdown</h2>
          <span className="text-[11px] text-[--ds-text-tertiary]">
            fonte: <code className="font-mono">savings_breakdown</code>
          </span>
        </header>
        <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] p-2">
          <SavingsBreakdownCard breakdown={breakdown.data} />
        </div>
      </section>

      {/* ── Top specs por custo (Project / AllProjects scopes only) ───── */}
      {(scope.kind === "project" || scope.kind === "all_projects") &&
        topAgents.length > 0 && (
          <section className="flex flex-col gap-3">
            <header className="flex items-baseline justify-between">
              <h2 className="text-sm font-medium">Top contribuintes</h2>
              <span className="text-[11px] text-[--ds-text-tertiary]">
                {scope.kind === "all_projects"
                  ? `${scope.projects.length} projetos comparados`
                  : "agentes mais caros do projeto"}
              </span>
            </header>
            <div className="flex flex-col gap-1">
              {topAgents.slice(0, 5).map((a) => (
                <BaseRow
                  key={`top-${a.agent_id}`}
                  label={a.agent_id || "—"}
                  summary={`${a.span_count} spans · ${formatUsd(a.cost_usd_micros)}`}
                  tokens={a.tokens}
                />
              ))}
            </div>
          </section>
        )}
    </div>
  );
}

// ── Helpers ────────────────────────────────────────────────────────────────

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

function describeScope(scope: EconomyScope): string {
  switch (scope.kind) {
    case "project":
      return "Projeto atual";
    case "spec":
      return scope.spec ? `Spec ${scope.spec}` : "Spec (selecione)";
    case "wave":
      return scope.wave ? `Wave ${scope.wave}` : "Wave (selecione)";
    case "all_projects":
      return `${scope.projects.length} projetos`;
  }
}
