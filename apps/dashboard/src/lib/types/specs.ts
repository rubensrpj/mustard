// Wave-3 spec-view types — mirrors spec_views.rs shapes.
// Field names are snake_case to match Rust serde output directly.

export interface SpecCard {
  spec: string;
  status: string;
  phase: string;
  scope: string | null;
  started_at: string | null;
  last_event_at: string | null;
  duration_ms: number | null;
  current_wave: number | null;
  total_waves: number | null;
  ac_passed: number;
  ac_total: number;
  files_touched: number;
  tools_used: number;
  model: string | null;
}

export interface SpecWave {
  wave: number;
  role: string | null;
  /** queued | in_progress | completed | failed */
  status: string;
  started_at: string | null;
  completed_at: string | null;
  agent_type: string | null;
  files_changed: number;
}

export interface SpecQualityItem {
  ac_id: string;
  ac_label: string | null;
  /** pass | fail | skip | unknown */
  status: string;
  wave: number | null;
  command: string | null;
  last_run_at: string | null;
  fail_reason: string | null;
}

export interface SpecTimelineNode {
  ts: string;
  /** phase | wave | qa | review | agent | tool | other */
  kind: string;
  label: string;
  phase: string | null;
  wave: number | null;
  payload_summary: string | null;
}

export interface TimelineEvent {
  id: string;
  ts: string;
  phase: string | null;
  spec: string | null;
  agent: string | null;
  summary: string;
}

export interface EventFilter {
  kinds?: string[];
  wave?: number;
  agent?: string;
  q?: string;
}

/** Matches SpecActionKind enum on Rust side (sent as string). */
export type SpecActionKind = "reopen" | "close" | "remove";

export interface SpecAction {
  action: string;
  spec: string;
  result: string;
  message: string | null;
}

export interface PhaseSegment {
  /** analyze | plan | execute | qa | close */
  phase: string;
  /** completed | active | future */
  state: string;
}

export interface SpecTrack {
  spec: string;
  status: string;
  current_phase: string;
  current_wave: number | null;
  total_waves: number | null;
  agents_active: number;
  last_event_at: string | null;
  blocked_reason: string | null;
  segments: PhaseSegment[];
}

export interface WorkspaceAlert {
  /** wave_failed | qa_fail */
  kind: string;
  spec: string;
  wave: number | null;
  message: string;
  ts: string | null;
}

export interface FileCount {
  path: string;
  count: number;
}

export interface WorkspaceSummary {
  events_per_minute: number;
  tokens_saved_today: number;
  specs_active_count: number;
  spec_tracks: SpecTrack[];
  alerts: WorkspaceAlert[];
  top_files_today: FileCount[];
}

/** ContributionCell — reserved for future heatmap grid (spec §259). */
export interface ContributionCell {
  date: string;
  count: number;
}
