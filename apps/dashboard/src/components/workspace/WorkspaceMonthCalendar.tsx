import { useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader, EmptyState } from "@/components/page";
import { useWorkspaceMonthActivity } from "@/hooks/useWorkspaceMonthActivity";
import type { DayActivity } from "@/lib/dashboard";

interface WorkspaceMonthCalendarProps {
  repoPath: string;
}

const MONTH_LABEL = [
  "Janeiro",
  "Fevereiro",
  "Março",
  "Abril",
  "Maio",
  "Junho",
  "Julho",
  "Agosto",
  "Setembro",
  "Outubro",
  "Novembro",
  "Dezembro",
];

// Weekday header — Sunday first, matching the 7-column grid layout below.
const WEEKDAY_LABEL = ["D", "S", "T", "Q", "Q", "S", "S"];

/** Pad a 1..N number into a zero-padded 2-digit string. */
function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

/** Number of days in the given month (1..12, 1-indexed). */
function daysInMonth(year: number, month: number): number {
  // new Date(year, month, 0) → last day of previous month.
  return new Date(year, month, 0).getDate();
}

/** Returns the densityClass for a given event count. */
function densityClass(count: number): string {
  if (count <= 0) return "bg-transparent";
  if (count <= 3) return "bg-sky-500/15";
  if (count <= 9) return "bg-amber-500/25";
  return "bg-emerald-500/35";
}

/**
 * Monthly activity heatmap. State holds (year, month); the Tauri query is
 * keyed by `(repoPath, year, month)` so navigating months refetches cleanly.
 */
export function WorkspaceMonthCalendar({ repoPath }: WorkspaceMonthCalendarProps) {
  const now = new Date();
  const [year, setYear] = useState<number>(now.getFullYear());
  // useState month is 1..12 to align with the backend signature.
  const [month, setMonth] = useState<number>(now.getMonth() + 1);

  const { data, isLoading } = useWorkspaceMonthActivity(repoPath, year, month);

  const byDate = useMemo(() => {
    const map = new Map<string, DayActivity>();
    for (const d of data ?? []) map.set(d.date, d);
    return map;
  }, [data]);

  const totalDays = daysInMonth(year, month);
  // JavaScript Date.getDay() → 0 (Sun) .. 6 (Sat), matches our header.
  const firstWeekday = new Date(year, month - 1, 1).getDay();

  // Build a flat 42-cell grid (6 rows × 7 cols). Cells before/after the month
  // are rendered as inert spacers so the calendar lines up by weekday.
  const cells: Array<{ day: number | null; date: string | null }> = [];
  for (let i = 0; i < firstWeekday; i += 1) cells.push({ day: null, date: null });
  for (let d = 1; d <= totalDays; d += 1) {
    const date = `${year}-${pad2(month)}-${pad2(d)}`;
    cells.push({ day: d, date });
  }
  while (cells.length < 42) cells.push({ day: null, date: null });

  function step(delta: number) {
    let m = month + delta;
    let y = year;
    if (m < 1) {
      m = 12;
      y -= 1;
    } else if (m > 12) {
      m = 1;
      y += 1;
    }
    setMonth(m);
    setYear(y);
  }

  const navigate = useNavigate();

  const header = (
    <div className="flex items-center gap-1.5">
      <button
        type="button"
        onClick={() => step(-1)}
        aria-label="Mês anterior"
        className={cn(
          "p-1 rounded hover:bg-muted/50 text-muted-foreground hover:text-foreground",
          "focus-visible:outline-none focus-visible:ring-2",
          "focus-visible:ring-[--color-accent-mustard]",
        )}
      >
        <ChevronLeft className="h-3.5 w-3.5" aria-hidden />
      </button>
      <span
        className="text-[11.5px] font-medium text-foreground/80 tabular-nums min-w-[110px] text-center"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {MONTH_LABEL[month - 1]} {year}
      </span>
      <button
        type="button"
        onClick={() => step(1)}
        aria-label="Próximo mês"
        className={cn(
          "p-1 rounded hover:bg-muted/50 text-muted-foreground hover:text-foreground",
          "focus-visible:outline-none focus-visible:ring-2",
          "focus-visible:ring-[--color-accent-mustard]",
        )}
      >
        <ChevronRight className="h-3.5 w-3.5" aria-hidden />
      </button>
    </div>
  );

  const showEmpty = !isLoading && (!data || data.length === 0);

  return (
    <DataCard padded>
      <SectionHeader title="Atividade do mês" right={header} />

      {showEmpty ? (
        <EmptyState
          className="mt-3"
          title="Sem atividade neste mês"
          description="Eventos do harness aparecem aqui assim que pipelines rodarem."
        />
      ) : (
        <>
          <div className="mt-3 grid grid-cols-7 gap-1">
            {WEEKDAY_LABEL.map((wd, i) => (
              <div
                key={`wd-${i}`}
                className="text-center text-[10px] uppercase tracking-wide text-muted-foreground/70"
              >
                {wd}
              </div>
            ))}
            {cells.map((cell, i) => {
              if (cell.day == null || cell.date == null) {
                return <div key={`pad-${i}`} className="aspect-square" aria-hidden />;
              }
              const entry = byDate.get(cell.date);
              const count = entry?.event_count ?? 0;
              const phase = entry?.top_phase ?? "—";
              const title = `${count} eventos · ${phase}`;
              return (
                <button
                  key={cell.date}
                  type="button"
                  title={title}
                  aria-label={`${cell.date}: ${title}`}
                  onClick={() => navigate(`/specs?date=${cell.date}`)}
                  className={cn(
                    "aspect-square rounded text-[11px] tabular-nums",
                    "flex items-center justify-center",
                    "border border-border/40 hover:border-foreground/30",
                    "focus-visible:outline-none focus-visible:ring-2",
                    "focus-visible:ring-[--color-accent-mustard] transition-colors",
                    densityClass(count),
                    count === 0
                      ? "text-muted-foreground/60"
                      : "text-foreground font-medium",
                  )}
                  style={{ fontVariantNumeric: "tabular-nums" }}
                >
                  {cell.day}
                </button>
              );
            })}
          </div>

          {/* Density legend — low → high */}
          <div className="mt-3 flex items-center justify-end gap-1.5">
            <span className="text-[10px] text-muted-foreground/70">menos</span>
            <span className="h-3 w-3 rounded border border-border/40 bg-transparent" />
            <span className="h-3 w-3 rounded bg-sky-500/15" />
            <span className="h-3 w-3 rounded bg-amber-500/25" />
            <span className="h-3 w-3 rounded bg-emerald-500/35" />
            <span className="text-[10px] text-muted-foreground/70">mais</span>
          </div>
        </>
      )}
    </DataCard>
  );
}
