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
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import dayjs from "dayjs";
import { AlertTriangle, Info } from "lucide-react";
import { cn } from "@/lib/utils";
import { useStore } from "@/lib/store";
import {
  EmptyState,
  KPICard,
  StatPill,
  PageSurface,
  EditorialBand,
} from "@/components/page";
import { StatusDot, type StatusDotVariant } from "@/components/page/StatusDot";
import { relativeTime } from "@/lib/time";
import { useProjects } from "@/lib/dashboard";
import {
  fetchEconomySavingsBreakdown,
  fetchEconomyContextRouting,
  fetchEconomyPerSpecCosts,
  fetchEconomyPerWaveCosts,
  fetchConsumption,
} from "@/lib/dashboard";
import type { ConsumptionSummary } from "@/lib/dashboard";
import { useEconomySummary } from "@/hooks/useEconomySummary";
import { useCollectorHealth } from "@/hooks/usePromptEconomy";
import type { CollectorHealth } from "@/api/promptEconomy";
import { ScopeBar } from "@/features/economy/ScopeBar";
import { PerAgentTable } from "@/features/economy/PerAgentTable";
import { SavingsBreakdownCard } from "@/features/economy/SavingsBreakdownCard";
import type { EconomyScope, SpecCost, WaveCost } from "@/lib/types/economy";
import { projectScope, formatTokens, formatUsd } from "@/lib/types/economy";


export function Economia() {
  const { t } = useTranslation();
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
  // staleTime aligned with the global 60s default — the 30s refetchInterval
  // stays as the live fallback, so route switches within the window render
  // from cache instead of refiring all economy folds.
  const breakdown = useQuery({
    queryKey: ["economy-savings", scope && scopeKey(scope)],
    queryFn: () => fetchEconomySavingsBreakdown(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 60_000,
    refetchInterval: 30_000,
  });

  const routing = useQuery({
    queryKey: ["economy-routing", scope && scopeKey(scope)],
    queryFn: () => fetchEconomyContextRouting(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 60_000,
    refetchInterval: 30_000,
  });

  const perSpec = useQuery({
    queryKey: ["economy-per-spec", scope && scopeKey(scope)],
    queryFn: () => fetchEconomyPerSpecCosts(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 60_000,
    refetchInterval: 30_000,
  });

  const perWave = useQuery({
    queryKey: ["economy-per-wave", scope && scopeKey(scope)],
    queryFn: () => fetchEconomyPerWaveCosts(scope as EconomyScope),
    enabled: !!scope,
    staleTime: 60_000,
    refetchInterval: 30_000,
  });

  // Reality block — the MEASURED total + per-model + per-spec consumption from
  // the single source of truth (`dashboard_consumption` → ConsumptionSummary,
  // built on `mustard_core::domain::economy`). Repo-scoped, not economy-scope:
  // it answers "what did this project really cost", independent of the
  // scope-bar filter that drives the cards below.
  const consumption = useQuery<ConsumptionSummary>({
    queryKey: ["economy-consumption", repoPath],
    queryFn: () => fetchConsumption(repoPath as string),
    enabled: !!repoPath,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });

  // Collector-health badge — tells the user the cost number is CURRENT, not a
  // ghost from a crashed collector. Same hook every other economy page uses.
  const collectorHealth = useCollectorHealth(repoPath);

  // ── Empty / config states ────────────────────────────────────────────────
  if (!projectsRoot) {
    return (
      <PageSurface>
        <EmptyState
          title={t("economy.empty.noRoot.title")}
          description={t("economy.empty.noRoot.description")}
        />
      </PageSurface>
    );
  }

  if (!activeWorkspaceId || !repoPath || !scope) {
    return (
      <PageSurface>
        <EmptyState
          title={t("economy.empty.noWorkspace.title")}
          description={t("economy.empty.noWorkspace.description")}
        />
      </PageSurface>
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
  const { badgeLabel, badgeVariant } = collectorBadge(health, t);
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

  // Top agents drive the "Por agente" / "By agent" section. The horizontal-bar
  // distribution chart that previously lived below was removed in the
  // 2026-05-23-economia-i18n-migration sub-spec — `PerAgentTable` already shows
  // tokens per agent, so the standalone chart was visual duplication.
  const topAgents = data?.top_agents_by_cost ?? [];

  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Economia"
        title={t("economy.kpi.cost.label")}
        subtitle={t("economy.byAgent.caption")}
      />
      <ScopeBar projectPath={repoPath} scope={scope} onScopeChange={setScope} />

      {showStaleBanner && ingestionStaleHours != null && (
        <IngestionStaleBanner hours={ingestionStaleHours} />
      )}

      {/* ── Reality: measured total + per-model + per-spec ───────────── */}
      <RealConsumption
        data={consumption.data}
        isLoading={consumption.isLoading}
      />

      {/* ── KPI cards: cost, savings, cache hit ──────────────────────── */}
      <section className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <KPICard
          label={t("economy.kpi.cost.label")}
          value={summary.isLoading ? "…" : formatUsd(data?.total_cost_usd_micros ?? 0)}
          hint={t("economy.kpi.cost.hint", {
            dispatches: (data?.span_count ?? 0).toLocaleString(),
            tokens: formatTokens(data?.total_tokens ?? 0),
          })}
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
                {updatedAgo ? (
                  <span>· {t("economy.kpi.cost.updatedAgo", { ago: updatedAgo })}</span>
                ) : null}
              </div>
              <span>{t("economy.kpi.cost.caption")}</span>
            </div>
          }
        />
        <KPICard
          label={t("economy.kpi.savings.label")}
          value={summary.isLoading ? "…" : `${formatTokens(data?.total_tokens_saved ?? 0)} tok`}
          hint={t("economy.kpi.savings.hint")}
          accent={data && data.total_tokens_saved > 0 ? "emerald" : "zinc"}
          caption={t("economy.kpi.savings.caption")}
        />
        <KPICard
          label={t("economy.kpi.cache.label")}
          value={
            routing.isLoading ? "…" : routing.data ? `${cacheRatio.toFixed(1)}%` : "—"
          }
          hint={routing.data ? cacheHitTier(cacheRatio, t) : t("economy.kpi.cache.noData")}
          accent={cacheRatio >= 80 ? "emerald" : cacheRatio >= 50 ? "amber" : "zinc"}
          caption={
            <div className="flex flex-col gap-1">
              <span>{t("economy.kpi.cache.caption")}</span>
              {scope.kind === "wave" && (
                <span className="text-[--intent-warning]/80">
                  {t("economy.kpi.cache.collapseWave")}
                </span>
              )}
            </div>
          }
        />
      </section>

      {summary.error ? (
        <EmptyState
          variant="warning"
          title={t("economy.summaryError.title")}
          description={String((summary.error as Error)?.message ?? summary.error)}
        />
      ) : null}

      {/* ── By agent (top-N) ───────────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium">
            {topAgents.length > 0
              ? t("economy.byAgent.title", { count: topAgents.length })
              : t("economy.byAgent.titleFallback")}
          </h2>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            {t("economy.byAgent.caption")}
          </p>
        </header>
        <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] overflow-hidden">
          <PerAgentTable
            agents={topAgents}
            measuredCostMicros={data?.total_cost_usd_micros ?? null}
          />
        </div>
      </section>

      {/* ── By session (measured cost per Claude Code session) ────────── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium">{t("economy.bySession.title")}</h2>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            {t("economy.bySession.captionBefore")}
            <code className="font-mono">/cost</code>
            {t("economy.bySession.captionAfter")}
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
                noSpecLabel={t("economy.bySession.noSpecChip")}
              />
            ))}
          </div>
        ) : scope.kind === "spec" || scope.kind === "wave" ? (
          <EmptyState
            title={t("economy.bySession.unavailable.title")}
            description={t("economy.bySession.unavailable.description")}
          />
        ) : (
          <EmptyState
            title={t("economy.bySession.empty.title")}
            description={t("economy.bySession.empty.description")}
          />
        )}
      </section>

      {/* ── Savings by source ──────────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium">{t("economy.savings.title")}</h2>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            {t("economy.savings.caption")}
          </p>
        </header>
        <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] p-2">
          <SavingsBreakdownCard breakdown={breakdown.data} />
        </div>
      </section>

      {/* ── Estimated per spec / wave (per-dispatch attribution) ──────── */}
      <section className="flex flex-col gap-3">
        <header className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <h2 className="text-sm font-medium">{t("economy.estimated.title")}</h2>
            <span className="px-2 py-0.5 rounded-full text-[10px] uppercase tracking-[0.14em] font-medium text-[--primary]/70 bg-[--primary]/10 border border-[--primary]/20">
              {t("economy.estimated.badge")}
            </span>
          </div>
          <p className="text-[11px] text-[--ds-text-tertiary]">
            {t("economy.estimated.caption")}
          </p>
        </header>
        <EstimatedBySpecWave
          perSpec={perSpec.data ?? []}
          perWave={perWave.data ?? []}
          isLoading={perSpec.isLoading || perWave.isLoading}
        />
      </section>
    </PageSurface>
  );
}

// ── Helpers ────────────────────────────────────────────────────────────────

/**
 * One row of the "Por sessão" card. Layout: date · spec chips · cost.
 *
 * The dedicated short-id column was removed (creative redesign feedback): the
 * eight-char hash had no semantic load on its own — useful only as a search
 * handle. We preserve it as the tooltip on the date span so power users can
 * still see/copy it on hover. Spec chips now own all the freed horizontal
 * space and only truncate when the spec slug actually exceeds the wider band
 * (280px). `last_at_ms == null` falls back to "—" rather than `Invalid Date`.
 *
 * Empty state uses a neutral chip ("sem spec" / "no spec") instead of italic
 * grey text — italic mid-row reads like an apology; the chip reads like a
 * structured value.
 */
function SessionRow({
  sessionId,
  usd,
  lastAtMs,
  specs,
  noSpecLabel,
}: {
  sessionId: string;
  usd: number;
  lastAtMs: number | null;
  specs: string[];
  noSpecLabel: string;
}) {
  const date = formatSessionDate(lastAtMs);
  // Truncate the session id for the date tooltip only — the dedicated column
  // was removed (creative redesign feedback). 8 chars stays enough to copy
  // for cross-reference with /cost output without owning visible real estate.
  const shortId = sessionId ? sessionId.substring(0, 8) : "";
  const visibleSpecs = specs.slice(0, 3);
  const overflowCount = Math.max(0, specs.length - visibleSpecs.length);
  const usdText = `$${usd.toFixed(usd < 0.01 ? 4 : usd < 1 ? 3 : 2)}`;
  const dateTooltip = shortId ? `${date} · session ${shortId}` : date;

  return (
    <div className="grid grid-cols-[88px_1fr_auto] items-center gap-3 px-3 py-2 rounded-[--ds-radius-md] bg-[--ds-surface-base] hover:bg-[--ds-surface-hover]/25 transition-colors">
      <span
        className="font-mono text-[12px] text-[--ds-text-secondary] tabular-nums shrink-0"
        title={dateTooltip}
      >
        {date}
      </span>
      <div className="flex flex-wrap items-center gap-1 min-w-0">
        {visibleSpecs.length === 0 ? (
          <span className="px-1.5 py-0.5 rounded text-[10.5px] text-[--ds-text-tertiary] bg-[--ds-surface-hover]/60">
            {noSpecLabel}
          </span>
        ) : (
          visibleSpecs.map((spec) => (
            <span
              key={`${sessionId}-spec-${spec}`}
              className="px-1.5 py-0.5 rounded text-[10.5px] font-mono text-[--ds-text-secondary] bg-[--ds-surface-hover] truncate max-w-[280px]"
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
      <StatPill value={usdText} intent={usd > 0 ? "info" : "neutral"} />
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
  // Round to a friendly bucket so the message reads naturally — "há 9 horas"
  // beats "há 8.73 horas". For very large gaps (>48h) tip over to "N dias"
  // because hours stop being legible past two days. Plural forms live in the
  // i18n bundle under `economy.staleBanner.label_{hours,days}_{one,other}`.
  const { t } = useTranslation();
  const isDays = hours >= 48;
  const count = isDays ? Math.round(hours / 24) : Math.round(hours);
  const label = isDays
    ? t("economy.staleBanner.label_days", { count })
    : t("economy.staleBanner.label_hours", { count });
  return (
    <div className="flex items-start gap-3 px-4 py-3 rounded-lg border border-[--intent-warning]/30 bg-[--intent-warning]/10 text-[12.5px]">
      <AlertTriangle
        className="h-4 w-4 text-[--intent-warning] shrink-0 mt-0.5"
        strokeWidth={2}
      />
      <div className="flex flex-col gap-1 min-w-0">
        <p className="font-medium text-[--ds-text-primary]">
          {t("economy.staleBanner.title", { label })}
        </p>
        <p className="text-[--ds-text-secondary] leading-relaxed">
          {t("economy.staleBanner.bodyBefore")}
          <code className="font-mono text-[11px]">mustard-rt</code>
          {t("economy.staleBanner.bodyBetween")}
          <code className="font-mono text-[11px]">OTEL_EXPORTER_OTLP_ENDPOINT</code>
          {t("economy.staleBanner.bodyAfter")}
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
 * - When a spec has wave rows without a `wave_id`, we no longer render a
 *   muted sub-row apologizing for missing attribution — that pattern bloated
 *   every spec by an extra line and the user couldn't tell signal from noise.
 *   Instead, we surface an inline amber "sem onda" badge on the parent row;
 *   the parent's totals already include the unwaved dispatches/tokens/cost.
 * - Sub-rows render ONLY when the spec has at least one named wave. A spec
 *   whose entire spend is in the unwaved bucket stays as a single row with a
 *   badge — no fake hierarchy.
 * - Ordering: `last_started_at` descending when the wire field is populated
 *   (parallel backend rollout), falling back to reverse-lexical `spec_id`
 *   (Mustard slugs are `YYYY-MM-DD-*`, so reverse alpha ≈ chronological).
 * - Costs of exactly $0 still render as "—"; for nonzero-but-tiny costs the
 *   `formatUsd` band now extends to six decimals so cache-heavy traffic
 *   doesn't read as "no data".
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
  const { t } = useTranslation();
  if (isLoading) {
    return (
      <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] p-4 text-[12px] text-[--ds-text-tertiary]">
        {t("economy.estimated.loading")}
      </div>
    );
  }
  const unattributed = perSpec.filter((row) => !row.spec_id);
  const unattributedDispatches = unattributed.reduce((acc, r) => acc + r.span_count, 0);
  // Defensive sort: backend will eventually deliver rows pre-sorted by
  // last_started_at desc, but the field is still rolling out. We sort here so
  // the UI is correct regardless of wire order.
  const namedSpecs = perSpec
    .filter((row) => row.spec_id)
    .slice()
    .sort((a, b) => {
      const aTs = a.last_started_at ?? null;
      const bTs = b.last_started_at ?? null;
      if (aTs != null && bTs != null) return bTs - aTs;
      if (aTs != null) return -1;
      if (bTs != null) return 1;
      // Fallback: reverse lexical on spec_id; Mustard slugs prefix YYYY-MM-DD
      // so reverse alpha is chronological-enough until the wire field lands.
      return b.spec_id.localeCompare(a.spec_id);
    });
  if (namedSpecs.length === 0) {
    return (
      <EmptyState
        title={t("economy.estimated.empty.title")}
        description={t("economy.estimated.empty.description")}
      />
    );
  }
  // Group waves under their parent spec. Real waves render as sub-rows;
  // unwaved rows are collapsed into a single aggregate per spec and surfaced
  // as a parent-row badge (no muted "ghost" sub-row).
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
        <span>{t("economy.estimated.col.specWave")}</span>
        <span className="text-right">{t("economy.estimated.col.dispatches")}</span>
        <span className="text-right">{t("economy.estimated.col.tokens")}</span>
        <span className="text-right">{t("economy.estimated.col.cost")}</span>
      </div>

      <div className="flex flex-col">
        {namedSpecs.map((row, idx) => {
          const waves = wavesBySpec.get(row.spec_id) ?? [];
          const hasUnwaved = unwavedBySpec.has(row.spec_id);
          // Only render sub-rows when there's a real wave to render. A spec
          // whose entire spend is unwaved stays as one row with a badge —
          // synthetic hierarchy adds noise without adding signal.
          const showSubRows = waves.length > 0;
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
                badge={hasUnwaved ? t("economy.estimated.noWaveBadge") : null}
              />
              {showSubRows && (
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
            {t("economy.estimated.unattributed", {
              count: unattributedDispatches,
            })}
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
 * "sem dado" instead of a misleading "$0.00"; nonzero-tiny costs go through
 * `formatUsd` and get the 6-decimal band so cache-heavy traffic stays visible.
 *
 * `badge` (optional) renders an inline amber pill after the row name — used
 * by the parent row to flag "this spec has unwaved dispatches mixed in".
 * Lives next to the name so the eye reads `<spec> [badge]` as one unit.
 */
function SpecOrWaveRow({
  name,
  dispatches,
  tokens,
  costMicros,
  nested = false,
  badge = null,
}: {
  name: string;
  dispatches: number;
  tokens: number;
  costMicros: number;
  nested?: boolean;
  /** Inline pill rendered after the name. `null` = no badge. */
  badge?: string | null;
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
            "truncate font-mono",
            nested
              ? "text-[11.5px] text-[--ds-text-secondary]"
              : "text-[12.5px] text-[--ds-text-primary]",
          )}
          title={name}
        >
          {name}
        </span>
        {badge && (
          <span className="shrink-0 px-1.5 py-0.5 rounded-full text-[9.5px] uppercase tracking-[0.12em] font-medium border bg-[--intent-warning]/10 border-[--intent-warning]/30 text-[--intent-warning]">
            {badge}
          </span>
        )}
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
function cacheHitTier(percent: number, t: TFunction): string {
  if (percent >= 80) return t("economy.kpi.cache.tier.optimal");
  if (percent >= 50) return t("economy.kpi.cache.tier.warm");
  if (percent > 0) return t("economy.kpi.cache.tier.cold");
  return t("economy.kpi.cache.tier.empty");
}

/**
 * Map the unified collector-health state to a PT label + status-dot variant.
 * `undefined` (still loading) reads as "desligado" so the badge never claims
 * the data is live before we know.
 */
function collectorBadge(
  health: CollectorHealth | undefined,
  t: TFunction,
): {
  badgeLabel: string;
  badgeVariant: StatusDotVariant;
} {
  switch (health) {
    case "live":
      return { badgeLabel: t("economy.kpi.cost.statusLive"), badgeVariant: "active" };
    case "stale":
      return { badgeLabel: t("economy.kpi.cost.statusStale"), badgeVariant: "blocked" };
    case "off":
    default:
      return { badgeLabel: t("economy.kpi.cost.statusOff"), badgeVariant: "idle" };
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

// ── Reality: measured consumption (total + per-model + per-spec) ─────────────
//
// Backed by the Tauri command `dashboard_consumption` →
// `mustard_core::domain::economy` (economy_summary + metric_token_summary +
// per_spec_costs) — the single source of truth. Every value here is a REAL
// token count or REAL billed USD, never a duration-ms proxy. Fail-open: a
// project with no telemetry yields an all-zero summary, which we render as an
// honest empty state rather than fake zeros.
//
// `ConsumptionSummary.cost_*` are floats in DOLLARS (not micro-USD), so this
// panel formats USD directly and must NOT route them through `formatUsd`
// (which expects micro-USD).

/** Format a USD float (dollars) for the consumption panel. */
function consUsd(usd: number): string {
  if (!Number.isFinite(usd) || usd <= 0) return "$0.00";
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  if (usd < 1) return `$${usd.toFixed(3)}`;
  return `$${usd.toFixed(2)}`;
}

/**
 * Render the reality block: total tokens + total cost cards, a per-model split,
 * and the per-spec (`top_specs`) breakdown — all from the measured economy
 * source of truth. When there is no measured consumption yet, an honest empty
 * state replaces the tables so the user never reads a fabricated zero.
 */
function RealConsumption({
  data,
  isLoading,
}: {
  data: ConsumptionSummary | undefined;
  isLoading: boolean;
}) {
  const { t } = useTranslation();
  if (isLoading) {
    return (
      <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] p-4 text-[12px] text-[--ds-text-tertiary]">
        {t("economy.estimated.loading")}
      </div>
    );
  }
  const tokensTotal = data?.tokens_total ?? 0;
  const costTotal = data?.cost_total_usd ?? 0;
  const byModel = data?.by_model ?? [];
  const topSpecs = data?.top_specs ?? [];
  const hasData = tokensTotal > 0 || costTotal > 0;

  return (
    <section className="flex flex-col gap-3">
      <header className="flex flex-col gap-1">
        <div className="flex items-center gap-2">
          <h2 className="text-sm font-medium">Consumo real do projeto</h2>
          <span className="px-2 py-0.5 rounded-full text-[10px] uppercase tracking-[0.14em] font-medium text-[--intent-success]/80 bg-[--intent-success]/10 border border-[--intent-success]/20">
            medido
          </span>
        </div>
        <p className="text-[11px] text-[--ds-text-tertiary]">
          Tokens e custo reais (canal OTEL claude_code.token.usage), por modelo e por spec.
        </p>
      </header>

      {!hasData ? (
        <EmptyState
          title="Sem consumo medido ainda"
          description="Nenhuma telemetria de tokens foi registrada para este projeto. Os números aparecem assim que o coletor OTEL ingerir o primeiro uso."
        />
      ) : (
        <div className="flex flex-col gap-3">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            <KPICard
              label="Tokens totais (medidos)"
              value={`${formatTokens(tokensTotal)} tok`}
              hint={`${byModel.length} modelo${byModel.length === 1 ? "" : "s"}`}
              accent={tokensTotal > 0 ? "indigo" : "zinc"}
              caption="Soma real de tokens de entrada + saída no projeto."
            />
            <KPICard
              label="Custo total (medido)"
              value={consUsd(costTotal)}
              hint={`hoje: ${consUsd(data?.cost_today_usd ?? 0)}`}
              accent={costTotal > 0 ? "indigo" : "zinc"}
              caption="Custo real acumulado, cobrado pela Anthropic."
            />
          </div>

          {/* Per-model split */}
          <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] overflow-hidden">
            <div className="grid grid-cols-[1fr_110px_90px_110px] gap-3 px-3 py-2 text-[10px] uppercase tracking-[0.14em] font-medium text-[--ds-text-tertiary] border-b border-[--ds-surface-hover]">
              <span>Modelo</span>
              <span className="text-right">Tokens</span>
              <span className="text-right">Share</span>
              <span className="text-right">Custo</span>
            </div>
            {byModel.length === 0 ? (
              <div className="px-3 py-4 text-[12px] text-[--ds-text-tertiary]">
                Sem detalhamento por modelo no canal de métricas ainda.
              </div>
            ) : (
              <div className="flex flex-col">
                {byModel.map((m, idx) => (
                  <div
                    key={`model-${m.model}`}
                    className={cn(
                      "grid grid-cols-[1fr_110px_90px_110px] gap-3 items-center px-3 py-2",
                      idx !== byModel.length - 1 &&
                        "border-b border-[--ds-surface-hover]/60",
                    )}
                  >
                    <span
                      className="truncate font-mono text-[12.5px] text-[--ds-text-primary]"
                      title={m.model}
                    >
                      {m.model}
                    </span>
                    <span className="font-mono tabular-nums text-right text-[12px] text-[--ds-text-secondary]">
                      {formatTokens(m.total_tokens)}
                    </span>
                    <span className="font-mono tabular-nums text-right text-[11.5px] text-[--ds-text-tertiary]">
                      {(m.pct_tokens * 100).toFixed(0)}%
                    </span>
                    <span className="font-mono tabular-nums text-right text-[12.5px] text-[--ds-text-primary]">
                      {consUsd(m.cost_usd)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Per-spec breakdown (top_specs) */}
          <div className="rounded-[--ds-radius-md] border border-[--ds-surface-hover] bg-[--ds-surface-base] overflow-hidden">
            <div className="grid grid-cols-[1fr_110px_110px] gap-3 px-3 py-2 text-[10px] uppercase tracking-[0.14em] font-medium text-[--ds-text-tertiary] border-b border-[--ds-surface-hover]">
              <span>Spec</span>
              <span className="text-right">Tokens</span>
              <span className="text-right">Custo</span>
            </div>
            {topSpecs.length === 0 ? (
              <div className="px-3 py-4 text-[12px] text-[--ds-text-tertiary]">
                Nenhuma spec com consumo atribuído ainda.
              </div>
            ) : (
              <div className="flex flex-col">
                {topSpecs.map((s, idx) => (
                  <div
                    key={`cons-spec-${s.spec}`}
                    className={cn(
                      "grid grid-cols-[1fr_110px_110px] gap-3 items-center px-3 py-2",
                      idx !== topSpecs.length - 1 &&
                        "border-b border-[--ds-surface-hover]/60",
                    )}
                  >
                    <span
                      className="truncate font-mono text-[12.5px] text-[--ds-text-primary]"
                      title={s.spec}
                    >
                      {s.spec || "—"}
                    </span>
                    <span className="font-mono tabular-nums text-right text-[12px] text-[--ds-text-secondary]">
                      {formatTokens(s.total_tokens)}
                    </span>
                    <span className="font-mono tabular-nums text-right text-[12.5px] text-[--ds-text-primary]">
                      {consUsd(s.cost_usd)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
