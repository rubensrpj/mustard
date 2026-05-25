// Wave-3 spec-view types — mirrors spec_views.rs shapes.
// Field names are snake_case to match Rust serde output directly.

// ── Lifecycle model (spec-lifecycle-unification W1 `mustard-core`) ────────────
// These mirror `packages/core/src/model/view/spec.rs` 1:1. Both `Stage` and
// `Outcome` serialize kebab-case (`#[serde(rename_all = "kebab-case")]`); the
// `qa-review` spelling and `wave-failed`/`followup-open` flags round-trip
// straight through. `Flags` fields default to `false` (serde `#[serde(default)]`).

/** Lifecycle position — `Stage` in `mustard-core` (kebab-case). */
export type Stage = "analyze" | "plan" | "execute" | "qa-review" | "close";

/** Terminal disposition — `Outcome` in `mustard-core` (kebab-case). */
export type Outcome = "active" | "completed" | "cancelled" | "abandoned";

/** Orthogonal qualifiers — `Flags` in `mustard-core`. All default `false`. */
export interface Flags {
  blocked: boolean;
  wave_failed: boolean;
  followup_open: boolean;
}

/** Canonical lifecycle state — `SpecState` in `mustard-core`. */
export interface SpecState {
  stage: Stage;
  outcome: Outcome;
  flags: Flags;
}

// ── spec-children-tree (W2 `mustard-rt run spec-children-tree`) ───────────────
// Mirrors `apps/rt/src/run/spec_children_tree.rs` 1:1. `WaveChild.status` is a
// `WaveStatus` (kebab-case: queued | in-progress | completed | failed);
// `AcChild.status` is an `AcStatus` (lowercase: pass | fail | skip | pending);
// `SubSpecChild` mirrors `mustard_core::SpecChild` (the `subspecs` element).

/** One wave row — mirrors `WaveChild` in `spec_children_tree.rs`. */
export interface WaveChild {
  idx: number;
  role: string;
  /** queued | in-progress | completed | failed (WaveStatus, kebab-case). */
  status: string;
  started_at: string | null;
  completed_at: string | null;
  duration_ms: number | null;
}

/** One acceptance-criterion row — mirrors `AcChild` in `spec_children_tree.rs`. */
export interface AcChild {
  id: string;
  label: string;
  /** pass | fail | skip | pending (AcStatus, lowercase). */
  status: string;
  last_run_at: string | null;
  evidence: string | null;
}

/** One linked sub-spec — the `subspecs` element (`mustard_core::SpecChild`). */
export interface SubSpecChild {
  spec: string;
  state: SpecState;
  /** Legacy flat status, derived from `state` (kebab-case). */
  status: string;
  started_at: string | null;
  completed_at: string | null;
  reason: string | null;
}

/**
 * Full projection from `mustard-rt run spec-children-tree --spec NAME` —
 * mirrors `ChildrenTree` in `apps/rt/src/run/spec_children_tree.rs`.
 */
export interface ChildrenTree {
  spec: string;
  waves: WaveChild[];
  acs: AcChild[];
  subspecs: SubSpecChild[];
}

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
  /**
   * Number of sub-specs linked to this spec via `spec.link` events. Populated
   * by the `spec_card_v2` adapter so the dashboard's `SpecCard` component can
   * render the `+N sub-specs` badge without fanning out one `useSpecChildren`
   * query per rendered card (spec `2026-05-21-speccard-use-children-count`).
   * Optional for backwards compatibility with payloads emitted before the
   * field existed.
   */
  children_count?: number;
}

/**
 * One sub-spec row linked to a parent. Mirrors `spec_views::SpecChild` on
 * the Rust side. Wave-3 (2026-05-20, spec
 * `2026-05-20-tactical-fix-via-sub-spec`) introduced the shape; Wave-6
 * (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`) added `source` to
 * surface whether the row was discovered via the SQLite `spec.link` event,
 * the filesystem `### Parent:` header, or both.
 */
export interface SpecChild {
  spec: string;
  /** kebab-case lifecycle status (no-events | planning | implementing | …). */
  status: string;
  started_at: string | null;
  completed_at: string | null;
  reason: string | null;
  /**
   * Provenance of this row, surfaced by the Wave-6 union scanner:
   * - `"event"` — found only in the SQLite `spec.link` projection
   * - `"header"` — found only via the on-disk `### Parent:` header scan
   * - `"both"` — present in both sources (the normal case once the event
   *    store has caught up)
   *
   * Optional for backwards compatibility — payloads from the pre-Wave-6
   * Tauri command never populate this field.
   */
  source?: "event" | "header" | "both";
  /**
   * Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`): the parent wave
   * whose execution window contains this child's `started_at`. `null` /
   * `undefined` when the child has no `started_at` (header-only) or its
   * start falls outside every wave window. Rendered as a nested row under
   * the matching wave in the Ondas tab; missing-wave children land in the
   * "Sem onda correlacionada" bucket.
   */
  wave?: number | null;
}

/** Wave-3 — sub-spec summary attached to a `SpecSummary` row. Optional for
 *  backwards compatibility with responses produced before this field existed. */
export interface SpecSummary {
  spec: string;
  status: string;
  /** Number of sub-specs linked to this spec via `spec.link` events. */
  children_count?: number;
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
  /**
   * W5 (`2026-05-24-mustard-unification`, T5.2) — the core projection rewrite
   * adds per-event execution metrics so the timeline can render the
   * claude-devtools style flat list (icon · label · tokens · duration ·
   * status dot · expand to reveal input/output renderer). All fields are
   * optional so the dashboard renders gracefully against pre-T5.2 cores —
   * `null` simply omits the corresponding chip / nested view.
   */
  tokens_in?: number | null;
  tokens_out?: number | null;
  duration_ms?: number | null;
  /** Parent NDJSON line offset / `pipeline_events.id` for `Task` children —
   *  drives the recursive execution-trace nested view. Numeric to match the
   *  core projection (signed integer). */
  parent_id?: number | null;
  /** Tool-specific input. For `Bash` → command string, `Read` → path, `Edit` →
   *  pre-edit snapshot, `Glob`/`Grep` → query, `Task` → subagent prompt. */
  input?: string | null;
  /** Tool-specific output. For `Bash` → stdout/stderr, `Read` → file body
   *  excerpt, `Edit` → diff, `Glob`/`Grep` → result list, `Task` → final reply. */
  output?: string | null;
  /** Raw tool name when `kind === "tool"` (e.g. `"Bash"`, `"Read"`, `"Edit"`,
   *  `"Glob"`, `"Grep"`, `"Task"`). Lets the renderer pick the right viewer. */
  tool?: string | null;
  /** Lifecycle status of this row — drives the status dot. */
  status?: "ok" | "error" | "running" | "warn" | null;
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
  /** `null` when token-savings data is unavailable — render "—" not "0". */
  tokens_saved_today: number | null;
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

/**
 * Wave-6 (2026-05-21, spec `spec-lifecycle-unification/wave-6-observability`) —
 * hygiene health roll-up returned by `workspace_health`. All counts default to 0
 * when the DB is absent (fail-open). Mirrors `spec_views::WorkspaceHealth`.
 */
export interface WorkspaceHealth {
  /** Specs whose last pipeline status is an active/in-progress variant. */
  active: number;
  /** Distinct specs with a `hygiene.detected` event in the last 7 days (still active). */
  suspects: number;
  /** `hygiene.autoclose` events in the last 24 hours. */
  autoclose_today: number;
  /** Active specs flagged as blocked. */
  blocked: number;
  /** Active specs flagged as wave-failed. */
  wave_failed: number;
  /** Active specs in the follow-up window. */
  followup_open: number;
  /** ISO-8601 timestamp of the most recent `hygiene.*` event. */
  last_hygiene_run_at: string | null;
  /** Slug list of suspect specs, for cross-referencing with the main spec list. */
  suspect_specs: string[];
}
