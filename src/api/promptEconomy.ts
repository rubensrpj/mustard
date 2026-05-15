import { invoke } from "@tauri-apps/api/core";

/**
 * Honest prompt-economy payload (Wave 5).
 *
 * Three independently-measured blocks plus a freshness signal:
 *  - `cost`         — USD from Claude Code's native OTEL stream (Anthropic-measured)
 *  - `subtractions` — bytes Mustard chose NOT to send (counterfactual savings)
 *  - `claude_events`— operational counters (sessions, active time)
 *  - `freshness`    — drives the green/amber/red badge and canary tail
 *
 * Returned by the `dashboard_prompt_economy` Tauri command.
 */
export type PromptEconomy = {
  cost: {
    usd_total: number;
    by_model: { model: string; usd: number }[];
    by_session: { session_id: string; usd: number }[];
  };
  subtractions: {
    wave_slice_bytes: number;
    wave_slice_count: number;
    diff_vs_full_bytes: number;
    diff_vs_full_count: number;
    review_diff_first_bytes: number;
    review_diff_first_count: number;
    analyze_diff_skip_bytes: number;
    analyze_diff_skip_count: number;
    // Lifetime totals above are an append-only accumulator. These count only
    // subtraction events inside the current session window; `session_known`
    // is false when no session window could be derived.
    session_bytes: number;
    session_count: number;
    session_known: boolean;
  };
  claude_events: {
    session_count: number;
    active_time_seconds: number;
  };
  freshness: {
    last_metric_ts: string | null;
    last_subtraction_ts: string | null;
    otel_healthy: boolean;
    canary_tail: string[] | null;
  };
};

export function fetchPromptEconomy(repoPath: string): Promise<PromptEconomy> {
  return invoke<PromptEconomy>("dashboard_prompt_economy", { repoPath });
}

/**
 * Unified OTEL collector badge state. Single source of truth — the Rust
 * `collector_health` command applies one rule for every page, so Telemetry,
 * Prompt Economy and any future Economy section show the same state at once.
 * Replaces the page-local `deriveBadge` heuristic.
 */
export type CollectorHealth = "live" | "stale" | "off";

export function fetchCollectorHealth(repoPath: string): Promise<CollectorHealth> {
  return invoke<CollectorHealth>("collector_health", { repoPath });
}
