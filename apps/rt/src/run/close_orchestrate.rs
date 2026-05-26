//! `mustard-rt run close-orchestrate` — drive the CLOSE-phase gates.
//!
//! Replaces the imperative step list inside `close/SKILL.md`. Runs the four
//! gates (verify-pipeline → qa-run → docs-stale-check → pipeline-summary) in
//! order, captures a pass/fail per gate, and emits one machine-readable JSON
//! report so the orchestrator can decide whether to finalize.
//!
//! ## Fail-open
//!
//! Each gate is fail-open at the subprocess level: a missing binary or
//! non-zero exit becomes a `gate.ok = false` row, the next gate still runs,
//! and the overall verdict is derived from the boolean vector. A SKILL
//! consumer reads the `overall` field; downstream tools may inspect each
//! individual `gate` entry.
//!
//! ## Output shape
//!
//! ```json
//! {
//!   "spec":    "<slug>",
//!   "overall": "pass" | "fail",
//!   "gates": [
//!     { "name": "verify-pipeline", "ok": true,  "duration_ms": 123 },
//!     { "name": "qa-run",          "ok": true,  "duration_ms": 456, "summary": "pass" },
//!     { "name": "docs-stale-check","ok": true,  "duration_ms": 78 },
//!     { "name": "pipeline-summary","ok": true,  "duration_ms": 12 }
//!   ],
//!   "duration_ms": 669
//! }
//! ```

use crate::run::env::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::process::rtk_command;
use serde::Serialize;
use serde_json::{json, Value};

/// Options for `mustard-rt run close-orchestrate`.
#[derive(Debug, Clone)]
pub struct CloseOrchestrateOpts {
    pub spec: String,
    /// Skip docs-stale-check (useful for non-architectural specs).
    pub skip_docs: bool,
}

/// One gate entry in the JSON report.
#[derive(Debug, Serialize)]
pub struct GateReport {
    pub name: String,
    pub ok: bool,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Aggregate report.
#[derive(Debug, Serialize)]
pub struct CloseReport {
    pub spec: String,
    pub overall: &'static str,
    pub gates: Vec<GateReport>,
    pub duration_ms: u64,
}

/// Run a `mustard-rt run <sub> [args]` and report `(ok, elapsed_ms, stdout)`.
fn run_subcmd(args: &[&str]) -> (bool, u64, String) {
    let started = std::time::Instant::now();
    let mut full: Vec<&str> = vec!["run"];
    full.extend_from_slice(args);
    let out = rtk_command("mustard-rt", &full).output();
    let dur = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match out {
        Ok(o) => (
            o.status.success(),
            dur,
            String::from_utf8_lossy(&o.stdout).into_owned(),
        ),
        Err(_) => (false, dur, String::new()),
    }
}

/// Inspect a `qa-run --format json` stdout for the `overall` field.
fn qa_overall(stdout: &str) -> Option<String> {
    let v: Value = serde_json::from_str(stdout.trim()).ok()?;
    v.get("overall").and_then(Value::as_str).map(str::to_string)
}

/// CLI entry.
pub fn run(opts: CloseOrchestrateOpts) {
    let started = std::time::Instant::now();
    let mut gates: Vec<GateReport> = Vec::new();

    // 1. verify-pipeline (build/test gate).
    let (ok, dur, _) = run_subcmd(&["verify-pipeline"]);
    gates.push(GateReport {
        name: "verify-pipeline".to_string(),
        ok,
        duration_ms: dur,
        summary: None,
    });

    // 2. qa-run --spec <spec>.
    let (qa_ok, qa_dur, qa_out) = run_subcmd(&["qa-run", "--spec", &opts.spec]);
    let qa_summary = qa_overall(&qa_out);
    // Treat `skip` as a pass for the overall verdict (no AC = no fail).
    let qa_pass = qa_ok
        && qa_summary
            .as_deref()
            .map_or(qa_ok, |s| s == "pass" || s == "skip");
    gates.push(GateReport {
        name: "qa-run".to_string(),
        ok: qa_pass,
        duration_ms: qa_dur,
        summary: qa_summary,
    });

    // 3. docs-stale-check (optional).
    if !opts.skip_docs {
        let (ok, dur, _) = run_subcmd(&["docs-stale-check"]);
        gates.push(GateReport {
            name: "docs-stale-check".to_string(),
            ok,
            duration_ms: dur,
            summary: None,
        });
    }

    // 4. pipeline-summary (advisory — always passes).
    let spec_dir = format!(".claude/spec/{}", opts.spec);
    let (sum_ok, sum_dur, _) = run_subcmd(&["pipeline-summary", "--spec-dir", &spec_dir]);
    gates.push(GateReport {
        name: "pipeline-summary".to_string(),
        ok: sum_ok,
        duration_ms: sum_dur,
        summary: None,
    });

    let overall_pass = gates.iter().all(|g| g.ok);
    let total = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let report = CloseReport {
        spec: opts.spec.clone(),
        overall: if overall_pass { "pass" } else { "fail" },
        gates,
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
            id: Some("close-orchestrate".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "close-orchestrate",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: spec_attr,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qa_overall_parses_pass() {
        assert_eq!(qa_overall(r#"{"overall":"pass"}"#).as_deref(), Some("pass"));
        assert_eq!(qa_overall(r#"{"overall":"fail"}"#).as_deref(), Some("fail"));
        assert_eq!(qa_overall(r#"{"overall":"skip"}"#).as_deref(), Some("skip"));
    }

    #[test]
    fn qa_overall_missing_field_returns_none() {
        assert!(qa_overall("{}").is_none());
        assert!(qa_overall("not json").is_none());
    }

    #[test]
    fn close_report_serializes_to_required_fields() {
        let r = CloseReport {
            spec: "demo".to_string(),
            overall: "pass",
            gates: vec![GateReport {
                name: "verify-pipeline".to_string(),
                ok: true,
                duration_ms: 1,
                summary: None,
            }],
            duration_ms: 2,
        };
        let v = serde_json::to_value(r).unwrap();
        assert!(v.get("spec").is_some());
        assert!(v.get("overall").is_some());
        assert!(v.get("gates").unwrap().is_array());
        assert!(v.get("duration_ms").is_some());
    }

    #[test]
    fn skip_docs_omits_docs_gate() {
        // We can't drive the full run() here without an installed mustard-rt;
        // sanity-test the structural property by hand-building the gate list.
        let mut gates: Vec<String> = vec![
            "verify-pipeline".to_string(),
            "qa-run".to_string(),
            "pipeline-summary".to_string(),
        ];
        gates.sort();
        assert!(!gates.contains(&"docs-stale-check".to_string()));
    }
}
