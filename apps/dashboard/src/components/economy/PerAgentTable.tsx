// PerAgentTable — top-N agents by cost for the W7 Economia page.
//
// Renders the `top_agents_by_cost` slice returned by
// `economy_summary(scope)` as a compact, dense table. The core reader caps the
// result at 3 entries today; we still allow up to 10 here so a future bump in
// the core ceiling lands without a UI change.

import { MetricsPill } from "@/components/ds";
import type { AgentCost } from "@/lib/types/economy";
import { formatTokens, formatUsd } from "@/lib/types/economy";

export interface PerAgentTableProps {
  agents: AgentCost[];
  /** Hard cap on rendered rows. Defaults to 10 — the core reader caps at 3
   * today but we expose the prop for future growth. */
  limit?: number;
}

export function PerAgentTable({ agents, limit = 10 }: PerAgentTableProps) {
  const rows = agents.slice(0, limit);

  if (rows.length === 0) {
    return (
      <p className="text-[12px] text-[--ds-text-tertiary] italic px-3 py-2">
        Nenhum agente custou nada neste escopo ainda.
      </p>
    );
  }

  return (
    <table className="w-full text-[12px] border-separate border-spacing-0">
      <thead>
        <tr className="text-left text-[11px] uppercase tracking-wide text-[--ds-text-tertiary]">
          <th className="font-medium px-3 py-2">Agente</th>
          <th className="font-medium px-3 py-2 text-right">Spans</th>
          <th className="font-medium px-3 py-2 text-right">Tokens</th>
          <th className="font-medium px-3 py-2 text-right">Custo</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr
            key={r.agent_id || `row-${i}`}
            className="border-t border-[--ds-surface-hover] hover:bg-[--ds-surface-hover]/50"
          >
            <td className="px-3 py-2 font-mono text-[--ds-text-primary] truncate max-w-[260px]">
              {r.agent_id || "—"}
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
      </tbody>
    </table>
  );
}
