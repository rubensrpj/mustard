import { useState, useMemo } from "react";
import { Search } from "lucide-react";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import { EventChip } from "@/components/page";
import type { TimelineEvent, EventFilter } from "@/lib/types/specs";

interface SpecEventsTabProps {
  events: TimelineEvent[];
  /** Pre-selected filter values (driven by clicking a timeline node). */
  initialFilter?: EventFilter;
}

const TYPE_CHIPS = [
  { label: "fase",   value: "pipeline.phase" },
  { label: "onda",   value: "pipeline.wave.start" },
  { label: "QA",     value: "qa.result" },
  { label: "agente", value: "agent.start" },
  { label: "tool",   value: "tool.use" },
];

export function SpecEventsTab({ events, initialFilter }: SpecEventsTabProps) {
  const [query, setQuery] = useState(initialFilter?.q ?? "");
  const [selectedKind, setSelectedKind] = useState<string | null>(
    initialFilter?.kinds?.[0] ?? null,
  );

  const filtered = useMemo(() => {
    let out = events;
    const q = query.trim().toLowerCase();
    if (q.length >= 2) {
      out = out.filter(
        (e) =>
          e.summary.toLowerCase().includes(q) ||
          (e.phase?.toLowerCase().includes(q) ?? false) ||
          (e.agent?.toLowerCase().includes(q) ?? false),
      );
    }
    if (selectedKind) {
      // Events from this tab don't carry the original `event` type string,
      // but summary/phase/agent can approximate — match by phase for phase chip
      // and agent for agent chip as best-effort.
      if (selectedKind.startsWith("pipeline.phase")) {
        out = out.filter((e) => !!e.phase);
      } else if (selectedKind.startsWith("agent")) {
        out = out.filter((e) => !!e.agent);
      }
      // Other chips narrow by summary content for now
    }
    return out;
  }, [events, query, selectedKind]);

  return (
    <div className="flex flex-col gap-3">
      {/* Search + chips */}
      <div className="flex flex-col gap-2">
        <div className="relative">
          <Search
            className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground"
            aria-hidden
          />
          <label htmlFor="spec-events-search" className="sr-only">
            Filtrar eventos
          </label>
          <input
            id="spec-events-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filtrar eventos…"
            className="w-full pl-8 pr-3 py-1.5 bg-card border border-border rounded-md text-[12px] outline-none placeholder:text-muted-foreground focus:border-primary transition-colors"
          />
        </div>

        <div className="flex items-center gap-1.5 flex-wrap" role="group" aria-label="Filtrar por tipo">
          {TYPE_CHIPS.map((chip) => (
            <button
              key={chip.value}
              type="button"
              onClick={() =>
                setSelectedKind((prev) => (prev === chip.value ? null : chip.value))
              }
              className={cn(
                "text-[11px] px-2 py-0.5 rounded-full border transition-colors",
                selectedKind === chip.value
                  ? "border-[--primary] text-[--primary] bg-[--primary]/10"
                  : "border-border text-muted-foreground hover:border-muted-foreground",
              )}
              aria-pressed={selectedKind === chip.value}
            >
              {chip.label}
            </button>
          ))}
          {selectedKind && (
            <button
              type="button"
              onClick={() => setSelectedKind(null)}
              className="text-[11px] text-muted-foreground/60 hover:text-muted-foreground ml-1"
              aria-label="Limpar filtro de tipo"
            >
              × limpar
            </button>
          )}
        </div>
      </div>

      {/* Event list */}
      {filtered.length === 0 ? (
        <p className="text-[13px] text-muted-foreground py-4 text-center">
          Nenhum evento encontrado.
        </p>
      ) : (
        <ul className="flex flex-col gap-1 max-h-[400px] overflow-y-auto pr-1">
          {filtered.map((ev) => (
            <li
              key={ev.id}
              className="flex items-start gap-2 py-1.5 border-b border-border/30 last:border-b-0"
            >
              <span className="text-[11px] text-muted-foreground/50 shrink-0 tabular-nums pt-0.5"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                {relativeTime(ev.ts)}
              </span>
              <div className="flex flex-col gap-0.5 min-w-0 flex-1">
                <div className="flex items-center gap-1.5 flex-wrap">
                  {ev.phase && (
                    <EventChip eventType={ev.phase} />
                  )}
                  {ev.agent && (
                    <span className="text-[10px] font-mono text-muted-foreground/60 bg-muted px-1 rounded truncate max-w-[100px]"
                      title={ev.agent}
                    >
                      {ev.agent}
                    </span>
                  )}
                </div>
                <p className="text-[12px] text-foreground/80 leading-snug">
                  {ev.summary}
                </p>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
