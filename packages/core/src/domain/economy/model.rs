//! Pure data types for the economy domain.
//!
//! Mirrors the `model` ↔ `store` split the rest of the crate enforces: these
//! types are pure `serde` records with zero side effects — the writer in
//! [`super::writer`] turns them into SQL, the reader in [`super::reader`]
//! returns them. No I/O, no logging, no panics.
//!
//! ## Money
//!
//! Every monetary value travels as a signed micro-USD count (`i64`):
//! `cost_usd_micros = round(cost_usd * 1_000_000)`. This is the same trick the
//! Anthropic billing pipeline uses internally — fixed-point integers are
//! drift-free under addition (no float epsilon accumulation across thousands of
//! requests) and a single `i64` can express ±$9.2 trillion, so overflow is not
//! a real concern. Conversion to a display string is the UI layer's job, not
//! ours.
//!
//! ## Lenient serde
//!
//! [`SpanRecord`] / [`SavingsRecord`] / [`ContextCostFrame`]
//! all carry an `#[serde(flatten)] extra: Map<String, Value>` field. External
//! adapters (OTEL in W3, JSONL in W3) frequently add fields the core domain
//! does not know about; capturing them in `extra` lets a downstream consumer
//! (e.g. dashboard drill-downs) recover the original payload without forcing
//! the core to add a column per signal.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::scope::{AgentId, ProjectPath, SpecId, WaveId};

/// One frame of Anthropic API cost — one request's worth of token usage and
/// (when known) its priced-in micro-USD total.
///
/// The shape mirrors the columns the harness `spans` projection already stores,
/// so the writer can INSERT directly into that table without an extra schema.
/// Fields are public so call sites compose with struct-init syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanRecord {
    /// ISO-8601 wall-clock timestamp of the request.
    pub ts: String,
    /// Session id the request belongs to (correlates with `events.session_id`).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Span id (Anthropic request id or a synthesized one).
    pub span_id: String,
    /// Anthropic model name (e.g. `"claude-opus-4-7"`).
    #[serde(default)]
    pub model: Option<String>,
    /// Spec the request was attributed to.
    #[serde(default)]
    pub spec: Option<String>,
    /// Pipeline phase active when the request fired.
    #[serde(default)]
    pub phase: Option<String>,
    /// Input tokens billed by Anthropic.
    #[serde(default)]
    pub input_tokens: Option<i64>,
    /// Output tokens billed by Anthropic.
    #[serde(default)]
    pub output_tokens: Option<i64>,
    /// Cache-read input tokens (charged at the discounted cache rate).
    #[serde(default)]
    pub cache_read_input_tokens: Option<i64>,
    /// Cache-creation input tokens (charged at the cache-write surcharge).
    #[serde(default)]
    pub cache_creation_input_tokens: Option<i64>,
    /// Priced cost of the request, in micro-USD. See module-level docs.
    #[serde(default)]
    pub cost_usd_micros: Option<i64>,
    /// Whether the request itself errored (HTTP 4xx/5xx).
    #[serde(default)]
    pub is_error: bool,
    /// Catch-all for adapter-specific fields not in the core schema.
    ///
    /// **W4 attribution channel:** adapters populate `extra["tool_use_id"]`
    /// (as a JSON string) when the upstream payload exposes the Anthropic
    /// `tool_use` block id. The writer pulls this out and persists it into the
    /// `spans.tool_use_id` column (migration v4), which the reader joins
    /// against `events.payload.$.tool_use_id` for primary attribution. Keeping
    /// the field in `extra` instead of bumping the struct shape preserves the
    /// W1-frozen `SpanRecord` API for downstream crates that struct-init it.
    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

/// Where a [`SavingsRecord`] originated. The dashboard groups savings by this
/// enum, so each variant maps 1:1 to a column in the W5 UI breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SavingsSource {
    /// Tokens saved by `rtk` rewriting a verbose command into its summary form.
    RtkRewrite,
    /// Tokens saved by `model_routing` downgrading a Task from Opus to a
    /// cheaper tier (only counted when the downgrade is actually safe, never
    /// for blocked upgrades).
    ModelRoutingDowngrade,
    /// Tokens saved by `bash_guard` blocking a destructive or noisy command
    /// (counts the would-be reply tokens we never paid for).
    BashGuardBlock,
    /// Tokens saved by `budget` truncating a Task's return payload before it
    /// re-entered the parent context.
    BudgetOutputCut,
    /// Tokens proxied by `recipe-match` handing the agent a 90%-complete
    /// skeleton instead of forcing it to derive the same structure from
    /// scratch. The skeleton character count is divided by 4 to estimate the
    /// tokens the model did not have to emit.
    RecipeInjection,
    /// Tokens NOT spent on the `scan` cold-path model round-trip because the
    /// deterministic structural extractor (tree-sitter + Aho-Corasick floor)
    /// recovered the subproject's entities/enums offline. The baseline is the
    /// estimated prompt + response token cost of the `interpret` call that the
    /// default-OFF path never made; the Rust cost is ~0.
    ScanStructuralExtract,
    /// Tokens NOT emitted by a model because the deterministic scan generator
    /// rendered the per-subproject scaffold documents (`SKILL.md`, `stack.md`,
    /// and the CLAUDE.md `## Guards` section) in Rust instead of dispatching an
    /// LLM to write them. The
    /// baseline is a chars-per-token (`/4`) proxy over the total bytes of the
    /// generated documents; the Rust cost is ~0.
    ScanSkillRender,
}

impl SavingsSource {
    /// Stable string used as the SQL column value and the dashboard key.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RtkRewrite => "rtk_rewrite",
            Self::ModelRoutingDowngrade => "model_routing_downgrade",
            Self::BashGuardBlock => "bash_guard_block",
            Self::BudgetOutputCut => "budget_output_cut",
            Self::RecipeInjection => "recipe_injection",
            Self::ScanStructuralExtract => "scan_structural_extract",
            Self::ScanSkillRender => "scan_skill_render",
        }
    }

    /// Parse the inverse of [`SavingsSource::as_str`].
    ///
    /// Unknown strings return `None`; callers fail open by treating that as a
    /// `bash_guard_block` was-noise event would be lost rather than mis-typed.
    #[must_use]
    pub fn from_str_opt(raw: &str) -> Option<Self> {
        Some(match raw {
            "rtk_rewrite" => Self::RtkRewrite,
            "model_routing_downgrade" => Self::ModelRoutingDowngrade,
            "bash_guard_block" => Self::BashGuardBlock,
            "budget_output_cut" => Self::BudgetOutputCut,
            "recipe_injection" => Self::RecipeInjection,
            "scan_structural_extract" => Self::ScanStructuralExtract,
            "scan_skill_render" => Self::ScanSkillRender,
            _ => return None,
        })
    }
}

/// One savings event — "this many tokens were not spent because Mustard
/// intervened in this way".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavingsRecord {
    /// ISO-8601 wall-clock timestamp.
    pub ts: String,
    /// Which Mustard subsystem produced the saving.
    pub source: SavingsSource,
    /// Token count saved (always positive; zero records are not interesting).
    pub tokens_saved: i64,
    /// Model the saving would have been spent on (so the dashboard can price
    /// the saving in micro-USD via the W1 pricing table).
    #[serde(default)]
    pub model_target: Option<String>,
    /// Project root the saving is attributed to.
    pub project_path: ProjectPath,
    /// Spec the saving is attributed to (when known).
    #[serde(default)]
    pub spec_id: Option<SpecId>,
    /// Wave the saving is attributed to (when known).
    #[serde(default)]
    pub wave_id: Option<WaveId>,
    /// Agent the saving is attributed to (when known).
    #[serde(default)]
    pub agent_id: Option<AgentId>,
    /// Catch-all for adapter-specific fields.
    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

/// Composition of the prompt sent to a single agent invocation.
///
/// Tracked so the W5 dashboard can answer "are we paying for the same prefix
/// twice?" (prefix-stable ratio), "is wave-slice doing its job?"
/// (`slice_bytes` / `prompt_size_bytes`), and "how much of my token spend is
/// retry overhead?" (`retry_overhead_bytes` / `prompt_size_bytes`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCostFrame {
    /// ISO-8601 wall-clock timestamp of the dispatch.
    pub ts: String,
    /// Agent that received the prompt.
    pub agent_id: AgentId,
    /// Wave the dispatch belongs to.
    #[serde(default)]
    pub wave_id: Option<WaveId>,
    /// Spec the dispatch belongs to.
    #[serde(default)]
    pub spec_id: Option<SpecId>,
    /// Project root that owns the dispatch.
    pub project_path: ProjectPath,
    /// Total prompt size in bytes (UTF-8).
    #[serde(default)]
    pub prompt_size_bytes: Option<i64>,
    /// Bytes counted as PREFIX-STABLE (cacheable, identical across invocations).
    #[serde(default)]
    pub prefix_stable_bytes: Option<i64>,
    /// Bytes of the per-task slice (the task-specific instructions).
    #[serde(default)]
    pub slice_bytes: Option<i64>,
    /// Bytes of the wave-specific slice (the cross-task wave context).
    #[serde(default)]
    pub wave_slice_bytes: Option<i64>,
    /// Bytes returned by the agent into the parent context.
    #[serde(default)]
    pub return_size_bytes: Option<i64>,
    /// Bytes spent re-dispatching due to a failed gate (retry tax).
    #[serde(default)]
    pub retry_overhead_bytes: Option<i64>,
    /// Catch-all for adapter-specific fields.
    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// Aggregate types returned by the reader.
// ---------------------------------------------------------------------------

/// Top-level summary the dashboard's hero card reads.
//
// No `Eq`: `by_session` carries `f64` USD values (no total order), so the
// struct is `PartialEq` only. Reader tests use `assert_eq!`, which only needs
// `PartialEq`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EconomySummary {
    /// Total cost in micro-USD across every span in scope.
    pub total_cost_usd_micros: i64,
    /// Total input + output tokens across every span in scope.
    pub total_tokens: i64,
    /// Total tokens saved by every Mustard intervention in scope.
    pub total_tokens_saved: i64,
    /// Number of span rows in scope (useful as a denominator for averages).
    pub span_count: i64,
    /// Top 3 agents ordered by cost descending (truncated to <=3).
    pub top_agents_by_cost: Vec<AgentCost>,
    /// MEASURED cost per session (Anthropic billed `cost.usage`), ordered by USD
    /// descending. Populated ONLY at the unfiltered project / all-projects scope
    /// — `usage_totals` carries no spec/wave dimension, so spec/wave scopes leave
    /// this empty. Lets the user cross-check ONE session against Claude Code's
    /// own `/cost` to confirm the headline number is real.
    #[serde(default)]
    pub by_session: Vec<SessionCost>,
    /// `MAX(usage_totals.updated_at)` epoch-ms — when the MEASURED counters were
    /// last refreshed. `None` at spec/wave scope or when no measured row exists.
    /// Drives the "atualizado há Xs" freshness caption on the cost KPI.
    #[serde(default)]
    pub last_updated_ms: Option<i64>,
    /// `MAX(run_usage.started_at)` epoch-ms — when the ESTIMATED counters were
    /// last ingested. `None` at spec/wave scope or when `run_usage` is empty.
    /// The dashboard compares this against `last_updated_ms`: when the gap
    /// exceeds the staleness threshold the UI surfaces a banner so the user
    /// knows the per-spec estimation has stopped catching new dispatches.
    #[serde(default)]
    pub last_estimated_ms: Option<i64>,
}

/// MEASURED cost for one Claude Code session, in USD (not micro-USD — sourced
/// from `usage_totals.cost.usage`, which is a float USD counter). Ordered by
/// `usd` descending by the reader. The session id matches what Claude Code's
/// `/cost` reports, so the user can match a single row one-to-one.
///
/// `last_at_ms` and `specs` are populated only at the unfiltered project /
/// all-projects scope (the same scope that populates `by_session` on
/// [`EconomySummary`]). At spec/wave scope they stay defaulted (`None` / empty)
/// because `usage_totals` carries no spec/wave dimension. Both fields are
/// `#[serde(default)]` so older payloads round-trip without breaking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCost {
    /// Claude Code session id (`usage_totals.session_id`).
    pub session_id: String,
    /// Aggregate measured cost for the session, in USD.
    pub usd: f64,
    /// `MAX(usage_totals.updated_at)` for this session — epoch-ms of the most
    /// recent measured counter update. Drives the per-session freshness caption.
    #[serde(default)]
    pub last_at_ms: Option<i64>,
    /// Distinct specs the session worked on, sourced from
    /// `run_usage.spec` (self-attributed at write time). Sorted ascending.
    /// Empty when the session has no spec-attributed runs yet.
    #[serde(default)]
    pub specs: Vec<String>,
}

/// Token totals projected from MEASURED OTEL `claude_code.token.usage`
/// metric datapoints, split by token side and grouped by model.
///
/// Unlike [`EconomySummary`] (which folds input + output into a single
/// `total_tokens`), this view keeps the input/output split the OTEL metric
/// carries on its `token_type` attribute, plus a per-model breakdown — the
/// shape the rt MCP `get_run_summary` contract exposes. The metric channel
/// is the only place the real billed token counts live, so this reader is the
/// canonical bridge between that channel and any consumer that needs the split.
///
/// Token-side mapping (matches Anthropic billing semantics): the OTEL
/// `type` attribute spells `input` / `output` / `cacheRead` / `cacheCreation`.
/// `output` is the only output-side type; `input`, `cacheRead`, and
/// `cacheCreation` are all input-side and roll into `input_tokens`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MetricTokenSummary {
    /// How many `claude_code.token.usage` datapoints contributed.
    pub datapoint_count: i64,
    /// Total input-side tokens (input + cacheRead + cacheCreation).
    pub input_tokens: i64,
    /// Total output-side tokens.
    pub output_tokens: i64,
    /// Per-model buckets, ordered by total (input + output) tokens descending.
    pub by_model: Vec<MetricTokenModelBucket>,
}

/// One model's slice of a [`MetricTokenSummary`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricTokenModelBucket {
    /// Model name as reported on the metric's `model` attribute, or
    /// `"unknown"` when the attribute is absent.
    pub model: String,
    /// How many datapoints rolled into this model.
    pub datapoint_count: i64,
    /// Input-side tokens (input + cacheRead + cacheCreation) for this model.
    pub input_tokens: i64,
    /// Output-side tokens for this model.
    pub output_tokens: i64,
}

/// Token totals attributed to each pipeline PHASE (ANALYZE / PLAN / EXECUTE /
/// QA / CLOSE / …) by CORRELATING the phase-less OTEL token metric channel with
/// the `pipeline.phase` transition timeline.
///
/// The OTEL `claude_code.token.usage` datapoints carry no phase dimension (and
/// `spec: null`), so a phase cannot be read off a datapoint directly. Instead
/// [`super::reader::per_phase_token_summary`] reconstructs, per session, the
/// ordered list of `(ts, phase)` transitions emitted by `pipeline.phase`, then
/// assigns every token datapoint to whichever phase was active at the
/// datapoint's timestamp (the last transition with `ts <= datapoint.ts`).
/// Datapoints that predate the first transition in their session fall into the
/// synthetic [`PHASE_UNATTRIBUTED`] bucket.
///
/// Token-side mapping matches [`MetricTokenSummary`]: `output` is the only
/// output-side type; `input` / `cacheRead` / `cacheCreation` roll into input.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PerPhaseTokenSummary {
    /// How many `claude_code.token.usage` datapoints were attributed in total.
    pub datapoint_count: i64,
    /// Total input-side tokens across every phase.
    pub input_tokens: i64,
    /// Total output-side tokens across every phase.
    pub output_tokens: i64,
    /// Per-phase buckets, ordered by total (input + output) tokens descending.
    pub by_phase: Vec<PhaseTokenBucket>,
}

/// Synthetic phase name for token datapoints that fall before the first
/// `pipeline.phase` transition in their session (no phase was active yet).
pub const PHASE_UNATTRIBUTED: &str = "unattributed";

/// One phase's slice of a [`PerPhaseTokenSummary`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseTokenBucket {
    /// Pipeline phase name as emitted on `pipeline.phase` `payload.to`
    /// (e.g. `"ANALYZE"`, `"EXECUTE"`), or [`PHASE_UNATTRIBUTED`] for
    /// datapoints that predate the first transition in their session.
    pub phase: String,
    /// How many datapoints rolled into this phase.
    pub datapoint_count: i64,
    /// Input-side tokens (input + cacheRead + cacheCreation) for this phase.
    pub input_tokens: i64,
    /// Output-side tokens for this phase.
    pub output_tokens: i64,
}

/// Per-agent cost roll-up. Ordered by `cost_usd_micros` desc by the reader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCost {
    /// Agent role/skill name.
    pub agent_id: AgentId,
    /// Aggregate cost in micro-USD.
    pub cost_usd_micros: i64,
    /// Aggregate token usage (input + output) for the agent in scope.
    pub tokens: i64,
    /// How many spans rolled up into this row.
    pub span_count: i64,
}

/// Per-spec cost roll-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecCost {
    /// Spec slug.
    pub spec_id: SpecId,
    /// Aggregate cost in micro-USD.
    pub cost_usd_micros: i64,
    /// Aggregate token usage for the spec in scope.
    pub tokens: i64,
    /// Span count under the spec.
    pub span_count: i64,
    /// `MAX(run_usage.started_at)` for the spec — populated by
    /// [`super::reader::per_spec_costs`] so the UI can sort newest specs first.
    /// `None` when the spec has no timestamped rows.
    #[serde(default)]
    pub last_started_at: Option<i64>,
}

/// Per-wave cost roll-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveCost {
    /// Spec slug the wave belongs to.
    pub spec_id: SpecId,
    /// Wave slug.
    pub wave_id: WaveId,
    /// Aggregate cost in micro-USD.
    pub cost_usd_micros: i64,
    /// Aggregate token usage for the wave in scope.
    pub tokens: i64,
    /// Span count under the wave.
    pub span_count: i64,
}

/// Breakdown of savings by [`SavingsSource`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SavingsBreakdown {
    /// Total tokens saved across every source in scope.
    pub total_tokens_saved: i64,
    /// Per-source roll-ups, ordered by `tokens_saved` desc.
    pub per_source: Vec<SavingsBySource>,
}

/// One row of the [`SavingsBreakdown::per_source`] list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavingsBySource {
    /// Which intervention this row aggregates.
    pub source: SavingsSource,
    /// Total tokens this intervention saved in scope.
    pub tokens_saved: i64,
    /// Number of savings events that contributed.
    pub event_count: i64,
}

/// Context-routing quality metrics — ratios on `[0.0, 1.0]` are stored as
/// permille (parts per thousand) `i64` so the wire format stays integer-only.
/// The UI divides by 1000.0 when rendering.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextRoutingMetrics {
    /// `prefix_stable_bytes / prompt_size_bytes` averaged across the in-scope
    /// frames, in permille (0–1000).
    pub prefix_stable_ratio_permille: i64,
    /// `cache_read_input_tokens / (input_tokens + cache_read_input_tokens)`
    /// averaged across in-scope spans, in permille (0–1000).
    pub cache_hit_ratio_permille: i64,
    /// `retry_overhead_bytes / prompt_size_bytes` averaged across in-scope
    /// frames, in permille (0–1000).
    pub retry_overhead_ratio_permille: i64,
    /// How many [`ContextCostFrame`] rows contributed to the averages.
    pub frame_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn savings_source_roundtrip_string() {
        for src in [
            SavingsSource::RtkRewrite,
            SavingsSource::ModelRoutingDowngrade,
            SavingsSource::BashGuardBlock,
            SavingsSource::BudgetOutputCut,
            SavingsSource::RecipeInjection,
            SavingsSource::ScanStructuralExtract,
            SavingsSource::ScanSkillRender,
        ] {
            let s = src.as_str();
            assert_eq!(SavingsSource::from_str_opt(s), Some(src));
        }
        assert!(SavingsSource::from_str_opt("nope").is_none());
    }

    #[test]
    fn span_record_serde_preserves_unknown_fields() {
        let json = r#"{
            "ts": "2026-05-20T00:00:00Z",
            "span_id": "sp-1",
            "input_tokens": 100,
            "future_field": "captured"
        }"#;
        let rec: SpanRecord = serde_json::from_str(json).unwrap();
        assert_eq!(rec.span_id, "sp-1");
        assert_eq!(rec.input_tokens, Some(100));
        assert_eq!(
            rec.extra.get("future_field").and_then(|v| v.as_str()),
            Some("captured")
        );
    }

}
