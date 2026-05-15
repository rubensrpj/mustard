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
}

export function fetchTelemetry(repoPath: string): Promise<TelemetrySummary> {
  return invoke<TelemetrySummary>("dashboard_telemetry", { repoPath });
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
