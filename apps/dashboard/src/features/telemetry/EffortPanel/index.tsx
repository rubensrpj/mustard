import { cn } from "@/lib/utils";
import type { EffortBreakdown } from "@/lib/types/telemetry";

export interface EffortPanelProps {
  effort: EffortBreakdown;
  className?: string;
}

interface BarRowItem {
  label: string;
  count: number;
}

/** Middle-truncation: keep START chars, then …, then END chars of the tail. */
function midTruncate(s: string, start = 14, end = 10): string {
  if (s.length <= start + end + 1) return s;
  return `${s.slice(0, start)}…${s.slice(-end)}`;
}

function BarRow({ label, count, maxCount }: BarRowItem & { maxCount: number }) {
  const pct = maxCount > 0 ? (count / maxCount) * 100 : 0;
  const shortLabel = midTruncate(label);

  return (
    <div className="flex items-center gap-2 min-w-0">
      <span
        className="text-[11px] text-muted-foreground flex-1 min-w-0 overflow-hidden whitespace-nowrap"
        title={label}
      >
        {shortLabel}
      </span>
      <div className="flex-shrink-0 w-20 h-1 rounded-full bg-muted overflow-hidden">
        <div
          className="h-full rounded-full bg-[--color-ink-subtle]"
          style={{ width: `${pct.toFixed(1)}%` }}
        />
      </div>
      <span
        className="text-[11px] text-muted-foreground w-8 text-right"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {count}
      </span>
    </div>
  );
}

function Section({
  title,
  items,
}: {
  title: string;
  items: BarRowItem[];
}) {
  const max = items[0]?.count ?? 1;
  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-[10px] text-muted-foreground/70 mb-0.5">{title}</p>
      {items.length === 0 ? (
        <p className="text-[11px] text-muted-foreground/40">—</p>
      ) : (
        items.map((item) => (
          <BarRow
            key={item.label}
            label={item.label}
            count={item.count}
            maxCount={max}
          />
        ))
      )}
    </div>
  );
}

export function EffortPanel({ effort, className }: EffortPanelProps) {
  return (
    <div className={cn("grid grid-cols-2 gap-x-6 gap-y-4", className)}>
      <Section
        title="arquivos"
        items={effort.top_files.map((f) => ({ label: f.path, count: f.count }))}
      />
      <Section
        title="ferramentas"
        items={effort.top_tools.map((t) => ({ label: t.name, count: t.count }))}
      />
      <Section
        title="fases"
        items={effort.top_phases.map((p) => ({
          label: p.phase,
          count: p.duration_ms,
        }))}
      />
      <Section
        title="agentes"
        items={effort.top_agents.map((a) => ({
          label: a.agent_type,
          count: a.count,
        }))}
      />
    </div>
  );
}
