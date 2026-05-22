//! Pure data types for the telemetry domain.
//!
//! Three structs, one per table the module owns:
//!
//! - [`UsageMetric`] — one row of `usage_totals` (aggregated OTEL counter).
//! - [`RunUsage`] — one row of `run_usage` (per-execution token usage + cost).
//! - [`RunAttribution`] — one row of `run_attribution` (write-time spec/wave/
//!   agent stamp for a tool use).
//!
//! Each type is `serde`-derivable so a consumer can transport it as JSON. The
//! ones that cross an external boundary (a collector decoding an upstream
//! payload) keep a `#[serde(default)]` on every optional field so a partial or
//! unknown shape degrades to `None` rather than failing the whole parse — the
//! lenient-serde convention this crate follows for ingest types.

use serde::{Deserialize, Serialize};

/// One aggregated usage counter — a row of `usage_totals`.
///
/// Replaces the legacy `claude_code_otel` row, reduced to the columns that
/// have a read consumer. `metric` is the OTEL metric name (e.g.
/// `claude_code.cost.usage`, `claude_code.session.count`); `sum` is the
/// accumulated value across every contributing datapoint for the
/// `(metric, model, session_id)` triple.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageMetric {
    /// OTEL metric name — the grouping key shared by every datapoint summed
    /// into this row.
    pub metric: String,
    /// Model the datapoint was attributed to; `None` for model-agnostic
    /// metrics such as `claude_code.session.count`.
    #[serde(default)]
    pub model: Option<String>,
    /// Session the datapoint belongs to; `None` when the upstream payload
    /// carried no `session.id`.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Accumulated value across every datapoint folded into this row.
    #[serde(default)]
    pub sum: f64,
    /// Milliseconds-epoch of the most recent contributing datapoint — the
    /// freshness signal `MAX(updated_at)` reads.
    #[serde(default)]
    pub updated_at: Option<i64>,
}

/// One per-execution usage record — a row of `run_usage`.
///
/// Carries the legacy `spans` columns the economy reader projects, plus the
/// load-bearing `agent_id`. `span_id` is the primary key; a re-record of the
/// same id is an upsert (idempotent ingest).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunUsage {
    /// Trace this run belongs to, when the adapter exposed one.
    #[serde(default)]
    pub trace_id: Option<String>,
    /// Span identifier — the primary key.
    pub span_id: String,
    /// Parent span, when this is a child span.
    #[serde(default)]
    pub parent_span_id: Option<String>,
    /// Human-readable span name.
    #[serde(default)]
    pub name: Option<String>,
    /// Start time in milliseconds-epoch.
    #[serde(default)]
    pub started_at: Option<i64>,
    /// End time in milliseconds-epoch.
    #[serde(default)]
    pub ended_at: Option<i64>,
    /// Wall-clock duration in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<i64>,
    /// Raw OTEL attributes blob (JSON string).
    #[serde(default)]
    pub attributes: Option<String>,
    /// Spec the run is attributed to (load-bearing).
    #[serde(default)]
    pub spec: Option<String>,
    /// Pipeline phase the run occurred in.
    #[serde(default)]
    pub phase: Option<String>,
    /// Model in use during the run.
    #[serde(default)]
    pub model: Option<String>,
    /// Input token count.
    #[serde(default)]
    pub input_tokens: Option<i64>,
    /// Output token count.
    #[serde(default)]
    pub output_tokens: Option<i64>,
    /// Cache-read input tokens (billed at a discount).
    #[serde(default)]
    pub cache_read_input_tokens: Option<i64>,
    /// Cache-creation input tokens.
    #[serde(default)]
    pub cache_creation_input_tokens: Option<i64>,
    /// Priced cost in micro-USD.
    #[serde(default)]
    pub cost_usd_micros: Option<i64>,
    /// Whether the run ended in an error.
    #[serde(default)]
    pub is_error: bool,
    /// Originating project path, when a multi-project reader needs to scope.
    #[serde(default)]
    pub project_path: Option<String>,
    /// ISO-8601 timestamp of the run.
    #[serde(default)]
    pub ts_iso: Option<String>,
    /// Session the run belongs to.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Wave the run was dispatched against (load-bearing).
    #[serde(default)]
    pub wave_id: Option<String>,
    /// Anthropic `tool_use` block id — the attribution join key.
    #[serde(default)]
    pub tool_use_id: Option<String>,
    /// Agent the run is attributed to (load-bearing).
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// One write-time attribution stamp — a row of `run_attribution`.
///
/// Keyed on `(session_id, tool_use_id)`: the collector records the spec / wave
/// / agent active for a tool use the moment it fires, so the reader never has
/// to reconstruct attribution from the event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunAttribution {
    /// Session the tool use belongs to.
    pub session_id: String,
    /// Anthropic `tool_use` block id.
    pub tool_use_id: String,
    /// Spec active when the tool use fired.
    #[serde(default)]
    pub spec: Option<String>,
    /// Wave active when the tool use fired.
    #[serde(default)]
    pub wave_id: Option<String>,
    /// Agent that issued the tool use.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Milliseconds-epoch of the stamp.
    #[serde(default)]
    pub updated_at: Option<i64>,
}
