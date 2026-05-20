import { useEffect, useRef } from "react";
import {
  Search,
  ClipboardList,
  Zap,
  CheckSquare,
  Archive,
} from "lucide-react";
import { cn } from "@/lib/utils";

export type PhaseStationState = "future" | "active" | "completed";

export interface PhaseStationProps {
  phase: "analyze" | "plan" | "execute" | "qa" | "close";
  state: PhaseStationState;
  eventsCount?: number;
  durationMs?: number;
  className?: string;
}

const PHASE_META: Record<
  PhaseStationProps["phase"],
  { label: string; Icon: React.ComponentType<{ className?: string }> }
> = {
  analyze:  { label: "Analyze",  Icon: Search },
  plan:     { label: "Plan",     Icon: ClipboardList },
  execute:  { label: "Execute",  Icon: Zap },
  qa:       { label: "QA",       Icon: CheckSquare },
  close:    { label: "Close",    Icon: Archive },
};

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return rem > 0 ? `${m}m ${rem}s` : `${m}m`;
}

export function PhaseStation({
  phase,
  state,
  eventsCount,
  durationMs,
  className,
}: PhaseStationProps) {
  const { label, Icon } = PHASE_META[phase];
  const dotRef = useRef<HTMLDivElement>(null);

  // wave-glow fires once when state becomes active
  useEffect(() => {
    if (state === "active" && dotRef.current) {
      dotRef.current.classList.remove("animate-wave-glow");
      // force reflow to restart animation
      void dotRef.current.offsetWidth;
      dotRef.current.classList.add("animate-wave-glow");
    }
  }, [state]);

  const ariaLabel = `${label}${eventsCount != null ? `, ${eventsCount} eventos` : ""}${durationMs != null ? `, ${Math.round(durationMs / 1000)}s` : ""} — ${state}`;

  return (
    <div
      tabIndex={0}
      role="img"
      aria-label={ariaLabel}
      className={cn(
        "flex flex-col items-center gap-1 min-w-[56px]",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--color-accent-mustard] focus-visible:rounded-sm",
        className,
      )}
      title={`${label}${eventsCount != null ? ` — ${eventsCount} eventos` : ""}`}
    >
      {/* glyph circle */}
      <div
        ref={dotRef}
        className={cn(
          "w-9 h-9 rounded-full flex items-center justify-center border transition-colors",
          state === "future" && "border-border text-muted-foreground/50 bg-transparent",
          state === "active" &&
            "border-[--color-accent-mustard] text-[--color-accent-mustard] bg-[--color-accent-mustard]/10",
          state === "completed" && "border-transparent text-foreground bg-muted",
        )}
      >
        <Icon className="w-4 h-4" />
      </div>

      {/* label */}
      <span
        className={cn(
          "text-[11px] font-medium leading-none",
          state === "future" && "text-muted-foreground/50",
          state === "active" && "text-[--color-accent-mustard]",
          state === "completed" && "text-foreground",
        )}
      >
        {label}
      </span>

      {/* duration + event count */}
      {(durationMs != null || eventsCount != null) && (
        <span
          className="text-[10px] text-muted-foreground tabular-nums leading-none"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {durationMs != null && formatDuration(durationMs)}
          {durationMs != null && eventsCount != null && " · "}
          {eventsCount != null && `${eventsCount}`}
        </span>
      )}
    </div>
  );
}
