// Telemetry aggregation types — mirroring telemetry_agg.rs shapes.
// All numeric fields from Rust i64 become number in TS.

export type TimeRange = "today" | "7d" | "30d" | "all";

export interface PhaseSummary {
  phase: string;
  events_count: number;
  last_event_at: string | null;
  /** 7-slot sparkline, oldest day first. */
  sparkline: number[];
}

export interface TimelineEvent {
  id: string;
  ts: string;
  phase: string | null;
  spec: string | null;
  agent: string | null;
  summary: string;
}

export interface HeatmapCell {
  /** 0 = Sunday … 6 = Saturday */
  day_of_week: number;
  /** 0–23 */
  hour: number;
  event_count: number;
}

export interface HistoryEntry {
  spec: string;
  status: string;
  started_at: string;
  completed_at: string | null;
  /** phase label → event count */
  duration_per_phase: Record<string, number>;
  ac_passed: number;
  ac_total: number;
}

export interface AcceptanceCriterion {
  spec: string;
  id: string;
  status: string;
  last_run_at: string | null;
}

export interface FileCount {
  path: string;
  count: number;
}

export interface ToolUseCount {
  name: string;
  count: number;
}

export interface PhaseEventCount {
  phase: string;
  duration_ms: number;
}

export interface AgentTypeCount {
  agent_type: string;
  count: number;
}

export interface EffortBreakdown {
  top_files: FileCount[];
  top_tools: ToolUseCount[];
  top_phases: PhaseEventCount[];
  top_agents: AgentTypeCount[];
}

export interface AgentDispatch {
  subagent_type: string;
  count: number;
  error_count: number;
  avg_duration_ms: number;
  last_dispatched_at: string | null;
}
