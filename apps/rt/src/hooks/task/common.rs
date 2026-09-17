//! Shared helpers for the Task/Subagent hook family.
//!
//! `tracker.rs` once held five concerns plus their plumbing in one file; the
//! concerns now live one-per-file ([`super::tool_use_counter`],
//! [`super::main_context_counter`], [`super::subagent_observer`],
//! [`super::metrics_observer`], [`super::skill_usage_observer`]). The small
//! pieces they share — project-dir resolution, harness-event emission, the
//! `pipeline.economy.run` finaliser, and a few payload extractors — live here
//! so no concern re-implements them.

use mustard_core::domain::model::contract::HookInput;


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



/// Resolve the active wave id from `MUSTARD_ACTIVE_WAVE` (the convention the
/// other hooks read for attribution). `None` when unset or blank.
pub(crate) fn current_wave_id() -> Option<String> {
    std::env::var("MUSTARD_ACTIVE_WAVE")
        .ok()
        .filter(|s| !s.is_empty())
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
// case in `apps/dashboard/server/src/telemetry.rs`).
