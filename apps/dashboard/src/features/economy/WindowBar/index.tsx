// WindowBar — 4-option time-window picker for the Economia page, shown beside
// the <ScopeBar>. Emits an `EconomyWindowPeriod` (1d/7d/15d/30d) whenever the
// user picks a period; the parent page owns the state and derives the concrete
// `TimeWindow` (lib/time.ts::economyWindow) at fetch time, so the "last N days"
// bound tracks now without churning the query key (which stays keyed on the
// period). The window composes onto the scope — it narrows which events fold in,
// never which project/spec/wave the <ScopeBar> selected.

import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import type { EconomyWindowPeriod } from "@/lib/types/economy";

// Exactly the four offered periods, widest last. Rendered as their own labels
// (day counts are language-neutral, so only the leading "Período:" is translated).
const PERIODS: EconomyWindowPeriod[] = ["1d", "7d", "15d", "30d"];

export interface WindowBarProps {
  /** Currently selected period — drives which toggle reads active. */
  period: EconomyWindowPeriod;
  onPeriodChange: (period: EconomyWindowPeriod) => void;
}

export function WindowBar({ period, onPeriodChange }: WindowBarProps) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2 text-[12px]">
      <span className="text-[--ds-text-tertiary]">{t("economy.window.label")}</span>
      <div className="inline-flex flex-wrap items-center gap-1.5">
        {PERIODS.map((p) => {
          const active = p === period;
          return (
            <button
              key={p}
              type="button"
              onClick={() => onPeriodChange(p)}
              className={cn(
                "inline-flex items-center px-2.5 py-1.5 rounded-[--ds-radius-md] text-[12px] font-medium transition-colors border tabular-nums",
                active
                  ? "bg-[--ds-accent-primary]/10 border-[--ds-accent-primary]/40 text-[--ds-accent-primary]"
                  : "bg-[--ds-surface-base] border-[--ds-surface-hover] text-[--ds-text-secondary] hover:text-[--ds-text-primary] hover:bg-[--ds-surface-hover]",
              )}
            >
              {p}
            </button>
          );
        })}
      </div>
    </div>
  );
}
