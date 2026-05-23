/**
 * AmendActivityBlock — inline sub-block showing pipeline.amend_* events
 * for a specific spec inside the Activity timeline card.
 *
 * Wave 4, spec 2026-05-20-session-bound-amendments, AC-15.
 *
 * Returns null when no amend events exist for the spec — no empty state shown.
 * Indented 24px, soft mustard background (10% opacity), separator line above.
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useStore } from "@/lib/store";
import { useProjects, fetchRecentEvents, type RecentEvent } from "@/lib/dashboard";
import { relativeTime } from "@/lib/time";
import { cn } from "@/lib/utils";

// ── icon map for amend event kinds ──────────────────────────────────────────

const AMEND_ICONS: Record<string, string> = {
  "pipeline.amend_open":    "○",
  "pipeline.amend_capture": "✎",
  "pipeline.amend_close":   "✓",
  "pipeline.amend_drift":   "⚡",
  "pipeline.amend_pending": "⏸",
};

function iconFor(eventType: string): string {
  return AMEND_ICONS[eventType] ?? "·";
}

function labelFor(eventType: string): string {
  const suffix = eventType.replace("pipeline.amend_", "");
  switch (suffix) {
    case "open":    return "Janela aberta";
    case "capture": return "Atividade capturada";
    case "close":   return "Janela fechada";
    case "drift":   return "Drift detectado";
    case "pending": return "Pendente (cross-session)";
    default:        return suffix;
  }
}

// ── component ────────────────────────────────────────────────────────────────

export interface AmendActivityBlockProps {
  /** The spec name / id to filter events for. */
  specId: string;
}

export function AmendActivityBlock({ specId }: AmendActivityBlockProps) {
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const path = projects.find((p) => p.id === activeWorkspaceId)?.path ?? null;

  // Reuse the existing recent-events query key so it benefits from cache.
  const { data: allEvents } = useQuery({
    queryKey: ["recent-events-amend", path],
    queryFn: () => fetchRecentEvents(path!, 500),
    enabled: !!path,
    staleTime: 15_000,
  });

  const amendEvents = useMemo<RecentEvent[]>(() => {
    if (!allEvents) return [];
    return allEvents
      .filter(
        (e) =>
          e.spec === specId &&
          typeof e.event_type === "string" &&
          e.event_type.startsWith("pipeline.amend_"),
      )
      // Sort chronologically (oldest first)
      .sort((a, b) => {
        if (!a.ts && !b.ts) return 0;
        if (!a.ts) return 1;
        if (!b.ts) return -1;
        return a.ts.localeCompare(b.ts);
      });
  }, [allEvents, specId]);

  // No amend events → render nothing (per spec: component returns null)
  if (amendEvents.length === 0) return null;

  return (
    <div
      className={cn(
        "ml-6 mt-2 border-t border-border/30 pt-2",
        "rounded-md px-3 py-2",
        // soft mustard background at 10% opacity via CSS custom prop fallback
      )}
      style={{
        backgroundColor: "color-mix(in srgb, var(--color-accent-mustard, #f5a623) 10%, transparent)",
      }}
    >
      <p className="text-[10px] uppercase tracking-wider text-muted-foreground/60 mb-1.5 font-medium">
        Amend windows
      </p>
      <ul className="flex flex-col gap-1">
        {amendEvents.map((e, i) => (
          <li
            key={`amend-${e.ts ?? ""}-${i}`}
            className="flex items-baseline gap-2 text-[12px] min-w-0"
          >
            <span
              className="shrink-0 font-mono text-[--color-accent-mustard] opacity-70"
              aria-hidden
            >
              {iconFor(e.event_type)}
            </span>
            <span className="text-foreground/80 truncate flex-1">
              {labelFor(e.event_type)}
              {e.summary && (
                <span className="text-muted-foreground/60 ml-1">— {e.summary}</span>
              )}
            </span>
            {e.ts && (
              <span className="shrink-0 text-[11px] text-muted-foreground/50 tabular-nums">
                {relativeTime(e.ts)}
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
