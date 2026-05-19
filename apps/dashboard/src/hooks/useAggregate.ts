import { useQueries } from "@tanstack/react-query";
import {
  fetchSpecs,
  fetchRecentEvents,
  type SpecRow,
  type RecentEvent,
} from "@/lib/dashboard";
import type { Project } from "@/api/discovery";

export interface AggregateCounters {
  activeSpecs: number;
  executing: number;
  completed7d: number;
  eventsToday: number;
}

export interface ActivePipelineRow {
  projectId: string;
  projectName: string;
  projectPath: string;
  spec: SpecRow;
}

export interface TimelineRow {
  projectId: string;
  projectName: string;
  event: RecentEvent;
}

interface AggregateResult {
  counters: AggregateCounters;
  activePipelines: ActivePipelineRow[];
  timeline: TimelineRow[];
  loading: boolean;
}

const ACTIVE_PHASES = new Set(["ANALYZE", "PLAN", "EXECUTE", "QA"]);

function startOfTodayMs(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function sevenDaysAgoMs(): number {
  return Date.now() - 7 * 24 * 60 * 60 * 1000;
}

function isActive(spec: SpecRow): boolean {
  if (spec.status === "blocked") return true;
  if (spec.phase && ACTIVE_PHASES.has(spec.phase)) return true;
  return false;
}

function tsMs(s: string | null | undefined): number | null {
  if (!s) return null;
  const t = Date.parse(s);
  return Number.isFinite(t) ? t : null;
}

export function useAggregate(projects: Project[]): AggregateResult {
  const specsQueries = useQueries({
    queries: projects.map((p) => ({
      queryKey: ["specs", p.path],
      queryFn: () => fetchSpecs(p.path),
      staleTime: 30_000,
    })),
  });

  const eventsQueries = useQueries({
    queries: projects.map((p) => ({
      queryKey: ["recent-events", p.path, 10],
      queryFn: () => fetchRecentEvents(p.path, 10),
      staleTime: 15_000,
    })),
  });

  const loading =
    specsQueries.some((q) => q.isLoading) || eventsQueries.some((q) => q.isLoading);

  const counters: AggregateCounters = {
    activeSpecs: 0,
    executing: 0,
    completed7d: 0,
    eventsToday: 0,
  };

  const activePipelines: ActivePipelineRow[] = [];
  const timeline: TimelineRow[] = [];

  const todayStart = startOfTodayMs();
  const sevenDaysAgo = sevenDaysAgoMs();

  projects.forEach((p, i) => {
    const specs = specsQueries[i]?.data ?? [];
    for (const s of specs) {
      if (isActive(s)) {
        counters.activeSpecs++;
        activePipelines.push({
          projectId: p.id,
          projectName: p.name,
          projectPath: p.path,
          spec: s,
        });
      }
      if (s.phase === "EXECUTE") counters.executing++;
      const completedMs = tsMs(s.completed_at);
      if (completedMs !== null && completedMs >= sevenDaysAgo) counters.completed7d++;
    }

    const events = eventsQueries[i]?.data ?? [];
    for (const e of events) {
      const eMs = tsMs(e.ts);
      if (eMs !== null && eMs >= todayStart) counters.eventsToday++;
      timeline.push({ projectId: p.id, projectName: p.name, event: e });
    }
  });

  activePipelines.sort((a, b) => {
    const aT = tsMs(a.spec.started_at) ?? 0;
    const bT = tsMs(b.spec.started_at) ?? 0;
    return bT - aT;
  });

  timeline.sort((a, b) => {
    const aT = tsMs(a.event.ts) ?? 0;
    const bT = tsMs(b.event.ts) ?? 0;
    return bT - aT;
  });

  return {
    counters,
    activePipelines,
    timeline: timeline.slice(0, 20),
    loading,
  };
}
