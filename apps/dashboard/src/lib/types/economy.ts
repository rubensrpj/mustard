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

/**
 * Mirror of `mustard_core::domain::economy::TimeWindow` (serde). Both bounds are
 * optional ISO-8601 strings; an absent bound is unbounded on that side. We OMIT
 * a bound (rather than send `null`) to match serde's
 * `skip_serializing_if = "Option::is_none"`, so `{ from }` alone is the wire
 * shape for a "last N days" window — unbounded above means "up to now".
 */
export interface TimeWindow {
  from?: string;
  to?: string;
}

/** The four fixed periods the Economia time-window selector offers. */
export type EconomyWindowPeriod = "1d" | "7d" | "15d" | "30d";

/**
 * Wire shape for an economy `invoke`: a base `EconomyScope` optionally wrapped
 * in a time window, mirroring `EconomyScopeDto::Windowed { window, inner }` on
 * the Rust side. Composition, not replacement — `inner` still decides the
 * project/spec/wave slice; `window` only narrows which events fold in (each
 * backend reader peels it back off via `into_parts()`). Built at the invoke
 * boundary via `windowedScope`; the base `EconomyScope` that `<ScopeBar>` drives
 * never carries a window itself, so the scope switch/match sites stay total.
 */
export type EconomyScopeWire =
  | EconomyScope
  | { kind: "windowed"; window: TimeWindow; inner: EconomyScope };

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
  /**
   * Epoch-ms of the last ESTIMATED row in `run_usage`. `null` at spec/wave
   * scope or when the table is empty. Compared against `last_updated_ms` to
   * detect stalled OTEL ingestion — a large positive `last_updated_ms -
   * last_estimated_ms` means measured counters kept advancing while the
   * per-spec estimator went silent.
   */
  last_estimated_ms: number | null;
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
  | "budget_output_cut";

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

/**
 * ESTIMATED per-spec cost row — sourced from self-attributed `run_usage`, NOT
 * from Anthropic's billed `usage_totals`. Matches
 * `mustard_core::economy::SpecCost`. UI labels every value as "estimado".
 */
export interface SpecCost {
  spec_id: string;
  cost_usd_micros: number;
  tokens: number;
  span_count: number;
  /**
   * Epoch-ms of MAX(started_at) for the spec — used by UI for descending sort.
   * Optional/null because the field is being rolled out aditively by a parallel
   * backend change; when absent, the UI falls back to lexical sort on spec_id
   * (Mustard slugs are date-prefixed `YYYY-MM-DD-*`, so reverse sort by id is
   * a chronological-enough proxy until the wire field lands).
   */
  last_started_at?: number | null;
}

/**
 * ESTIMATED per-wave cost row. Carries both `spec_id` and `wave_id` so the UI
 * can group rows by spec. Matches `mustard_core::economy::WaveCost`.
 */
export interface WaveCost {
  spec_id: string;
  wave_id: string;
  cost_usd_micros: number;
  tokens: number;
  span_count: number;
  /** Epoch-ms of MAX(started_at) for the wave — optional, sort fallback. */
  last_started_at?: number | null;
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

/**
 * Wrap a base scope in a time window for the wire (mirrors the Rust
 * `EconomyScopeDto::Windowed` composition). Kept a constructor like the others
 * so callers don't hand-build the `{ kind: "windowed", … }` literal — the base
 * scope stays untouched and only the events narrow by timestamp.
 */
export function windowedScope(window: TimeWindow, inner: EconomyScope): EconomyScopeWire {
  return { kind: "windowed", window, inner };
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

/**
 * Format a micro-USD value as `$1.234`, `$0.012`, `$0.000123`, `$0.00`.
 *
 * The 6-decimal band (<$0.0001) exists for the per-spec estimate rows: with
 * cache-heavy traffic, a freshly attributed dispatch can land on the order of
 * tens of micro-USD. Truncating to "$0.00" / "—" hides real signal — the user
 * was specifically confused by an i18n-migration spec showing 2.5k tokens but
 * a missing cost. We render six decimals so cents-of-a-cent stays visible
 * instead of vanishing. Use `formatUsdOrDash` when "missing data" should still
 * read as an explicit em-dash.
 */
export function formatUsd(microsUsd: number): string {
  const usd = microsToUsd(microsUsd);
  if (usd === 0) return "$0.00";
  if (usd < 0.0001) return `$${usd.toFixed(6)}`;
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
