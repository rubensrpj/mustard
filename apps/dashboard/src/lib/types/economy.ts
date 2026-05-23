// Mirror of `mustard_core::economy` shapes exposed via the
// `dashboard_economy_summary` Tauri command (W7 of
// 2026-05-20-economia-moat-unification).
//
// Kept in sync with `packages/core/src/economy/model.rs` and
// `apps/dashboard/src-tauri/src/telemetry.rs::EconomyScopeDto`. When the core
// shape evolves, update this file in lockstep — both sides use `serde`
// snake_case so the wire format is the same on both ends.

/**
 * Discriminated union matching the Tauri DTO `EconomyScopeDto` (internally
 * tagged on `kind` with snake_case variant names). Use the type-narrowing
 * helpers below to construct scopes instead of building literal objects, so a
 * later variant doesn't silently slip past the compiler.
 */
export type EconomyScope =
  | { kind: "project"; project: string }
  | { kind: "spec"; project: string; spec: string }
  | { kind: "wave"; project: string; spec: string; wave: string }
  | { kind: "all_projects"; projects: string[] };

/** Stable scope kinds used by UI components for switch/match. */
export type EconomyScopeKind = EconomyScope["kind"];

/** Per-agent cost row. Matches `mustard_core::economy::AgentCost`. */
export interface AgentCost {
  /** Agent id. Backed by `AgentId(String)` which serializes transparently. */
  agent_id: string;
  cost_usd_micros: number;
  tokens: number;
  span_count: number;
}

/**
 * Top-level summary the W7 Economia page renders. Matches
 * `mustard_core::economy::EconomySummary` exactly — every monetary value is
 * micro-USD (cost_usd = value / 1_000_000), every token count is an integer.
 */
export interface EconomySummary {
  total_cost_usd_micros: number;
  total_tokens: number;
  total_tokens_saved: number;
  span_count: number;
  /** Top 3 agents ordered by `cost_usd_micros` desc (truncated to <= 3). */
  top_agents_by_cost: AgentCost[];
  /**
   * MEASURED cost per session (`usage_totals.cost.usage`), ordered by USD desc.
   * Populated ONLY at project / all-projects scope — empty at spec/wave scope.
   * Lets the user match one session against Claude Code's `/cost`.
   */
  by_session: SessionCost[];
  /**
   * Epoch-ms of the last MEASURED counter refresh (`MAX(usage_totals.updated_at)`).
   * `null` at spec/wave scope or when no measured row exists.
   */
  last_updated_ms: number | null;
}

/**
 * MEASURED cost for one session, in USD (NOT micro-USD — sourced from the
 * `cost.usage` float counter). Matches `mustard_core::economy::SessionCost`.
 *
 * `last_at_ms` is epoch-ms (same units as `EconomySummary.last_updated_ms`) of
 * the most recent usage row for this session; `null` when telemetry has no
 * `usage_totals` row for the session yet. `specs` are the distinct specs
 * touched during the session (empty when none were recorded).
 */
export interface SessionCost {
  session_id: string;
  usd: number;
  last_at_ms: number | null;
  specs: string[];
}

/** Stable snake_case keys for `SavingsSource` (`mustard_core::economy`). */
export type SavingsSource =
  | "rtk_rewrite"
  | "model_routing_downgrade"
  | "bash_guard_block"
  | "budget_output_cut"
  | "recipe_injection";

/** One row of the savings breakdown, keyed by intervention. */
export interface SavingsBySource {
  source: SavingsSource;
  tokens_saved: number;
  event_count: number;
}

/**
 * Per-`SavingsSource` breakdown. Matches
 * `mustard_core::economy::SavingsBreakdown`. The W7 `<SavingsBreakdownCard>`
 * renders one `<BaseRow>` per source ordered by `tokens_saved` desc.
 */
export interface SavingsBreakdown {
  total_tokens_saved: number;
  per_source: SavingsBySource[];
}

/**
 * Context-routing quality metrics. Every ratio is permille (0..1000) on the
 * wire — divide by 1000 for a `[0, 1]` ratio when rendering. Matches
 * `mustard_core::economy::ContextRoutingMetrics`.
 */
export interface ContextRoutingMetrics {
  prefix_stable_ratio_permille: number;
  cache_hit_ratio_permille: number;
  retry_overhead_ratio_permille: number;
  frame_count: number;
}

// ── Scope constructors ──────────────────────────────────────────────────────
//
// Small helpers so consumer components don't have to remember the variant
// shape. They're cheap and keep the kind/projects discriminant honest.

export function projectScope(project: string): EconomyScope {
  return { kind: "project", project };
}

export function specScope(project: string, spec: string): EconomyScope {
  return { kind: "spec", project, spec };
}

export function waveScope(project: string, spec: string, wave: string): EconomyScope {
  return { kind: "wave", project, spec, wave };
}

export function allProjectsScope(projects: string[]): EconomyScope {
  return { kind: "all_projects", projects };
}

// ── Display helpers ─────────────────────────────────────────────────────────
//
// Co-locating these with the types keeps callers honest about the units: the
// wire format is micro-USD, the display format is USD. A single source of
// truth for the conversion prevents a future page from dividing by 1000 by
// accident.

/** Convert micro-USD to USD (1_000_000 micro-USD = $1). */
export function microsToUsd(microsUsd: number): number {
  return microsUsd / 1_000_000;
}

/** Format a micro-USD value as `$1.234`, `$0.012`, `$0.00` … */
export function formatUsd(microsUsd: number): string {
  const usd = microsToUsd(microsUsd);
  if (usd === 0) return "$0.00";
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  if (usd < 1) return `$${usd.toFixed(3)}`;
  return `$${usd.toFixed(2)}`;
}

/** Format a token integer with k / M suffix for dense UI rows. */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  if (n < 1_000) return String(n);
  if (n < 1_000_000) return `${(n / 1_000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
