// PerAgentTable — top-3 agents + aggregate "Others" + estimated total.
//
// Renders the `top_agents_by_cost` slice returned by `economy_summary(scope)`
// as a compact, dense table. The core reader caps the result at 3 today and
// the page passes the full list through; we render top-3 inline, fold the
// remainder into a single "Outros (N agentes)" row, then close with a footer
// that totals the estimated cost and — when available — a discreet caption
// comparing it against the measured cost (`total_cost_usd_micros`). The gap
// is the cache-aware vs. billed-reality delta; surfacing it once is enough.
//
// Humanization: backend ids like `core-impl` / `general-purpose` are
// terminal-friendly but unreadable in a user surface. `humanizeAgent` looks
// each id up in the i18n bundle (`economy.agents.<id>`) and falls back to the
// raw id when no key matches, so adding a new role never crashes the page.

import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { MetricsPill } from "@/components/ds";
import type { AgentCost } from "@/lib/types/economy";
import { formatTokens, formatUsd } from "@/lib/types/economy";

export interface PerAgentTableProps {
  agents: AgentCost[];
  /** Hard cap on rendered top rows. Defaults to 3 — matches the core reader's
   * cap today and keeps the visible band tight. Excess agents collapse into
   * the "Others" aggregate row. */
  topN?: number;
  /**
   * Measured project cost in micro-USD (from `EconomySummary.total_cost_usd_micros`).
   * When provided, a discreet "≈ medido $X.XX" caption is rendered under the
   * estimated total so the user can eyeball the gap (estimate vs. Anthropic-
   * billed reality). Null/undefined → caption hidden.
   */
  measuredCostMicros?: number | null;
}

/**
 * Resolve a backend agent id to a human label via the i18n bundle. Falls back
 * to the raw id (so a new role never blanks the table). The lookup never
 * throws — `t(key, { defaultValue: id })` is safe even if the key is missing.
 */
function humanizeAgent(id: string, t: TFunction): string {
  if (!id) return "—";
  return t(`economy.agents.${id}`, { defaultValue: id });
}

export function PerAgentTable({
  agents,
  topN = 3,
  measuredCostMicros = null,
}: PerAgentTableProps) {
  const { t } = useTranslation();

  if (agents.length === 0) {
    return (
      <p className="text-[12px] text-[--ds-text-tertiary] italic px-3 py-2">
        {t("economy.table.empty")}
      </p>
    );
  }

  const topRows = agents.slice(0, topN);
  const restRows = agents.slice(topN);

  // Aggregate the tail into a single "Outros (N)" row. We keep the aggregate
  // even when restRows is empty? No — render only when there's something to
  // collapse, otherwise the footer would lie about a non-existent bucket.
  const restAggregate = restRows.reduce(
    (acc, r) => {
      acc.span_count += r.span_count;
      acc.tokens += r.tokens;
      acc.cost_usd_micros += r.cost_usd_micros;
      return acc;
    },
    { span_count: 0, tokens: 0, cost_usd_micros: 0 },
  );

  // Estimated total spans the full input — not just topN — so the row matches
  // the sum the user can pull out of the wire payload directly.
  const total = agents.reduce(
    (acc, r) => {
      acc.span_count += r.span_count;
      acc.tokens += r.tokens;
      acc.cost_usd_micros += r.cost_usd_micros;
      return acc;
    },
    { span_count: 0, tokens: 0, cost_usd_micros: 0 },
  );

  return (
    <table className="w-full text-[12px] border-separate border-spacing-0">
      <thead>
        <tr className="text-left text-[11px] uppercase tracking-wide text-[--ds-text-tertiary]">
          <th className="font-medium px-3 py-2">{t("economy.table.agent")}</th>
          <th className="font-medium px-3 py-2 text-right">
            {t("economy.table.dispatches")}
          </th>
          <th className="font-medium px-3 py-2 text-right">
            {t("economy.table.tokens")}
          </th>
          <th className="font-medium px-3 py-2 text-right">
            {t("economy.table.cost")}
          </th>
        </tr>
      </thead>
      <tbody>
        {topRows.map((r, i) => (
          <tr
            key={r.agent_id || `row-${i}`}
            className="border-t border-[--ds-surface-hover] hover:bg-[--ds-surface-hover]/50 transition-colors"
          >
            <td className="px-3 py-2 truncate max-w-[260px]">
              <span className="text-[--ds-text-primary]">
                {humanizeAgent(r.agent_id, t)}
              </span>
              {r.agent_id && r.agent_id !== humanizeAgent(r.agent_id, t) && (
                <span className="font-mono text-[10.5px] text-[--ds-text-tertiary] ml-2">
                  {r.agent_id}
                </span>
              )}
            </td>
            <td className="px-3 py-2 text-right text-[--ds-text-secondary] tabular-nums">
              {r.span_count.toLocaleString()}
            </td>
            <td className="px-3 py-2 text-right">
              <MetricsPill value={formatTokens(r.tokens)} unit="tok" />
            </td>
            <td className="px-3 py-2 text-right">
              <MetricsPill
                value={formatUsd(r.cost_usd_micros)}
                intent={r.cost_usd_micros > 0 ? "info" : "neutral"}
              />
            </td>
          </tr>
        ))}

        {restRows.length > 0 && (
          <tr className="border-t border-[--ds-surface-hover] bg-[--ds-surface-hover]/15">
            <td className="px-3 py-2 text-[--ds-text-tertiary] italic">
              {t("economy.byAgent.others", { count: restRows.length })}
            </td>
            <td className="px-3 py-2 text-right text-[--ds-text-tertiary] tabular-nums">
              {restAggregate.span_count.toLocaleString()}
            </td>
            <td className="px-3 py-2 text-right">
              <MetricsPill value={formatTokens(restAggregate.tokens)} unit="tok" />
            </td>
            <td className="px-3 py-2 text-right">
              <MetricsPill
                value={formatUsd(restAggregate.cost_usd_micros)}
                intent="neutral"
              />
            </td>
          </tr>
        )}
      </tbody>
      <tfoot>
        <tr className="border-t-2 border-[--ds-surface-hover]">
          <td className="px-3 py-2.5 font-medium text-[--ds-text-primary]">
            {t("economy.byAgent.total")}
          </td>
          <td className="px-3 py-2.5 text-right font-medium text-[--ds-text-primary] tabular-nums">
            {total.span_count.toLocaleString()}
          </td>
          <td className="px-3 py-2.5 text-right">
            <MetricsPill value={formatTokens(total.tokens)} unit="tok" />
          </td>
          <td className="px-3 py-2.5 text-right">
            <MetricsPill
              value={formatUsd(total.cost_usd_micros)}
              intent={total.cost_usd_micros > 0 ? "info" : "neutral"}
            />
          </td>
        </tr>
        {measuredCostMicros != null && measuredCostMicros > 0 && (
          <tr>
            <td colSpan={4} className="px-3 pb-2 pt-0">
              <span className="text-[10.5px] text-[--ds-text-tertiary] tabular-nums">
                {t("economy.byAgent.matchMeasured", {
                  cost: formatUsd(measuredCostMicros),
                })}
              </span>
            </td>
          </tr>
        )}
      </tfoot>
    </table>
  );
}
