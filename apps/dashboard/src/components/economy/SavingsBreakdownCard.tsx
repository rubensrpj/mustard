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

// User-facing labels: plain PT, no internal module names. The `estimated`
// flag triggers a small "(estimado)" suffix in the rendered row — used for
// recipe injection where the saving is a per-call estimate, not a measured
// counter delta.
const SOURCE_LABEL: Record<SavingsSource, string> = {
  rtk_rewrite: "Reescrita de comando shell",
  model_routing_downgrade: "Modelo mais barato quando seguro",
  bash_guard_block: "Comando bloqueado por segurança",
  budget_output_cut: "Resposta cortada por orçamento",
  recipe_injection: "Esqueleto de receita",
};

const SOURCE_HINT: Record<SavingsSource, string> = {
  rtk_rewrite: "encurtou o comando antes de rodar, mantendo o resultado",
  model_routing_downgrade: "trocou para um modelo mais barato quando a tarefa permitia",
  bash_guard_block: "barrou um comando destrutivo ou ruidoso antes da execução",
  budget_output_cut: "cortou uma resposta muito longa antes de devolver ao pai",
  recipe_injection: "injetou um esqueleto pronto em vez de pedir um do zero",
};

const SOURCE_ESTIMATED: Record<SavingsSource, boolean> = {
  rtk_rewrite: false,
  model_routing_downgrade: false,
  bash_guard_block: false,
  budget_output_cut: false,
  recipe_injection: true,
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
          tokens que a ferramenta evitou de gastar
        </span>
        <span className="text-[11px] text-[--ds-text-tertiary] tabular-nums">
          total: {formatTokens(breakdown?.total_tokens_saved ?? 0)} tok ·{" "}
          {totalEvents.toLocaleString()} ocorrências
        </span>
      </div>
      <div className="flex flex-col gap-1">
        {SOURCE_ORDER.map((src) => {
          const tokens = bySource.get(src) ?? 0;
          const label = SOURCE_ESTIMATED[src]
            ? `${SOURCE_LABEL[src]} (estimado)`
            : SOURCE_LABEL[src];
          return (
            <BaseRow
              key={src}
              label={label}
              summary={SOURCE_HINT[src]}
              tokens={tokens}
            />
          );
        })}
      </div>
    </div>
  );
}
