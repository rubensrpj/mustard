// SavingsBreakdownCard — one `<BaseRow>` per `SavingsSource` for the W7
// Economia page. Sources missing from the backend response collapse to
// zero rows so the user sees the full intervention set ordered consistently.

import { BaseRow } from "@/components/ds";
import type { SavingsBreakdown, SavingsSource } from "@/lib/types/economy";
import { formatTokens } from "@/lib/types/economy";

const SOURCE_ORDER: readonly SavingsSource[] = [
  "rtk_rewrite",
  "model_routing_downgrade",
  "bash_guard_block",
  "budget_output_cut",
  "recipe_injection",
] as const;

const SOURCE_LABEL: Record<SavingsSource, string> = {
  rtk_rewrite: "RTK rewrite",
  model_routing_downgrade: "Model routing (downgrade)",
  bash_guard_block: "Bash guard block",
  budget_output_cut: "Budget output cut",
  recipe_injection: "Recipe injection",
};

const SOURCE_HINT: Record<SavingsSource, string> = {
  rtk_rewrite: "rtk reescreveu o comando em forma compacta",
  model_routing_downgrade: "routing trocou Opus por modelo mais barato (seguro)",
  bash_guard_block: "bash_guard bloqueou comando destrutivo/ruidoso",
  budget_output_cut: "budget cortou retorno antes de re-injetar no pai",
  recipe_injection: "recipe esqueleto injetado em vez de derivar do zero",
};

export interface SavingsBreakdownCardProps {
  breakdown: SavingsBreakdown | undefined;
}

export function SavingsBreakdownCard({ breakdown }: SavingsBreakdownCardProps) {
  // Build a lookup so missing sources render as zero rows rather than
  // disappearing — the user should see the full set of interventions even
  // when one hasn't fired in this scope.
  const bySource = new Map<SavingsSource, number>();
  let totalEvents = 0;
  for (const row of breakdown?.per_source ?? []) {
    bySource.set(row.source, row.tokens_saved);
    totalEvents += row.event_count;
  }

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-baseline justify-between px-1 pb-2">
        <span className="text-[11px] text-[--ds-text-tertiary] uppercase tracking-wide">
          tokens economizados por intervenção
        </span>
        <span className="text-[11px] text-[--ds-text-tertiary] tabular-nums">
          total: {formatTokens(breakdown?.total_tokens_saved ?? 0)} tok ·{" "}
          {totalEvents.toLocaleString()} eventos
        </span>
      </div>
      <div className="flex flex-col gap-1">
        {SOURCE_ORDER.map((src) => {
          const tokens = bySource.get(src) ?? 0;
          return (
            <BaseRow
              key={src}
              label={SOURCE_LABEL[src]}
              summary={SOURCE_HINT[src]}
              tokens={tokens}
            />
          );
        })}
      </div>
    </div>
  );
}
