import { useNavigate } from "react-router";
import { cn } from "@/lib/utils";
import type { SpecTrack } from "@/lib/types/specs";

interface SpecTrackRowProps {
  track: SpecTrack;
  className?: string;
}

const PHASE_ORDER = ["analyze", "plan", "execute", "qa", "close"] as const;

/** State marker glyph */
function statusGlyph(status: string): string {
  switch (status) {
    case "completed":
    case "closed":
      return "○";
    case "blocked":
      return "⚠";
    case "cancelled":
      return "⊘";
    default:
      return "●";
  }
}

function statusColor(status: string): string {
  switch (status) {
    case "completed":
    case "closed":
      return "text-muted-foreground";
    case "blocked":
      return "text-[--color-error]";
    case "cancelled":
      return "text-muted-foreground/50";
    default:
      return "text-[--color-accent-mustard]";
  }
}

/** Right indicator */
function RightIndicator({ track }: { track: SpecTrack }) {
  if (track.blocked_reason) {
    return (
      <span className="text-[11px] font-medium text-[--color-error] shrink-0">
        ⚠ BLOCKED
      </span>
    );
  }
  if (track.status === "completed" || track.status === "closed") {
    return (
      <span className="text-[11px] font-medium text-muted-foreground shrink-0">
        ✓ CLOSED
      </span>
    );
  }
  return (
    <span
      className="text-[12px] text-[--color-accent-mustard] shrink-0"
      aria-label="Em execução"
    >
      ▶
    </span>
  );
}

/** 5-segment phase track */
function PhaseTrack({ segments }: { segments: SpecTrack["segments"] }) {
  // Build a map phase→state for quick lookup
  const stateMap = new Map(segments.map((s) => [s.phase.toLowerCase(), s.state]));

  return (
    <div className="flex items-center gap-0.5" role="img" aria-label="Fases da pipeline">
      {PHASE_ORDER.map((phase, i) => {
        const state = stateMap.get(phase) ?? "future";
        return (
          <span key={phase} className="flex items-center">
            {i > 0 && (
              <span
                className={cn(
                  "mx-0.5 text-[10px] leading-none select-none",
                  state === "future" ? "text-border" : "text-muted-foreground/50",
                )}
              >
                {state === "future" ? "─" : "━"}
              </span>
            )}
            <span
              className={cn(
                "text-[11px] leading-none select-none",
                state === "completed" && "text-foreground",
                state === "active" && "text-[--color-accent-mustard]",
                state === "future" && "text-border",
              )}
              title={`${phase}: ${state}`}
            >
              {state === "completed" ? "━" : state === "active" ? "●" : "─"}
            </span>
          </span>
        );
      })}
    </div>
  );
}

export function SpecTrackRow({ track, className }: SpecTrackRowProps) {
  const navigate = useNavigate();

  const waveLabel =
    track.current_wave != null
      ? track.total_waves != null
        ? `onda ${track.current_wave}/${track.total_waves}`
        : `onda ${track.current_wave}`
      : track.current_phase;

  function handleClick() {
    // Navigate to Specs page with this spec expanded via hash
    navigate(`/specs#${encodeURIComponent(track.spec)}`);
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handleClick();
    }
  }

  return (
    <div
      role="button"
      tabIndex={0}
      aria-label={`Spec ${track.spec}, fase ${track.current_phase}, status ${track.status}. Clique para expandir.`}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      className={cn(
        "flex items-center gap-3 px-3 py-2 rounded-md cursor-pointer",
        "hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2",
        "focus-visible:ring-[--color-accent-mustard] focus-visible:rounded-md",
        "transition-colors min-w-0",
        className,
      )}
    >
      {/* State marker */}
      <span
        className={cn("shrink-0 text-[14px] leading-none", statusColor(track.status))}
        aria-hidden
      >
        {statusGlyph(track.status)}
      </span>

      {/* Spec name — truncate at end only, never cut the prefix */}
      <span
        className="font-mono text-[13px] font-medium truncate min-w-0 flex-1"
        title={track.spec}
      >
        {track.spec}
      </span>

      {/* Phase track — 5 segments */}
      <PhaseTrack segments={track.segments} />

      {/* Wave / phase label */}
      <span className="text-[11px] text-muted-foreground shrink-0 tabular-nums hidden sm:block"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {waveLabel}
      </span>

      {/* Right indicator */}
      <RightIndicator track={track} />
    </div>
  );
}
