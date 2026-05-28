//! `mustard-rt run pipeline-prelude` — consolidate the per-phase warm-up.
//!
//! Replaces the three-call dance every pipeline ANALYZE/PLAN/EXECUTE used to
//! perform manually (`spec-hygiene` probe + `diff-context` snapshot +
//! `sync-detect` / `sync-registry` gate). W6 callers invoke this single
//! subcommand once per phase entry — the binary chains the steps internally and
//! emits one summary JSON.
//!
//! Each inner step is fail-open: an error degrades a field to a string and the
//! prelude continues with the next step. The phase-specific behavior:
//!
//! | Phase      | Steps                                          |
//! |------------|------------------------------------------------|
//! | `ANALYZE`  | sync-detect only (diff is empty pre-work)      |
//! | `PLAN`     | sync-detect + diff-context summary             |
//! | `EXECUTE`  | sync-detect + diff-context summary             |
//!
//! ## Output shape
//!
//! ```json
//! {
//!   "spec":   "<slug>",
//!   "phase":  "EXECUTE",
//!   "steps":  [
//!     { "name": "sync-detect",  "ok": true, "duration_ms": 12 },
//!     { "name": "diff-context", "ok": true, "duration_ms": 34 }
//!   ],
//!   "duration_ms": 46
//! }
//! ```
//!
//! ## Telemetry
//!
//! Emits `pipeline.economy.operation.invoked { operation: "pipeline-prelude",
//! duration_ms, tokens_used: 0, was_rust_only: true }` once per invocation.

use crate::shared::context::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::process::rtk_command;
use serde::Serialize;
use serde_json::json;

/// Options for `mustard-rt run pipeline-prelude`.
#[derive(Debug, Clone)]
pub struct PreludeOpts {
    pub spec: String,
    pub phase: String,
}

/// One step in the prelude pipeline.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StepReport {
    pub name: String,
    pub ok: bool,
    pub duration_ms: u64,
}

/// Aggregate report emitted to stdout.
#[derive(Debug, Serialize)]
pub struct PreludeReport {
    pub spec: String,
    pub phase: String,
    pub steps: Vec<StepReport>,
    pub duration_ms: u64,
}

/// Normalise the phase token; unknown phases default to `EXECUTE` behavior.
#[must_use]
pub fn normalised_phase(raw: &str) -> String {
    let upper = raw.trim().to_ascii_uppercase();
    match upper.as_str() {
        "ANALYZE" | "PLAN" | "EXECUTE" | "REVIEW" | "QA" | "CLOSE" => upper,
        _ => "EXECUTE".to_string(),
    }
}

/// Decide which inner steps run for a phase. Pure, unit-testable.
#[must_use]
pub fn steps_for_phase(phase: &str) -> Vec<&'static str> {
    match phase {
        "ANALYZE" => vec!["sync-detect"],
        _ => vec!["sync-detect", "diff-context"],
    }
}

/// Run one inner step by name. Returns `(ok, elapsed_ms)`.
fn run_step(step: &str, phase: &str) -> (bool, u64) {
    let started = std::time::Instant::now();
    let ok = match step {
        "sync-detect" => {
            // Best-effort: shell out to ourselves so the user sees the same JSON
            // they would from a direct call. We don't parse the result — the
            // success signal is `exit == 0`.
            rtk_command("mustard-rt", &["run", "sync-detect"])
                .output()
                .is_ok_and(|o| o.status.success())
        }
        "diff-context" => rtk_command(
            "mustard-rt",
            &["run", "diff-context", "--phase", &phase.to_ascii_lowercase()],
        )
        .output()
        .is_ok_and(|o| o.status.success()),
        _ => false,
    };
    let dur = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    (ok, dur)
}

/// CLI entry. Prints the JSON report and emits the economy event.
pub fn run(opts: PreludeOpts) {
    let started = std::time::Instant::now();
    let phase = normalised_phase(&opts.phase);
    let mut steps: Vec<StepReport> = Vec::new();
    for name in steps_for_phase(&phase) {
        let (ok, dur) = run_step(name, &phase);
        steps.push(StepReport { name: name.to_string(), ok, duration_ms: dur });
    }
    let total = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let report = PreludeReport {
        spec: opts.spec.clone(),
        phase,
        steps,
        duration_ms: total,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(total, &opts.spec);
}

fn emit_economy(duration_ms: u64, spec: &str) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec_attr = if spec.is_empty() {
        current_spec(&cwd)
    } else {
        Some(spec.to_string())
    };
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("pipeline-prelude".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "pipeline-prelude",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: spec_attr,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_normaliser_uppercases_known_phases() {
        assert_eq!(normalised_phase("analyze"), "ANALYZE");
        assert_eq!(normalised_phase("Execute"), "EXECUTE");
        assert_eq!(normalised_phase("REVIEW"), "REVIEW");
    }

    #[test]
    fn phase_normaliser_falls_back_to_execute() {
        assert_eq!(normalised_phase("draft"), "EXECUTE");
        assert_eq!(normalised_phase(""), "EXECUTE");
    }

    #[test]
    fn analyze_phase_skips_diff_context() {
        let steps = steps_for_phase("ANALYZE");
        assert_eq!(steps, vec!["sync-detect"]);
    }

    #[test]
    fn execute_phase_runs_sync_and_diff() {
        let steps = steps_for_phase("EXECUTE");
        assert_eq!(steps, vec!["sync-detect", "diff-context"]);
    }

    #[test]
    fn plan_phase_runs_sync_and_diff() {
        let steps = steps_for_phase("PLAN");
        assert_eq!(steps, vec!["sync-detect", "diff-context"]);
    }

    #[test]
    fn report_serializes_to_required_fields() {
        let r = PreludeReport {
            spec: "demo".to_string(),
            phase: "EXECUTE".to_string(),
            steps: vec![StepReport { name: "sync-detect".to_string(), ok: true, duration_ms: 1 }],
            duration_ms: 2,
        };
        let v = serde_json::to_value(r).unwrap();
        assert!(v.get("spec").is_some());
        assert!(v.get("phase").is_some());
        assert!(v.get("steps").unwrap().is_array());
        assert!(v.get("duration_ms").is_some());
    }
}
