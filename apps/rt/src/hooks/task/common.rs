//! Shared helpers for the Task/Subagent hook family.
//!
//! `tracker.rs` once held five concerns plus their plumbing in one file; the
//! concerns now live one-per-file ([`super::tool_use_counter`],
//! [`super::main_context_counter`], [`super::subagent_observer`],
//! [`super::metrics_observer`], [`super::skill_usage_observer`]). The small
//! pieces they share — project-dir resolution, harness-event emission, the
//! `pipeline.economy.run` finaliser, and a few payload extractors — live here
//! so no concern re-implements them.

use crate::shared::context::current_spec;
use mustard_core::domain::economy::estimator;
use mustard_core::domain::economy::writer as economy_writer;
use mustard_core::domain::economy::SpanRecord;
use mustard_core::domain::model::contract::HookInput;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::time::now_iso8601;
use serde_json::{Map, Value};

/// Finalise an agent's Task dispatch as one `pipeline.economy.run` NDJSON
/// event. The companion channel to the OTEL collector's
/// `pipeline.telemetry.run` — same payload shape so the dashboard reader
/// (`mustard_core::domain::economy::reader::*`) aggregates both transparently.
///
/// W7B of [[2026-05-26-no-sqlite-git-source-of-truth]] replaced the previous
/// SQLite `telemetry.db` write with this NDJSON emit. `model` is the model
/// id the dispatch ran under (may be empty — pricing falls through to
/// `(0, 0)`). `api_input_tokens` / `api_output_tokens` come from the
/// Anthropic `usage` payload when present, else are estimated from byte
/// size. Fail-open: a routing failure simply drops the telemetry.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_task_run(
    project_dir: &str,
    session_id: Option<&str>,
    span_id: String,
    model: &str,
    spec: Option<String>,
    input_text: &str,
    output_text: &str,
    api_input_tokens: Option<i64>,
    api_output_tokens: Option<i64>,
    wave_id: Option<&str>,
    agent_id: Option<&str>,
    tool_use_id: Option<&str>,
    is_error: bool,
) {
    let input_tokens = api_input_tokens
        .unwrap_or_else(|| i64::from(estimator::estimate_input_tokens(input_text, model)));
    let output_tokens = api_output_tokens
        .unwrap_or_else(|| i64::from(estimator::estimate_output_tokens(output_text, model)));
    let (in_micros_per_m, out_micros_per_m) =
        estimator::model_pricing_usd_micros_per_million(model);
    // Saturating arithmetic — keeps the writer safe even when an adapter
    // ships an absurd token count (`i64::MAX`).
    let cost_usd_micros = in_micros_per_m
        .saturating_mul(input_tokens)
        .saturating_add(out_micros_per_m.saturating_mul(output_tokens))
        / 1_000_000;
    let ts = now_iso8601();
    let mut extra = Map::new();
    if let Some(w) = wave_id {
        extra.insert("wave_id".to_string(), Value::String(w.to_string()));
    }
    if let Some(a) = agent_id {
        extra.insert("agent_id".to_string(), Value::String(a.to_string()));
    }
    if let Some(t) = tool_use_id {
        extra.insert("tool_use_id".to_string(), Value::String(t.to_string()));
    }
    let rec = SpanRecord {
        ts: ts.clone(),
        session_id: session_id.map(str::to_string),
        span_id,
        model: if model.is_empty() {
            None
        } else {
            Some(model.to_string())
        },
        spec,
        phase: None,
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        cost_usd_micros: Some(cost_usd_micros),
        is_error,
        extra,
    };
    let (event_name, payload) = economy_writer::run_event(&rec);
    emit_event(project_dir, "tracker", &event_name, payload, session_id);
}

/// Resolve the project dir for an invocation: the harness `cwd`, else `.`.
/// Mirrors the JS `data.cwd || process.cwd()`.
pub(crate) fn project_dir(input: &HookInput) -> String {
    match input.cwd.as_deref() {
        Some(cwd) if !cwd.is_empty() && cwd != "." => cwd.to_string(),
        _ => ".".to_string(),
    }
}

/// Like [`project_dir`] but returns `None` when no valid harness cwd is
/// supplied (avoids leaking state writes into the process cwd — the
/// `cargo test -p mustard-rt` AC-W5.2 regression).
pub(crate) fn project_dir_opt(input: &HookInput) -> Option<String> {
    match input.cwd.as_deref() {
        Some(cwd) if !cwd.is_empty() && cwd != "." => Some(cwd.to_string()),
        _ => None,
    }
}

/// Build one harness event from the hook context. `session_id` is the id the
/// harness threaded onto the `HookInput`; `None` falls back to `"unknown"` so
/// `route::emit` resolves it via the environment, exactly as before.
fn build_harness_event(
    project_dir: &str,
    hook_id: &str,
    event: &str,
    payload: Value,
    session_id: Option<&str>,
) -> HarnessEvent {
    // Best-effort wave attribution from `MUSTARD_ACTIVE_WAVE`. A session spans
    // multiple waves and the PostToolUse hook context carries no per-event wave
    // signal, so this env var is the only reliable source; leave 0 when unset
    // (the router treats 0 as "no wave" and falls back to its own env read).
    let wave = current_wave_id()
        .and_then(|w| w.parse::<u32>().ok())
        .unwrap_or(0);
    HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_string(),
        wave,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some(hook_id.to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload,
        spec: current_spec(project_dir),
    }
}

/// Emit one harness event, best-effort. Telemetry is never load-bearing.
///
/// Routes through the W5 [`crate::shared::events::route::emit`] classifier:
/// `pipeline.*` lands in SQLite, everything else (the vast majority of
/// these telemetry events — `tool.use`, `agent.start`, `agent.stop`,
/// `subagent.*`, etc.) lands in the per-spec NDJSON sink.
pub(crate) fn emit_event(
    project_dir: &str,
    hook_id: &str,
    event: &str,
    payload: Value,
    session_id: Option<&str>,
) {
    let harness_event = build_harness_event(project_dir, hook_id, event, payload, session_id);
    let _ = crate::shared::events::route::emit(project_dir, &harness_event);
}

/// Resolve the active wave id from `MUSTARD_ACTIVE_WAVE` (the convention the
/// other hooks read for attribution). `None` when unset or blank.
pub(crate) fn current_wave_id() -> Option<String> {
    std::env::var("MUSTARD_ACTIVE_WAVE")
        .ok()
        .filter(|s| !s.is_empty())
}

/// Extract `tool_use_id` from the hook payload — Claude Code may place it at the
/// root or nested under `tool_response`. Mirrors `tool_result::extract_tool_use_id`.
pub(crate) fn extract_tool_use_id(input: &HookInput) -> Option<String> {
    if let Some(s) = input.raw.get("tool_use_id").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    input
        .raw
        .get("tool_response")
        .and_then(|v| v.get("tool_use_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Truncate `s` to `max` chars (char-boundary safe).
pub(crate) fn cap(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// W7B: the legacy `upsert_run_attribution` (which wrote a row into
// `telemetry.db.run_attribution` keyed on `(session_id, tool_use_id)`) was
// deleted. Attribution now travels INLINE with each run event — `record_task_run`
// promotes `wave_id` / `agent_id` / `tool_use_id` into the
// `pipeline.economy.run` payload, and the OTEL collector does the same for
// `pipeline.telemetry.run`. The dashboard reader resolves attribution off
// those keys directly (W5#8 two-tier fallback already covers the late-binding
// case in `apps/dashboard/src-tauri/src/telemetry.rs`).
