import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { SpecTrackRow } from "../SpecTrackRow";
import type { SpecTrack } from "@/lib/types/specs";

interface SpecTracksListProps {
  tracks: SpecTrack[];
  className?: string;
}

// Mirrors the TERMINAL_STATUSES set used in `Specs.tsx`; kept inline because
// Visão Geral's segregation is "active vs. closed today" while the Specs page
// uses a fuller filter taxonomy.
const TERMINAL_STATUSES = new Set([
  "completed",
  "closed",
  "rejected",
  "aborted",
  "amended",
  "cancelled",
]);

function isActive(track: SpecTrack): boolean {
  return !TERMINAL_STATUSES.has(track.status);
}

/** Local-time start-of-day in epoch ms. */
function startOfTodayMs(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function isClosedToday(track: SpecTrack, todayStart: number): boolean {
  if (!TERMINAL_STATUSES.has(track.status)) return false;
  if (!track.last_event_at) return false;
  return new Date(track.last_event_at).getTime() >= todayStart;
}

export function SpecTracksList({ tracks, className }: SpecTracksListProps) {
  const [closedExpanded, setClosedExpanded] = useState(false);

  const { active, closedToday } = useMemo(() => {
    const todayStart = startOfTodayMs();
    const sorted = [...tracks].sort((a, b) => {
      const ta = a.last_event_at ? new Date(a.last_event_at).getTime() : 0;
      const tb = b.last_event_at ? new Date(b.last_event_at).getTime() : 0;
      return tb - ta; // most recent first
    });

    return {
      active: sorted.filter(isActive),
      closedToday: sorted.filter((t) => isClosedToday(t, todayStart)),
    };
  }, [tracks]);

  if (active.length === 0 && closedToday.length === 0) {
    return (
      <p className="text-[13px] text-muted-foreground px-3 py-4">
        Nenhuma spec em execução nem encerrada hoje.
      </p>
    );
  }

  return (
    <div className={cn("flex flex-col gap-2", className)}>
      <section aria-label="Em execução" className="flex flex-col gap-0.5">
        <h3 className="text-[11px] uppercase tracking-wide text-muted-foreground font-medium px-3">
          Em execução{" "}
          <span
            className="tabular-nums text-foreground/70"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {active.length}
          </span>
        </h3>
        {active.length === 0 ? (
          <p className="text-[12px] text-muted-foreground/60 px-3 py-1">
            Nenhuma spec ativa no momento.
          </p>
        ) : (
          active.map((t) => <SpecTrackRow key={t.spec} track={t} />)
        )}
      </section>

      {closedToday.length > 0 && (
        <section aria-label="Concluídas hoje" className="flex flex-col gap-0.5">
          <button
            type="button"
            onClick={() => setClosedExpanded((v) => !v)}
            aria-expanded={closedExpanded}
            className={cn(
              "flex items-center gap-1 text-[11px] uppercase tracking-wide",
              "text-muted-foreground font-medium px-3 py-1 rounded",
              "hover:text-foreground focus-visible:outline-none",
              "focus-visible:ring-2 focus-visible:ring-[--primary]",
            )}
          >
            {closedExpanded ? (
              <ChevronDown className="h-3 w-3" aria-hidden />
            ) : (
              <ChevronRight className="h-3 w-3" aria-hidden />
            )}
            Concluídas hoje{" "}
            <span
              className="tabular-nums text-foreground/70"
              style={{ fontVariantNumeric: "tabular-nums" }}
            >
              {closedToday.length}
            </span>
          </button>
          {closedExpanded &&
            closedToday.map((t) => <SpecTrackRow key={t.spec} track={t} />)}
        </section>
      )}
    </div>
  );
}
