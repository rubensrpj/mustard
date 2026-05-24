import { invoke } from "@tauri-apps/api/core";

export interface PipelineSummary {
  spec_name: string;
  phase: string;
  scope: string;
  status: string;
  updated_at: string | null;
}

export interface MetricsSummary {
  total_events: number;
  sessions_recent: number;
  agents_dispatched: number;
  last_event_at: string | null;
  tokens_total: number;
  tokens_today: number;
}

export interface KnowledgeSummary {
  patterns_count: number;
  conventions_count: number;
  high_confidence_count: number;
}

export type SpecBucket = "active" | "completed" | "cancelled";

export interface SpecRow {
  name: string;
  status: string | null;
  phase: string | null;
  started_at: string | null;
  completed_at: string | null;
  affected_files: string[];
  bucket: SpecBucket | null;
  /** When set, this row is a wave child (e.g. wave-2-frontend) of the named
   * parent spec (a wave plan). Children are grouped under the parent in UI. */
  parent: string | null;
}

export interface KnowledgeRow {
  id: string;
  type: string;
  name: string;
  description: string;
  confidence: number;
  source: string | null;
}

export function fetchPipelines(repoPath: string): Promise<PipelineSummary[]> {
  return invoke<PipelineSummary[]>("dashboard_pipelines", { repoPath });
}

export function fetchMetrics(repoPath: string): Promise<MetricsSummary> {
  return invoke<MetricsSummary>("dashboard_metrics", { repoPath });
}

export function fetchKnowledge(repoPath: string): Promise<KnowledgeSummary> {
  return invoke<KnowledgeSummary>("dashboard_knowledge", { repoPath });
}

export interface SubprojectInfo {
  name: string;
  role: string | null;
}

export interface RecipeMeta {
  name: string;
  description: string;
}

export interface SkillMeta {
  name: string;
  description: string;
  source: string;
}

export interface RecentEvent {
  event_type: string;
  ts: string | null;
  summary: string | null;
  spec?: string | null;
  wave?: number | null;
  actor_kind?: string | null;
  actor_id?: string | null;
  tool_name?: string | null;
  target?: string | null;
  phase?: string | null;
}

export function fetchSubprojects(repoPath: string): Promise<SubprojectInfo[]> {
  return invoke<SubprojectInfo[]>("dashboard_subprojects", { repoPath });
}

export function fetchRecipes(repoPath: string): Promise<RecipeMeta[]> {
  return invoke<RecipeMeta[]>("dashboard_recipes", { repoPath });
}

export function fetchSkills(repoPath: string): Promise<SkillMeta[]> {
  return invoke<SkillMeta[]>("dashboard_skills", { repoPath });
}

export function fetchRecentEvents(repoPath: string, limit?: number): Promise<RecentEvent[]> {
  return invoke<RecentEvent[]>("dashboard_recent_events", { repoPath, limit });
}

export function fetchSpecs(repoPath: string): Promise<SpecRow[]> {
  return invoke<SpecRow[]>("dashboard_specs", { repoPath });
}

export function fetchSpecMarkdown(repoPath: string, specName: string): Promise<string> {
  return invoke<string>("dashboard_spec_markdown", { repoPath, specName });
}

export function completeSpec(repoPath: string, specName: string): Promise<SpecBucket> {
  return invoke<SpecBucket>("dashboard_spec_complete", { repoPath, specName });
}

export function cancelSpec(repoPath: string, specName: string): Promise<SpecBucket> {
  return invoke<SpecBucket>("dashboard_spec_cancel", { repoPath, specName });
}

export function reactivateSpec(repoPath: string, specName: string): Promise<SpecBucket> {
  return invoke<SpecBucket>("dashboard_spec_reactivate", { repoPath, specName });
}

export function fetchSearchEvents(
  repoPath: string,
  query: string,
  limit?: number,
): Promise<RecentEvent[]> {
  return invoke<RecentEvent[]>("dashboard_search_events", { repoPath, query, limit });
}

export function fetchSearchKnowledge(
  repoPath: string,
  query: string,
  limit?: number,
): Promise<KnowledgeRow[]> {
  return invoke<KnowledgeRow[]>("dashboard_search_knowledge", { repoPath, query, limit });
}

// --- Telemetry ---

export interface RtkDaily {
  date: string;
  commands: number;
  input_tokens: number;
  output_tokens: number;
  saved_tokens: number;
  savings_pct: number;
}

export interface RtkBlock {
  available: boolean;
  total_commands: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  tokens_saved: number | null;
  savings_pct: number | null;
  total_exec_time_ms: number | null;
  daily: RtkDaily[];
}

export interface MeasuredBlock {
  tokens_total: number;
  tokens_today: number;
}

export interface HookFireCount {
  hook: string;
  fires: number;
  tokens_saved: number;
  most_recent_ts: string | null;
  /** Subset of `fires` / `tokens_saved` within the current session window. */
  session_fires: number;
  session_tokens_saved: number;
}

export interface RoutingByIntent {
  intent: string;
  blocks: number;
  allows: number;
}

export interface RoutingByNote {
  /** "violation" | "no-model-denied" | "no-model-denied-sonnet" | "no-model-advisory" | "passed" | … */
  note: string;
  count: number;
}

export interface RoutingBlock {
  blocks: number;
  allows: number;
  by_intent: RoutingByIntent[];
  by_note: RoutingByNote[];
  /** Subset of `blocks` / `allows` within the current session window. */
  session_blocks: number;
  session_allows: number;
}

export interface PhaseCount {
  phase: string;
  count: number;
}

export interface WorkflowBlock {
  by_phase: PhaseCount[];
}

export interface ToolCount {
  tool_name: string;
  count: number;
}

export interface AgentActivity {
  agent_type: string;
  starts: number;
  stops: number;
  errors: number;
  avg_duration_ms: number;
  last_ts: string | null;
}

export interface AgentActivityBlock {
  total_dispatches: number;
  total_errors: number;
  agents: AgentActivity[];
}

export interface TelemetrySummary {
  rtk: RtkBlock;
  measured: MeasuredBlock;
  prevention: HookFireCount[];
  routing: RoutingBlock;
  workflow: WorkflowBlock;
  tool_breakdown: ToolCount[];
  agent_activity: AgentActivityBlock;
  /** ISO timestamp the current session began emitting, or null. Every
   *  `session_*` counter in this payload counts lines with `ts >=` this. */
  session_start_ts: string | null;
}

export function fetchTelemetry(repoPath: string): Promise<TelemetrySummary> {
  return invoke<TelemetrySummary>("dashboard_telemetry", { repoPath });
}

// --- Friction telemetry (.claude/.metrics/friction.json) ---

/**
 * Measured atrito — hook-retry counts and heavy-pipeline signals. NOT a
 * knowledge pattern: it lives in friction.json and the Knowledge page shows
 * it in a separate "Atrito" section. Usually empty (friction is rare).
 */
export interface FrictionEntry {
  name: string;
  description: string;
  source: string | null;
  tags: string[];
  /** Measured hook-level retries (high-hook-retry entries). */
  retry_count: number | null;
  /** Measured API call count (heavy-pipeline entries). */
  api_calls: number | null;
  prescription: string | null;
  updated_at: string | null;
}

export function fetchFriction(repoPath: string): Promise<FrictionEntry[]> {
  return invoke<FrictionEntry[]>("dashboard_friction", { repoPath });
}

// --- Live activity (events.jsonl tail) ---

export interface PhaseActivity {
  /** "ANALYZE" | "PLAN" | "EXECUTE" | "QA" | "CLOSE" */
  phase: string;
  events_today: number;
  events_last_hour: number;
  events_last_5min: number;
  /** 60 minute buckets, oldest first. */
  minute_buckets: number[];
  last_event_ts: string | null;
  top_tools: ToolCount[];
  last_spec: string | null;
}

export interface LiveActivity {
  last_event_ts: string | null;
  events_today: number;
  events_last_hour: number;
  events_last_5min: number;
  tools_today: ToolCount[];
  /** 60 minute buckets, oldest first (aggregate across phases). */
  minute_buckets: number[];
  current_spec: string | null;
  current_phase: string | null;
  current_wave: number | null;
  is_fresh: boolean;
  /** Always 5 entries in canonical order: ANALYZE, PLAN, EXECUTE, QA, CLOSE. */
  by_phase: PhaseActivity[];
}

export function fetchLiveActivity(repoPath: string): Promise<LiveActivity> {
  return invoke<LiveActivity>("dashboard_live_activity", { repoPath });
}

// Collector health (the unified OTEL badge) lives in `src/api/promptEconomy.ts`
// since it belongs to the economy/freshness domain. Re-exported here so the
// dashboard surface stays the single import site for pages already on it.
export { fetchCollectorHealth, type CollectorHealth } from "@/api/promptEconomy";

// --- Active Pipelines ---

export interface ActivePipeline {
  spec_name: string;
  status: string;
  phase: string;
  current_wave: number | null;
  total_waves: number | null;
  model: string | null;
  has_dispatch_failure: boolean;
  failure_age_ms: number | null;
  tasks_pending: number;
  tasks_in_progress: number;
  tasks_completed: number;
  updated_at: string | null;
}

export function fetchActivePipelines(repoPath: string): Promise<ActivePipeline[]> {
  return invoke<ActivePipeline[]>("dashboard_active_pipelines", { repoPath });
}

// --- New Wave 3 commands ---

export interface ActivityGroup {
  spec: string | null;
  wave: number | null;
  action_kind: string | null;
  count: number;
  min_ts: string | null;
  max_ts: string | null;
  tokens_total: number;
  files_touched: number;
}

export interface RoleQuality {
  role: string;
  pass_at_1: number;
  fix_loops: number;
  samples: number;
}

export interface SlowestWave {
  spec: string | null;
  wave: number | null;
  duration_ms: number;
}

export interface PhaseTokens {
  phase: string;
  input_avg: number;
  output_avg: number;
}

export interface QualityMetrics {
  pass_at_1: number;
  fix_loop_rate: number;
  avg_phase_duration_ms: number;
  by_role: RoleQuality[];
  slowest_waves: SlowestWave[];
  tokens_by_phase: PhaseTokens[];
}

export function fetchActivityAggregated(repoPath: string, limit = 200): Promise<ActivityGroup[]> {
  return invoke<ActivityGroup[]>("dashboard_activity_aggregated", { repoPath, limit });
}

export function fetchQualityMetrics(repoPath: string): Promise<QualityMetrics> {
  return invoke<QualityMetrics>("dashboard_quality_metrics", { repoPath });
}

export type KnowledgeBrowseRow = KnowledgeRow;

export function fetchKnowledgeBrowse(repoPath: string, limit = 500): Promise<KnowledgeRow[]> {
  return invoke<KnowledgeRow[]>("dashboard_knowledge_browse", { repoPath, limit });
}

// --- Consumption & cost ---

export interface ModelUsage {
  model: string;
  calls: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_usd: number;
  /** Share of total tokens, 0..1. */
  pct_tokens: number;
}

export interface AgentUsage {
  agent_type: string;
  calls: number;
  total_tokens: number;
  cost_usd: number;
  pct_tokens: number;
}

export interface SpecUsage {
  spec: string;
  calls: number;
  total_tokens: number;
  cost_usd: number;
}

export interface DailyPoint {
  date: string;
  calls: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_usd: number;
}

export interface ConsumptionSummary {
  tokens_total: number;
  tokens_today: number;
  cost_total_usd: number;
  cost_today_usd: number;
  by_model: ModelUsage[];
  by_agent_type: AgentUsage[];
  top_specs: SpecUsage[];
  daily_series: DailyPoint[];
}

export interface ProjectUsage {
  id: string;
  name: string;
  path: string;
  tokens_total: number;
  tokens_today: number;
  cost_total_usd: number;
  cost_today_usd: number;
  last_activity_ms: number | null;
}

export interface GlobalConsumption {
  tokens_total: number;
  tokens_today: number;
  cost_total_usd: number;
  cost_today_usd: number;
  by_project: ProjectUsage[];
  by_model: ModelUsage[];
  daily_series: DailyPoint[];
  rtk: RtkBlock;
}

export function fetchConsumption(repoPath: string): Promise<ConsumptionSummary> {
  return invoke<ConsumptionSummary>("dashboard_consumption", { repoPath });
}

export function fetchConsumptionGlobal(projectsRoot: string): Promise<GlobalConsumption> {
  return invoke<GlobalConsumption>("dashboard_consumption_global", { projectsRoot });
}

// --- Telemetry aggregation (Wave 7) ---

export type {
  TimeRange,
  PhaseSummary,
  TimelineEvent,
  HeatmapCell,
  HistoryEntry,
  AcceptanceCriterion,
  EffortBreakdown,
  AgentDispatch,
} from "@/lib/types/telemetry";

export function dashboardTelemetryPhases(
  repoPath: string,
  timeRange: string,
): Promise<import("@/lib/types/telemetry").PhaseSummary[]> {
  return invoke("dashboard_telemetry_phases", { repoPath, timeRange });
}

export function dashboardTelemetryTimeline(
  repoPath: string,
  timeRange: string,
  limit?: number,
): Promise<import("@/lib/types/telemetry").TimelineEvent[]> {
  return invoke("dashboard_telemetry_timeline", { repoPath, timeRange, limit });
}

export function dashboardTelemetryHeatmap(
  repoPath: string,
  timeRange: string,
): Promise<import("@/lib/types/telemetry").HeatmapCell[]> {
  return invoke("dashboard_telemetry_heatmap", { repoPath, timeRange });
}

export function dashboardTelemetryHistory(
  repoPath: string,
  timeRange: string,
  limit?: number,
): Promise<import("@/lib/types/telemetry").HistoryEntry[]> {
  return invoke("dashboard_telemetry_history", { repoPath, timeRange, limit });
}

export function dashboardTelemetryCriteria(
  repoPath: string,
  timeRange: string,
): Promise<import("@/lib/types/telemetry").AcceptanceCriterion[]> {
  return invoke("dashboard_telemetry_criteria", { repoPath, timeRange });
}

export function dashboardTelemetryEffort(
  repoPath: string,
  timeRange: string,
): Promise<import("@/lib/types/telemetry").EffortBreakdown> {
  return invoke("dashboard_telemetry_effort", { repoPath, timeRange });
}

export function dashboardTelemetryAgents(
  repoPath: string,
  timeRange: string,
): Promise<import("@/lib/types/telemetry").AgentDispatch[]> {
  return invoke("dashboard_telemetry_agents", { repoPath, timeRange });
}

// --- Economy summary (W7 — 2026-05-20-economia-moat-unification) ---
//
// Thin invoke wrapper for the W7 Tauri command. The scope union maps directly
// onto the Rust `EconomyScopeDto` (internally tagged on `kind` with snake_case
// variant names) — JS literal `{ kind: "project", project: "/..." }` is the
// exact payload serde-deserialize expects on the other side.

export type {
  EconomyScope,
  EconomyScopeKind,
  EconomySummary,
  AgentCost,
  SavingsSource,
  SavingsBySource,
  SavingsBreakdown,
  ContextRoutingMetrics,
} from "@/lib/types/economy";
import type {
  EconomyScope,
  EconomySummary,
  SavingsBreakdown,
  ContextRoutingMetrics,
  SpecCost,
  WaveCost,
} from "@/lib/types/economy";

export function fetchEconomySummary(scope: EconomyScope): Promise<EconomySummary> {
  return invoke<EconomySummary>("dashboard_economy_summary", { scope });
}

export function fetchEconomySavingsBreakdown(scope: EconomyScope): Promise<SavingsBreakdown> {
  return invoke<SavingsBreakdown>("dashboard_economy_savings_breakdown", { scope });
}

export function fetchEconomyContextRouting(scope: EconomyScope): Promise<ContextRoutingMetrics> {
  return invoke<ContextRoutingMetrics>("dashboard_economy_context_routing", { scope });
}

export function fetchEconomyPerSpecCosts(scope: EconomyScope): Promise<SpecCost[]> {
  return invoke<SpecCost[]>("dashboard_economy_per_spec_costs", { scope });
}

export function fetchEconomyPerWaveCosts(scope: EconomyScope): Promise<WaveCost[]> {
  return invoke<WaveCost[]>("dashboard_economy_per_wave_costs", { scope });
}

// --- useProjects hook ---
import { useQuery as _useQuery } from "@tanstack/react-query";
import { discoverProjects as _discoverProjects } from "@/api/discovery";
import { useStore as _useStore } from "@/lib/store";

export interface Project {
  id: string;
  name: string;
  path: string;
  last_activity_ms?: number | null;
}

export function useProjects(): Project[] {
  const projectsRoot = _useStore((s) => s.projectsRoot);
  const { data } = _useQuery({
    queryKey: ["discover", projectsRoot],
    queryFn: () => _discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });
  return (data as Project[] | undefined) ?? [];
}

// --- Amend queries (Wave 4, spec 2026-05-20-session-bound-amendments) ---

/** Resolution rate: fraction of closed amend windows that ended 'archived'. */
export function fetchAmendResolutionRate(repoPath: string): Promise<number> {
  return invoke<number>("amend_resolution_rate", { repoPath });
}

/** Drift rate: fraction of closed amend windows that ended 'closed-amend-drift'. */
export function fetchAmendDriftRate(repoPath: string): Promise<number> {
  return invoke<number>("amend_drift_rate", { repoPath });
}

/** Count of windows carrying cross-session debt (status='closed-amend-pending'). */
export function fetchCrossSessionAmendCount(repoPath: string): Promise<number> {
  return invoke<number>("cross_session_amend_count", { repoPath });
}

/** Duration histogram input: Vec<i64> of millisecond durations for closed windows. */
export function fetchAmendWindowDuration(repoPath: string): Promise<number[]> {
  return invoke<number[]>("amend_window_duration", { repoPath });
}

// --- Wave-6 hygiene health ---

export type { WorkspaceHealth } from "@/lib/types/specs";

/**
 * Fetch the hygiene health roll-up for one project. Never throws — returns
 * all-zeros when the DB is absent (Tauri command is fail-open).
 */
export function fetchWorkspaceHealth(
  repoPath: string,
): Promise<import("@/lib/types/specs").WorkspaceHealth> {
  return invoke("workspace_health", { repoPath });
}

// --- Wave-3 spec-card commands ---

export type {
  SpecCard,
  SpecChild,
  SpecSummary,
  SpecWave,
  SpecQualityItem,
  SpecTimelineNode,
  TimelineEvent as SpecTimelineEvent,
  EventFilter,
  SpecActionKind,
  SpecAction,
  PhaseSegment,
  SpecTrack,
  WorkspaceAlert,
  WorkspaceSummary,
  FileCount as SpecFileCount,
  ContributionCell,
} from "@/lib/types/specs";

export function dashboardSpecCard(
  repoPath: string,
  spec: string,
): Promise<import("@/lib/types/specs").SpecCard> {
  return invoke("dashboard_spec_card", { repoPath, spec });
}

export function dashboardSpecWaves(
  repoPath: string,
  spec: string,
): Promise<import("@/lib/types/specs").SpecWave[]> {
  return invoke("dashboard_spec_waves", { repoPath, spec });
}

export function dashboardSpecQuality(
  repoPath: string,
  spec: string,
): Promise<import("@/lib/types/specs").SpecQualityItem[]> {
  return invoke("dashboard_spec_quality", { repoPath, spec });
}

export function dashboardSpecTimeline(
  repoPath: string,
  spec: string,
): Promise<import("@/lib/types/specs").SpecTimelineNode[]> {
  return invoke("dashboard_spec_timeline", { repoPath, spec });
}

export function dashboardSpecEvents(
  repoPath: string,
  spec: string,
  filter?: import("@/lib/types/specs").EventFilter,
): Promise<import("@/lib/types/specs").TimelineEvent[]> {
  return invoke("dashboard_spec_events", { repoPath, spec, filter });
}

export function dashboardSpecAction(
  repoPath: string,
  spec: string,
  action: import("@/lib/types/specs").SpecActionKind,
): Promise<import("@/lib/types/specs").SpecAction> {
  return invoke("dashboard_spec_action", { repoPath, spec, action });
}

export function dashboardWorkspaceSummary(
  repoPath: string,
): Promise<import("@/lib/types/specs").WorkspaceSummary> {
  return invoke("dashboard_workspace_summary", { repoPath });
}

/**
 * Wave-3 (2026-05-20, spec `2026-05-20-tactical-fix-via-sub-spec`): list
 * sub-specs linked to `parent` via the `spec.link` event. Always resolves —
 * the backend collapses missing rows / DB-unavailable into an empty Vec so
 * the UI renders an empty state.
 */
export function dashboardSpecChildren(
  repoPath: string,
  parent: string,
): Promise<import("@/lib/types/specs").SpecChild[]> {
  return invoke("dashboard_spec_children", { repoPath, parent });
}

/**
 * Wave 3 (spec-lifecycle-unification): fetch the children tree (waves +
 * acceptance criteria + sub-specs) for one spec in a single round-trip. Backed
 * by `mustard-rt run spec-children-tree --spec NAME`. Always resolves — the
 * backend collapses subprocess/parse failures into an empty tree so the
 * expandable row renders a clean empty state instead of throwing.
 */
export function fetchSpecChildrenTree(
  spec: string,
  projectPath: string,
): Promise<import("@/lib/types/specs").ChildrenTree> {
  return invoke("spec_children_tree", { spec, projectPath });
}

// --- Wave-4 metrics wave-status (spec mustard-wave-network-standard) ---

/** One per-wave row returned by `mustard-rt run metrics wave-status`. */
export interface MetricsWaveRow {
  name: string;
  status: string | null;
  tokens_saved: number;
  duration_ms: number;
  retries: number;
  cross_wave_memory_bytes: number;
  model: string | null;
}

/** Parent → waves rollup. `parent` is null when the rt binary failed to spawn. */
export interface MetricsWaveStatus {
  parent: string | null;
  waves: MetricsWaveRow[];
}

/**
 * Wave-4 wrapper for the new `dashboard_metrics_wave_status` Tauri command.
 * Shells out to `mustard-rt run metrics wave-status --spec <name>` on the
 * backend and returns the parsed JSON. Always resolves — the backend swallows
 * subprocess/parse failures into an empty `waves` vec so the UI can render
 * "sem ondas" instead of throwing.
 */
export function dashboardMetricsWaveStatus(
  repoPath: string,
  specName: string,
): Promise<MetricsWaveStatus> {
  return invoke<MetricsWaveStatus>("dashboard_metrics_wave_status", {
    repoPath,
    specName,
  });
}

// --- Wave-3 wikilink graph + cross-wave memory (spec mustard-wave-network-standard) ---

/** One wikilink occurrence emitted by `mustard-rt run wikilink-extract`. */
export interface Wikilink {
  from: string;
  to: string;
  file: string;
  line: number;
}

/**
 * Full payload of `mustard-rt run wikilink-extract`: every `[[name]]`
 * occurrence under the spec dir plus the set of orphan targets (names that
 * don't resolve to a spec file). The dashboard groups these into
 * parent/waves/dependents layers client-side.
 */
export interface WikilinkExtract {
  wikilinks: Wikilink[];
  orphans: string[];
}

/**
 * Wave-3 wrapper for `dashboard_wikilink_extract`. The backend resolves the
 * spec directory under `.claude/spec/{active,completed,cancelled}/<specName>`
 * so the frontend never passes a raw filesystem path. Always resolves —
 * missing dir / unparseable JSON collapse to an empty extract.
 */
export function dashboardWikilinkExtract(
  repoPath: string,
  specName: string,
): Promise<WikilinkExtract> {
  return invoke<WikilinkExtract>("dashboard_wikilink_extract", {
    repoPath,
    specName,
  });
}

/**
 * Wave-3 wrapper for `dashboard_memory_cross_wave`. Returns the markdown
 * payload (stdout) — empty string when no prior wave has recorded memory yet.
 */
export function dashboardMemoryCrossWave(
  repoPath: string,
  spec: string,
  wave: number,
): Promise<string> {
  return invoke<string>("dashboard_memory_cross_wave", { repoPath, spec, wave });
}

// --- Wave-2 (spec 2026-05-21-dashboard-spec-tabs): real file count + wave markdown ---

/**
 * Real file count for a wave + full wave-N markdown so the drawer can render
 * it without a second round-trip. Backed by `mustard-rt run wave-files`.
 * `path` is `null` when the wave sub-spec is missing on disk.
 */
export interface WaveFilesPayload {
  count: number;
  markdown: string;
  path: string | null;
}

export function dashboardSpecWaveFiles(
  path: string,
  spec: string,
  wave: number,
): Promise<WaveFilesPayload> {
  return invoke<WaveFilesPayload>("dashboard_spec_wave_files", {
    repoPath: path,
    spec,
    wave,
  });
}

// --- Wave 1 polish (spec 2026-05-21-dashboard-spec-tabs-polish): planned waves ---
//
// One wave declared on disk under `.claude/spec/{spec}/wave-N-{role}/`. The
// Specs page unions this with the SpecWave[] projection from SQLite so the
// "Ondas" tab can render the full wave plan during EXECUTE — even when the
// SQLite event stream hasn't caught up with wave start/complete events yet.

export interface SpecWavePlanned {
  wave: number;
  role: string | null;
  declared_files_count: number;
}

export function dashboardSpecWavesPlanned(
  repoPath: string,
  spec: string,
): Promise<SpecWavePlanned[]> {
  return invoke<SpecWavePlanned[]>("dashboard_spec_waves_planned", {
    repoPath,
    spec,
  });
}

// --- Wave-2 dashboard visual overview (spec 2026-05-20-dashboard-visual-overview) ---

/** Per-pipeline token savings entry returned in `TokenSummary.top_pipelines`. */
export interface TopPipeline {
  spec: string;
  saved: number;
}

/**
 * Aggregate token-savings payload for the workspace overview cards. Mirrors
 * the Rust `TokenSummary` struct (`serde(rename_all = "snake_case")`).
 */
export interface TokenSummary {
  total_saved: number;
  top_pipelines: TopPipeline[];
}

/** One calendar-day bucket in the monthly activity heatmap. */
export interface DayActivity {
  /** YYYY-MM-DD */
  date: string;
  event_count: number;
  /** Phase with the most events that day, when any phase was tagged. */
  top_phase: string | null;
}

/** One event row in the live workspace feed. */
export interface FeedEvent {
  id: string;
  /** ISO-8601 timestamp. */
  ts: string;
  kind: string;
  spec: string | null;
  payload_summary: string;
}

/** Aggregate token-savings totals + top-N pipelines for the active workspace. */
export function dashboardTokenSummary(projectPath: string): Promise<TokenSummary> {
  return invoke<TokenSummary>("dashboard_token_summary", { projectPath });
}

/** Per-day activity counts for the given month (1..12). */
export function dashboardMonthActivity(
  projectPath: string,
  year: number,
  month: number,
): Promise<DayActivity[]> {
  return invoke<DayActivity[]>("dashboard_month_activity", { projectPath, year, month });
}

/** Most-recent feed events (newest first), capped by `limit`. */
export function dashboardEventsFeed(projectPath: string, limit: number): Promise<FeedEvent[]> {
  return invoke<FeedEvent[]>("dashboard_events_feed", { projectPath, limit });
}

// --- Wave 4 mustard-unification — language + tone settings ----------------
//
// `mustard.json#lang` (BCP-47 `pt-BR`/`en-US`) and `mustard.json#tone`
// (`didactic`/`technical`/`concise`) are written via these Tauri commands so
// the validation + telemetry contract is centralised on the backend.

/** Shape returned by `commands::settings::read_settings`. Both fields are
 *  optional — a fresh project ships `mustard.json` without either. */
export interface ProjectSettings {
  lang: string | null;
  tone: string | null;
}

/** Read `lang` + `tone` from `mustard.json`. Fail-open: a missing or
 *  malformed file resolves to `{ lang: null, tone: null }`. */
export function readSettings(repoPath: string): Promise<ProjectSettings> {
  return invoke<ProjectSettings>("read_settings", { repoPath });
}

/** Write `mustard.json#lang` after validating against the BCP-47 catalog
 *  (`pt-BR` / `en-US`). Rejects legacy short forms with a typed error. */
export function setLanguage(repoPath: string, lang: string): Promise<void> {
  return invoke<void>("set_language", { repoPath, lang });
}

/** Write `mustard.json#tone` after validating against the catalog
 *  (`didactic` / `technical` / `concise`). */
export function setTone(repoPath: string, tone: string): Promise<void> {
  return invoke<void>("set_tone", { repoPath, tone });
}
