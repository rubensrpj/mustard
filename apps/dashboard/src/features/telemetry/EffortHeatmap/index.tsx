import { useState } from "react";
import { cn } from "@/lib/utils";
import type { HeatmapCell } from "@/lib/types/telemetry";

const DAY_LABELS = ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"];
const HOUR_MARKS = [0, 6, 12, 18];

const CELL_SIZE = 12; // px
const CELL_GAP = 2;  // px

export interface EffortHeatmapProps {
  cells: HeatmapCell[];
  className?: string;
}

interface TooltipState {
  x: number;
  y: number;
  label: string;
}

export function EffortHeatmap({ cells, className }: EffortHeatmapProps) {
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);

  // Build lookup: key = `${dow}:${hr}` → count
  const lookup = new Map<string, number>();
  let maxCount = 0;
  for (const c of cells) {
    lookup.set(`${c.day_of_week}:${c.hour}`, c.event_count);
    if (c.event_count > maxCount) maxCount = c.event_count;
  }

  const totalW = 24 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
  const totalH = 7 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
  const svgW = totalW + 32; // left margin for day labels
  const svgH = totalH + 20; // bottom margin for hour labels

  return (
    <div className={cn("relative inline-block", className)}>
      <svg
        width={svgW}
        height={svgH}
        role="img"
        aria-label="Heatmap de atividade por dia e hora"
        className="overflow-visible"
      >
        {/* day-of-week labels (left) */}
        {DAY_LABELS.map((d, dow) => (
          <text
            key={dow}
            x={28}
            y={dow * (CELL_SIZE + CELL_GAP) + CELL_SIZE / 2 + 4}
            textAnchor="end"
            className="fill-muted-foreground"
            style={{ fontSize: 9 }}
          >
            {d}
          </text>
        ))}

        {/* hour labels (bottom) */}
        {HOUR_MARKS.map((hr) => (
          <text
            key={hr}
            x={32 + hr * (CELL_SIZE + CELL_GAP)}
            y={svgH - 2}
            textAnchor="middle"
            className="fill-muted-foreground"
            style={{ fontSize: 9 }}
          >
            {String(hr).padStart(2, "0")}
          </text>
        ))}

        {/* cells */}
        {Array.from({ length: 7 }, (_, dow) =>
          Array.from({ length: 24 }, (_, hr) => {
            const count = lookup.get(`${dow}:${hr}`) ?? 0;
            const opacity = maxCount > 0 ? count / maxCount : 0;
            const cx = 32 + hr * (CELL_SIZE + CELL_GAP);
            const cy = dow * (CELL_SIZE + CELL_GAP);
            const label = `${DAY_LABELS[dow]} · ${String(hr).padStart(2, "0")}h · ${count} eventos`;

            return (
              <rect
                key={`${dow}-${hr}`}
                x={cx}
                y={cy}
                width={CELL_SIZE}
                height={CELL_SIZE}
                rx={2}
                role="img"
                aria-label={label}
                style={{
                  fill:
                    opacity === 0
                      ? "var(--color-paper, #252525)"
                      : `color-mix(in srgb, var(--primary, #e6c84a) ${Math.round(opacity * 100)}%, var(--color-paper, #252525))`,
                  cursor: "crosshair",
                }}
                onMouseEnter={(e) => {
                  const rect = (e.target as SVGRectElement).getBoundingClientRect();
                  setTooltip({ x: rect.left + CELL_SIZE / 2, y: rect.top, label });
                }}
                onMouseLeave={() => setTooltip(null)}
              />
            );
          })
        )}
      </svg>

      {/* tooltip */}
      {tooltip && (
        <div
          className="fixed z-50 pointer-events-none rounded bg-popover border border-border px-2 py-1 text-[11px] text-popover-foreground shadow-md whitespace-nowrap"
          style={{ left: tooltip.x, top: tooltip.y - 30, transform: "translateX(-50%)" }}
        >
          {tooltip.label}
        </div>
      )}
    </div>
  );
}
