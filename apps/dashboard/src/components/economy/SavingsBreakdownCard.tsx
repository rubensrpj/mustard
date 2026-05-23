// SavingsBreakdownCard — one `<BaseRow>` per `SavingsSource` for the W7
// Economia page. Sources missing from the backend response collapse to
// zero rows so the user sees the full intervention set ordered consistently.
//
// User-facing strings are resolved via `react-i18next`'s `useTranslation`. The
// per-source label/hint pairs live under `economy.savings.source.<key>` and
// `<key>.hint` in `i18n.ts`. The duplicated all-caps subheader was removed in
// the 2026-05-23-economia-i18n-migration sub-spec — the parent `<h2>` in
// `Economia.tsx` is now the single title for this section.

import { useTranslation } from "react-i18next";
import { BaseRow } from "@/components/page";
import type { SavingsBreakdown, SavingsSource } from "@/lib/types/economy";
import { formatTokens } from "@/lib/types/economy";

const SOURCE_ORDER: readonly SavingsSource[] = [
  "rtk_rewrite",
  "model_routing_downgrade",
  "bash_guard_block",
  "budget_output_cut",
  "recipe_injection",
] as const;

// Recipe injection is the only currently-estimated source; we still drive the
// flag through a map so adding a new estimated source is a one-line change.
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
  const { t } = useTranslation();

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
      <div className="flex items-baseline justify-end px-1 pb-2">
        <span className="text-[11px] text-[--ds-text-tertiary] tabular-nums">
          {t("economy.savings.total", {
            count: totalEvents,
            tokens: formatTokens(breakdown?.total_tokens_saved ?? 0),
          })}
        </span>
      </div>
      <div className="flex flex-col gap-1">
        {SOURCE_ORDER.map((src) => {
          const tokens = bySource.get(src) ?? 0;
          const base = t(`economy.savings.source.${src}`);
          const label = SOURCE_ESTIMATED[src]
            ? `${base} ${t("economy.savings.estimatedSuffix")}`
            : base;
          return (
            <BaseRow
              key={src}
              label={label}
              summary={t(`economy.savings.source.${src}.hint`)}
              tokens={tokens}
            />
          );
        })}
      </div>
    </div>
  );
}
