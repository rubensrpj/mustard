//! `mustard-rt run review-dispatch` — orchestrate the REVIEW phase steps.
//!
//! Replaces the imperative steps in `review/SKILL.md`: emit `review.start`,
//! prefetch the PR via `review-prefetch`, render the diff via `diff-context`,
//! and emit `review.complete` after the SKILL/Task call. Each step is
//! independent — a failure in one does not prevent later ones from running.
//!
//! ## Output shape
//!
//! ```json
//! {
//!   "pr":         42,
//!   "spec":       "<slug>",
//!   "steps":      [
//!     { "name": "review.start",  "ok": true, "duration_ms": 2 },
//!     { "name": "prefetch",      "ok": true, "duration_ms": 350 },
//!     { "name": "diff-context",  "ok": true, "duration_ms": 47 }
//!   ],
//!   "prefetch":   { ... }, // the parsed prefetch JSON when available
//!   "diff":       "...",   // raw diff body when available
//!   "duration_ms": 401
//! }
//! ```
//!
//! The harness consumer reads `prefetch` + `diff` to seed the review agent
//! prompt. `review.complete` is emitted by a follow-up call (this subcommand
//! covers the *dispatch* half — the verdict half is `review-result`).

use crate::shared::context::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::platform::process::rtk_command;
use serde::Serialize;
use serde_json::{json, Value};

/// Options for `mustard-rt run review-dispatch`.
#[derive(Debug, Clone)]
pub struct ReviewDispatchOpts {
    /// PR number (positional or `--pr N`).
    pub pr: u64,
    /// Spec slug for event attribution.
    pub spec: Option<String>,
    /// Subproject to scope the diff to.
    pub subproject: Option<String>,
}

/// One step in the dispatch sequence.
#[derive(Debug, Serialize)]
pub struct DispatchStep {
    pub name: String,
    pub ok: bool,
    pub duration_ms: u64,
}

/// Aggregate report.
#[derive(Debug, Serialize)]
pub struct DispatchReport {
    pub pr: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    pub steps: Vec<DispatchStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    pub duration_ms: u64,
}

/// Run a subcommand, capture (ok, dur, stdout).
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

/// CLI entry.
pub fn run(opts: ReviewDispatchOpts) {
    let started = std::time::Instant::now();
    let mut steps: Vec<DispatchStep> = Vec::new();

    // 1. Emit `review.start`.
    let pr_str = opts.pr.to_string();
    let mut start_args: Vec<&str> = vec![
        "emit-event",
        "--event",
        "review.start",
        "--payload",
        "target=",
    ];
    // We append target=N as a single payload entry by replacing the empty stub.
    let target_payload = format!("target={pr_str}");
    start_args.pop();
    start_args.push(&target_payload);
    if let Some(spec) = opts.spec.as_deref() {
        start_args.extend_from_slice(&["--spec", spec]);
    }
    let (ok, dur, _) = run_subcmd(&start_args);
    steps.push(DispatchStep {
        name: "review.start".to_string(),
        ok,
        duration_ms: dur,
    });

    // 2. Prefetch PR data.
    let (ok, dur, pf_out) = run_subcmd(&["review-prefetch", &pr_str, "--format", "json"]);
    steps.push(DispatchStep {
        name: "prefetch".to_string(),
        ok,
        duration_ms: dur,
    });
    let prefetch: Option<Value> = serde_json::from_str(pf_out.trim()).ok();

    // 3. Diff context.
    let mut diff_args: Vec<&str> = vec!["diff-context", "--phase", "execute"];
    if let Some(sub) = opts.subproject.as_deref() {
        diff_args.extend_from_slice(&["--subproject", sub]);
    }
    let (ok, dur, diff_out) = run_subcmd(&diff_args);
    steps.push(DispatchStep {
        name: "diff-context".to_string(),
        ok,
        duration_ms: dur,
    });

    let total = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let report = DispatchReport {
        pr: opts.pr,
        spec: opts.spec.clone(),
        steps,
        prefetch,
        diff: if diff_out.trim().is_empty() {
            None
        } else {
            Some(diff_out)
        },
        duration_ms: total,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(total, opts.spec.as_deref());
}

fn emit_economy(duration_ms: u64, spec: Option<&str>) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec_attr = spec.map(str::to_string).or_else(|| current_spec(&cwd));
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("review-dispatch".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "review-dispatch",
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
    fn dispatch_report_serializes_to_required_fields() {
        let r = DispatchReport {
            pr: 42,
            spec: Some("demo".to_string()),
            steps: vec![DispatchStep {
                name: "review.start".to_string(),
                ok: true,
                duration_ms: 1,
            }],
            prefetch: Some(json!({"title": "x"})),
            diff: Some("diff body".to_string()),
            duration_ms: 2,
        };
        let v = serde_json::to_value(r).unwrap();
        assert_eq!(v["pr"], json!(42));
        assert!(v.get("steps").unwrap().is_array());
        assert!(v.get("prefetch").is_some());
        assert!(v.get("diff").is_some());
    }

    #[test]
    fn optional_fields_skip_when_missing() {
        let r = DispatchReport {
            pr: 1,
            spec: None,
            steps: Vec::new(),
            prefetch: None,
            diff: None,
            duration_ms: 0,
        };
        let v = serde_json::to_value(r).unwrap();
        // `serde(skip_serializing_if = ...)` keeps the JSON compact.
        assert!(v.get("spec").is_none());
        assert!(v.get("prefetch").is_none());
        assert!(v.get("diff").is_none());
    }

    #[test]
    fn step_name_is_serialized() {
        let s = DispatchStep {
            name: "prefetch".to_string(),
            ok: false,
            duration_ms: 99,
        };
        let v = serde_json::to_value(s).unwrap();
        assert_eq!(v["name"], json!("prefetch"));
        assert_eq!(v["ok"], json!(false));
        assert_eq!(v["duration_ms"], json!(99));
    }
}
