import { cn } from "@/lib/utils";
import { useTelemetryTimeRange } from "../TelemetryTimeRangeContext";
import type { TimeRange } from "@/lib/types/telemetry";

const OPTIONS: { value: TimeRange; label: string }[] = [
  { value: "today", label: "Hoje" },
  { value: "7d", label: "7 dias" },
  { value: "30d", label: "30 dias" },
  { value: "all", label: "Tudo" },
];

export interface TimeRangeSelectorProps {
  className?: string;
}

export function TimeRangeSelector({ className }: TimeRangeSelectorProps) {
  const { timeRange, setTimeRange } = useTelemetryTimeRange();

  return (
    <div
      role="group"
      aria-label="Selecionar período"
      className={cn(
        "inline-flex items-center gap-0.5 rounded-full border border-border bg-muted/40 p-0.5",
        className,
      )}
    >
      {OPTIONS.map((opt) => {
        const active = timeRange === opt.value;
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => setTimeRange(opt.value)}
            aria-pressed={active}
            className={cn(
              "rounded-full px-3 py-1 text-[12px] font-medium transition-colors",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary] focus-visible:ring-offset-1",
              active
                ? "bg-[--primary]/20 text-[--primary] shadow-sm"
                : "text-muted-foreground hover:text-foreground hover:bg-muted",
            )}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
