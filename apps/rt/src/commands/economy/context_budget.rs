//! `mustard-rt run context-budget` — planning-time budget calculator.
//!
//! Returns the recommended prompt budget for a `(role, spec, wave)` triple so a
//! pipeline phase can plan its dispatch envelope **before** building the prompt.
//! Pure transform: no IO beyond an optional `mustard.json` peek for the project
//! locale (used in a tiny advisory hint). Output is byte-stable JSON.
//!
//! Per-role budgets follow the W5 contract documented in
//! `pipeline-config.md § Per-Role Budgets`:
//!
//! | Role             | char budget |
//! |------------------|-------------|
//! | `explore`        | 10_000      |
//! | `review`         | 12_000      |
//! | `qa`             | 12_000      |
//! | `plan`           | 18_000      |
//! | `general-purpose`| 30_000      |
//! | any other        | 30_000 (fallback) |
//!
//! Wave + spec are echoed in the report so the dashboard can group budget
//! requests by pipeline run.
//!
//! ## Telemetry
//!
//! Emits a single `pipeline.economy.operation.invoked` event with
//! `{ operation: "context-budget", duration_ms, tokens_used: 0, was_rust_only: true }`.

use crate::shared::context::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde::Serialize;
use serde_json::json;

/// Options for `mustard-rt run context-budget`.
#[derive(Debug, Clone)]
pub struct ContextBudgetOpts {
    /// Agent role token (e.g. `explore`, `general-purpose`).
    pub role: String,
    /// Spec slug under `.claude/spec/` (optional — only echoed in the report).
    pub spec: Option<String>,
    /// 1-based wave number (optional).
    pub wave: Option<u32>,
}

/// Pure report structure — what the caller reads.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BudgetReport {
    pub role: String,
    pub spec: Option<String>,
    pub wave: Option<u32>,
    pub char_budget: u32,
    pub note: &'static str,
}

/// Resolve the per-role char budget. Pure — unit-testable without IO.
#[must_use]
pub fn char_budget_for(role: &str) -> (u32, &'static str) {
    match role.trim().to_ascii_lowercase().as_str() {
        "explore" => (10_000, "Explore agents cap at 10k chars to keep search scope tight."),
        "review" => (12_000, "Review agents cap at 12k chars — diff is source of truth."),
        "qa" => (12_000, "QA agents cap at 12k chars — AC commands drive verification."),
        "plan" => (18_000, "Plan agents get 18k chars for cross-file design synthesis."),
        "general-purpose" | "general" => {
            (30_000, "General-purpose agents get 30k chars for implementation.")
        }
        _ => (30_000, "Unknown role — defaults to general-purpose budget (30k)."),
    }
}

/// Compute the report. Pure function — no side effects.
#[must_use]
pub fn compute(opts: &ContextBudgetOpts) -> BudgetReport {
    let (char_budget, note) = char_budget_for(&opts.role);
    BudgetReport {
        role: opts.role.clone(),
        spec: opts.spec.clone(),
        wave: opts.wave,
        char_budget,
        note,
    }
}

/// CLI entry point. Prints the JSON report and emits the economy event.
pub fn run(opts: ContextBudgetOpts) {
    let started = std::time::Instant::now();
    let report = compute(&opts);
    let body = serde_json::to_string_pretty(&report)
        .unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis(), &opts);
}

/// Emit the universal economy marker. Fail-open.
fn emit_economy(duration_ms: u128, opts: &ContextBudgetOpts) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = opts
        .spec
        .clone()
        .or_else(|| current_spec(&cwd));
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: opts.wave.unwrap_or(0),
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("context-budget".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "context-budget",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_explore_caps_at_10k() {
        let (b, _) = char_budget_for("explore");
        assert_eq!(b, 10_000);
    }

    #[test]
    fn role_review_and_qa_cap_at_12k() {
        assert_eq!(char_budget_for("review").0, 12_000);
        assert_eq!(char_budget_for("qa").0, 12_000);
        assert_eq!(char_budget_for("REVIEW").0, 12_000);
    }

    #[test]
    fn role_general_purpose_caps_at_30k() {
        assert_eq!(char_budget_for("general-purpose").0, 30_000);
        assert_eq!(char_budget_for("general").0, 30_000);
    }

    #[test]
    fn unknown_role_falls_back_to_30k() {
        assert_eq!(char_budget_for("backend").0, 30_000);
        assert_eq!(char_budget_for("").0, 30_000);
    }

    #[test]
    fn compute_preserves_wave_and_spec() {
        let opts = ContextBudgetOpts {
            role: "explore".to_string(),
            spec: Some("demo".to_string()),
            wave: Some(3),
        };
        let r = compute(&opts);
        assert_eq!(r.role, "explore");
        assert_eq!(r.spec.as_deref(), Some("demo"));
        assert_eq!(r.wave, Some(3));
        assert_eq!(r.char_budget, 10_000);
    }

    #[test]
    fn json_shape_includes_required_fields() {
        let opts = ContextBudgetOpts {
            role: "review".to_string(),
            spec: None,
            wave: None,
        };
        let v = serde_json::to_value(compute(&opts)).unwrap();
        assert!(v.get("role").is_some());
        assert!(v.get("char_budget").is_some());
        assert!(v.get("note").is_some());
        assert_eq!(v["char_budget"], json!(12_000));
    }
}
