import { useNavigate } from "react-router";
import { cn } from "@/lib/utils";
import { PhaseStation } from "../telemetry/PhaseStation";
import type { SpecTrack } from "@/lib/types/specs";
import type { PhaseStationProps } from "../telemetry/PhaseStation";

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

/**
 * Compact phase track — one [`PhaseStation`] per phase (ANALYZE → PLAN →
 * EXECUTE → QA → CLOSE). Replaces the previous ASCII glyph track; reuses the
 * same component the `PipelineTimeline` uses so the visual language stays
 * consistent across Visão Geral and drill-down. Density-reduced (no
 * duration/event counts, no labels — handled by PhaseStation's defaults).
 */
function PhaseTrack({ segments }: { segments: SpecTrack["segments"] }) {
  const stateMap = new Map(
    segments.map((s) => [s.phase.toLowerCase(), s.state]),
  );

  return (
    <div
      className="flex items-center gap-1"
      role="img"
      aria-label="Fases da pipeline"
    >
      {PHASE_ORDER.map((phase) => {
        const raw = stateMap.get(phase) ?? "future";
        const state: PhaseStationProps["state"] =
          raw === "completed" || raw === "active" ? raw : "future";
        return (
          <PhaseStation
            key={phase}
            phase={phase}
            state={state}
            className="min-w-0 gap-0 [&_span]:hidden [&>div]:w-5 [&>div]:h-5 [&_svg]:w-3 [&_svg]:h-3"
          />
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
