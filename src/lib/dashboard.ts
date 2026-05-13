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

export interface SpecRow {
  name: string;
  status: string | null;
  phase: string | null;
  started_at: string | null;
  completed_at: string | null;
  affected_files: string[];
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
