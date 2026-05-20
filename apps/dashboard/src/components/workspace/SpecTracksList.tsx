import { useMemo } from "react";
import { cn } from "@/lib/utils";
import { SpecTrackRow } from "./SpecTrackRow";
import type { SpecTrack } from "@/lib/types/specs";

interface SpecTracksListProps {
  tracks: SpecTrack[];
  className?: string;
}

const RECENT_CLOSED_MS = 24 * 60 * 60 * 1000; // 24 h

function isRecentlyClosed(track: SpecTrack): boolean {
  if (!["completed", "closed"].includes(track.status)) return false;
  if (!track.last_event_at) return false;
  const diff = Date.now() - new Date(track.last_event_at).getTime();
  return diff <= RECENT_CLOSED_MS;
}

function isActive(track: SpecTrack): boolean {
  return !["completed", "closed", "cancelled"].includes(track.status);
}

export function SpecTracksList({ tracks, className }: SpecTracksListProps) {
  const { active, recentClosed } = useMemo(() => {
    const sorted = [...tracks].sort((a, b) => {
      const ta = a.last_event_at ? new Date(a.last_event_at).getTime() : 0;
      const tb = b.last_event_at ? new Date(b.last_event_at).getTime() : 0;
      return tb - ta; // most recent first
    });

    return {
      active: sorted.filter(isActive),
      recentClosed: sorted.filter(isRecentlyClosed),
    };
  }, [tracks]);

  if (active.length === 0 && recentClosed.length === 0) {
    return (
      <p className="text-[13px] text-muted-foreground px-3 py-4">
        Nenhuma spec ativa nas últimas 24h.
      </p>
    );
  }

  return (
    <div className={cn("flex flex-col gap-0.5", className)}>
      {active.map((t) => (
        <SpecTrackRow key={t.spec} track={t} />
      ))}

      {recentClosed.length > 0 && (
        <>
          {active.length > 0 && (
            <div className="border-t border-border/40 my-1" aria-hidden />
          )}
          {recentClosed.map((t) => (
            <SpecTrackRow key={t.spec} track={t} />
          ))}
        </>
      )}
    </div>
  );
}
