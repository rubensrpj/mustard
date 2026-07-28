//! `mustard-rt run close-pipeline` — composite CLOSE face: review verdicts +
//! QA + (only on QA pass) the terminal complete + summary, in one report.
//!
//! Composes, **in-process** (module-qualified, no subprocess):
//!
//! 1. **Reviews (advisory)** — every `review.result` event found in the spec's
//!    per-spec NDJSON log, listed chronologically. Purely informational: a
//!    rejected review does not block this composite (the REVIEW loop owns the
//!    retry policy).
//! 2. **QA** — [`crate::commands::review::qa_run::run_qa_with_options`]
//!    (`self_invoked: true`, since this process IS `mustard-rt` and an AC may
//!    try to rebuild it). Emits the `qa.result` event exactly like the
//!    standalone `qa-run`.
//! 3. **The CONFIRMATION pass — only with `overall == "pass"`** —
//!    [`crate::commands::review::ac_negative_check::confirm`]. Every criterion
//!    proven RED at plan time is run AGAIN, now that its work has landed, and
//!    must come back green; the verdict lands in `<spec>/ac-proof.json`'s second
//!    column and in this report. See § The confirmation below.
//! 4. **Complete + summary — only with `overall == "pass"`** —
//!    [`crate::commands::spec::complete_spec::finalize`] (the QA-less tail of
//!    `run_complete`; QA already ran above) then
//!    [`crate::commands::pipeline::pipeline_summary::build_for_dir`].
//!
//! ## The confirmation
//!
//! The red proof is half an answer: a criterion that failed before its work
//! existed has never been asked whether it passes now. `ac-negative-check
//! --confirm` is the half that asks — and until this composite took it, nothing
//! in the pipeline ever did, so the flag existed and no wave ever used it. CLOSE
//! is the one moment the question is due: the work has landed, and this is where
//! the spec stops being open.
//!
//! Two properties, both deliberate:
//!
//! - **It is TAKEN, not derived.** QA ran the same commands moments earlier, and
//!   reading its verdict back would have been cheaper. But the ledger's second
//!   column would then record a pass nobody took, which is precisely the habit
//!   this composite is closing. The confirmation runs the criteria itself,
//!   through the same engine `--confirm` runs.
//! - **It is ADVISORY, and says so.** It does not gate `completed`: QA already
//!   blocks the close on the same commands, and a ledger predating the second
//!   column carries no red proof to confirm — reported as `unproven` with the
//!   reason naming the missing RED proof, never silently as a pass. `ok` is
//!   `null`, not `false`, when the pass was not taken at all: NOT TAKEN and
//!   TAKEN-AND-FAILED ask for opposite actions.
//!
//! QA `fail` **or** `skip` → `completed: false`, `summary: null`, and the
//! failed/skipped ACs stay visible in `qa.criteria` — the spec is NOT closed.
//! No bypass is passed anywhere: the `pipeline.complete` QA gate inside
//! `emit-pipeline` remains the standing authority for any later close attempt.
//!
//! ## Output
//!
//! ```json
//! {
//!   "completed": true,
//!   "confirmation": { "taken": true, "ok": true, "confirmed": 8,
//!                     "unconfirmed": 0, "unproven": [], "reason": null },
//!   "qa": { "overall": "pass", "criteria": [...] },
//!   "reviews": [ { "critical": 0, "subproject": null, "verdict": "approved" } ],
//!   "summary": { "done": [...], "left": [...], "nextSteps": [...], "followUps": [...] }
//! }
//! ```
//!
//! Keys serialize sorted (serde_json default map). The report carries no
//! timestamps; `qa.criteria` keeps the standalone `qa-run` per-criterion shape
//! (which includes each AC's measured `duration_ms`, the existing contract).

use crate::commands::pipeline::{dispatch_plan, pipeline_summary};
use crate::commands::review::ac_negative_check::{self, Verdict};
use crate::commands::review::qa_run::{self, QaRunOptions};
use crate::commands::spec::complete_spec;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// CLI entry — `mustard-rt run close-pipeline --spec <slug>`.
pub fn run(spec: &str) {
    let cwd = PathBuf::from(crate::shared::context::project_dir());
    let report = close(&cwd, spec);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
    );
}

/// The composite miolo against an explicit `cwd` root (testable without
/// mutating the process cwd). Returns the report Value [`run`] prints.
pub(crate) fn close(cwd: &Path, spec: &str) -> Value {
    // 1. Reviews — advisory listing of every review.result verdict.
    let reviews = collect_review_verdicts(cwd, spec);

    // 2. QA — the in-process run (emits qa.result, writes the sidecar/report).
    let qa = qa_run::run_qa_with_options(cwd, spec, QaRunOptions { self_invoked: true });
    let qa_json = json!({
        "overall": qa.overall,
        "criteria": qa_run::criteria_json(&qa.criteria),
    });

    // 3. Only a hard pass closes. `skip` (no AC / nothing ran) is NOT a pass
    //    here — an unverified spec must not be finalized by the composite.
    let (completed, summary, confirmation) = if qa.overall == "pass" {
        // The confirmation is TAKEN before the terminal event: the ledger's
        // second column is part of what this close records, not a note appended
        // to a spec that is already closed.
        let confirmation = take_confirmation(cwd, spec);
        let complete_value = complete_spec::finalize(cwd, spec);
        let ok = complete_value
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let spec_dir = dispatch_plan::resolve_spec_dir(cwd, spec);
        // Advisory: an unreadable spec.md degrades the summary to null without
        // un-completing the close.
        let summary = pipeline_summary::build_for_dir(&spec_dir)
            .map(|(model, _header)| pipeline_summary::model_json(&model))
            .unwrap_or(Value::Null);
        (ok, summary, confirmation)
    } else {
        (false, Value::Null, confirmation_not_taken(CONFIRMATION_NOT_DUE))
    };

    json!({
        "completed": completed,
        "confirmation": confirmation,
        "qa": qa_json,
        "reviews": reviews,
        "summary": summary,
    })
}

/// Why a close reports NO confirmation: QA did not pass, so nothing closed and
/// the criteria are not due to be confirmed yet.
const CONFIRMATION_NOT_DUE: &str = "the confirmation was NOT TAKEN: QA did not pass, so this spec \
     is not closed and its criteria are not due to be confirmed — clear the failing criteria, \
     then close again";

/// TAKE the confirmation pass over `spec` and report its verdict.
///
/// Runs [`ac_negative_check::confirm_in_process`], the same engine
/// `ac-negative-check --confirm` runs, in the variant that knows it is executing
/// INSIDE `mustard-rt`: each criterion that cleared the RED proof is run again
/// and must now come back green, except one whose command rebuilds this very
/// binary — that one is reported NOT TAKEN rather than judged from a compile
/// that cannot link. The ledger is written by that call; this function only
/// shapes what the close reports.
///
/// `unproven` names every criterion the pass did NOT clear, which is the only
/// part a reader can act on. It stays advisory (see the module doc), so the
/// caller sees the list and the composite does not block on it.
fn take_confirmation(cwd: &Path, spec: &str) -> Value {
    let report = ac_negative_check::confirm_in_process(cwd, spec);
    let unproven: Vec<Value> = report
        .criteria
        .iter()
        .filter(|c| c.verdict == Verdict::Unproven)
        .map(|c| json!(c.id))
        .collect();
    json!({
        "taken": true,
        "ok": report.ok,
        "confirmed": report.confirmed,
        "unconfirmed": report.unconfirmed,
        "unproven": unproven,
        "reason": report.error,
    })
}

/// The report for a confirmation that was NEVER TAKEN.
///
/// `ok` is `null`, never `false`: a pass nobody took and a pass that came back
/// wrong ask for opposite actions, so they are never spelled the same way.
fn confirmation_not_taken(reason: &str) -> Value {
    json!({
        "taken": false,
        "ok": Value::Null,
        "confirmed": 0,
        "unconfirmed": 0,
        "unproven": [],
        "reason": reason,
    })
}

/// Every `review.result` verdict recorded for `spec`, chronological. Advisory
/// — the composite lists them verbatim (verdict / critical count /
/// subproject); it never blocks on a rejection. Fail-open: a missing events
/// dir yields `[]`.
fn collect_review_verdicts(cwd: &Path, spec: &str) -> Vec<Value> {
    let events_dir = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map_or_else(
            || {
                ClaudePaths::compose_unchecked(cwd)
                    .spec_dir()
                    .join(spec)
                    .join(".events")
            },
            |sp| sp.events_dir(),
        );
    let mut events = read_harness_events_from_ndjson_dir(&events_dir);
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    events
        .into_iter()
        .filter(|e| e.event == "review.result" && e.spec.as_deref() == Some(spec))
        .map(|e| {
            json!({
                "verdict": e.payload.get("verdict").cloned().unwrap_or(Value::Null),
                "critical": e.payload.get("criticalCount").cloned().unwrap_or(Value::Null),
                "subproject": e.payload.get("subproject").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    /// Anchor a project root so `ClaudePaths::for_project` resolves.
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    /// Seed a flat spec whose single AC runs `cmd`.
    fn seed_spec(project: &Path, slug: &str, cmd: &str) -> PathBuf {
        let spec_dir = project.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            format!(
                "# {slug}\n\n## Acceptance Criteria\n- [ ] AC-1: it runs — Command: `{cmd}`\n"
            ),
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"light","lang":"en-US"}"#,
        )
        .unwrap();
        spec_dir
    }

    /// Emit a `review.result` event for `spec` (the same shape `review-result`
    /// records).
    fn emit_review(project: &Path, spec: &str, verdict: &str, critical: i64, ts: &str) {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Cli,
                id: Some("review-result".to_string()),
                actor_type: None,
            },
            event: "review.result".to_string(),
            payload: json!({
                "spec": spec,
                "verdict": verdict,
                "criticalCount": critical,
                "subproject": null,
            }),
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(project.to_str().unwrap(), &event);
    }

    /// Happy path: a passing AC closes the spec — reviews listed, QA pass,
    /// `completed: true`, summary present, and the spec's projection + sidecar
    /// land on completed.
    #[test]
    fn composite_close_pipeline_pass_completes_and_summarises() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec = "close-pass";
        // `echo ok` exits 0 under both `cmd /c` and `sh -c`.
        let spec_dir = seed_spec(project, spec, "echo ok");
        emit_review(project, spec, "approved", 0, "2026-06-09T00:00:01.000Z");

        let report = close(project, spec);

        assert_eq!(report["qa"]["overall"], json!("pass"), "{report}");
        assert_eq!(report["completed"], json!(true), "{report}");
        // Reviews listed verbatim, advisory.
        assert_eq!(report["reviews"][0]["verdict"], json!("approved"), "{report}");
        assert_eq!(report["reviews"][0]["critical"], json!(0), "{report}");
        // Summary carries the json-format shape.
        assert!(report["summary"]["done"].is_array(), "{report}");
        assert!(report["summary"]["nextSteps"].is_array(), "{report}");

        // The close really landed: meta.json flipped to Close/Completed.
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stage"], json!("Close"), "{meta}");
        assert_eq!(meta["outcome"], json!("Completed"), "{meta}");
    }

    /// Degraded: a failing AC reports the reproved criterion and does NOT
    /// close — `completed: false`, no summary, sidecar untouched.
    #[test]
    fn composite_close_pipeline_qa_fail_does_not_close() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec = "close-fail";
        // `exit 3` exits non-zero under both `cmd /c` and `sh -c`.
        let spec_dir = seed_spec(project, spec, "exit 3");
        emit_review(project, spec, "rejected", 2, "2026-06-09T00:00:01.000Z");

        let report = close(project, spec);

        assert_eq!(report["qa"]["overall"], json!("fail"), "{report}");
        assert_eq!(report["completed"], json!(false), "{report}");
        assert_eq!(report["summary"], Value::Null, "no summary on a failed QA");
        // The reproved AC is named in the criteria.
        assert_eq!(report["qa"]["criteria"][0]["id"], json!("AC-1"), "{report}");
        assert_eq!(report["qa"]["criteria"][0]["status"], json!("fail"), "{report}");
        // Reviews stay advisory (the rejected verdict is listed, not acted on).
        assert_eq!(report["reviews"][0]["verdict"], json!("rejected"), "{report}");

        // NOT closed: sidecar still mid-pipeline.
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stage"], json!("Execute"), "{meta}");
        assert_eq!(meta["outcome"], json!("Active"), "{meta}");
        // And no pipeline.complete event landed.
        assert!(!crate::commands::event::verify_emit::verify_event_landed(
            project,
            "pipeline.complete",
            Some(spec),
            Some("1h"),
        ));
    }

    /// Seed a flat spec with TWO criteria, both running `cmd`.
    ///
    /// Two, not one: the trailing criterion is exempt by position (the
    /// build-green safety net), so a single-AC spec has nothing the
    /// confirmation pass is allowed to speak about.
    fn seed_spec_two_acs(project: &Path, slug: &str, cmd: &str) -> PathBuf {
        let spec_dir = project.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            format!(
                "# {slug}\n\n## Acceptance Criteria\n\
                 - [ ] AC-1: the behaviour is there — Command: `{cmd}`\n\
                 - [ ] AC-2: the build is green — Command: `echo ok`\n"
            ),
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"light","lang":"en-US"}"#,
        )
        .unwrap();
        spec_dir
    }

    /// Write a ledger recording AC-1 as PROVEN by a RED proof over `cmd` — the
    /// state a spec is in after `ac-negative-check` ran at plan time.
    fn seed_red_ledger(spec_dir: &Path, slug: &str, cmd: &str) {
        std::fs::write(
            spec_dir.join("ac-proof.json"),
            format!(
                r#"{{"spec":"{slug}","criteria":[{{"id":"AC-1","command":"{cmd}","expect":null,
                   "verdict":"proven","proof":"red","confirmation":"not-taken","exit":3,
                   "confirmation_exit":null,"reason":null,"stderr_excerpt":""}}],
                   "amendments":[]}}"#
            ),
        )
        .unwrap();
    }

    /// The criterion record `id` carries in the ledger on disk.
    fn ledger_entry(spec_dir: &Path, id: &str) -> Value {
        let body = std::fs::read_to_string(spec_dir.join("ac-proof.json")).unwrap();
        let ledger: Value = serde_json::from_str(&body).unwrap();
        ledger["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == json!(id))
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// AC-1 — the CLOSE composite TAKES the confirmation pass over the criteria
    /// proven red at plan time and records the verdict, instead of clearing the
    /// spec on the red proof alone.
    ///
    /// Three-sided, so no half of the assertion can pass vacuously:
    ///
    /// 1. **It is taken and recorded.** A spec whose AC-1 carries a RED proof
    ///    and whose command now passes closes with `confirmation.taken:true`,
    ///    `ok:true` — and the LEDGER ON DISK flips AC-1's second column from
    ///    `not-taken` to `green`. Asserting the file, not just the report, is
    ///    what proves the pass really ran rather than being summarised.
    /// 2. **A red proof alone does NOT clear it.** The same close over a spec
    ///    with NO recorded proof reports AC-1 by name under `unproven` with
    ///    `ok:false` — the confirmation refuses to speak for a criterion whose
    ///    red half was never taken.
    /// 3. **It is never fabricated.** A close whose QA failed reports
    ///    `taken:false` and `ok:null` (NOT `false`), and leaves the ledger's
    ///    second column untouched.
    #[test]
    fn close_pipeline_takes_the_confirmation_pass() {
        // --- 1. Taken, green, and written to the ledger -------------------
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "confirm-green";
        let spec_dir = seed_spec_two_acs(dir.path(), spec, "echo ok");
        seed_red_ledger(&spec_dir, spec, "echo ok");
        assert_eq!(
            ledger_entry(&spec_dir, "AC-1")["confirmation"],
            json!("not-taken"),
            "precondition: nobody has confirmed AC-1 yet",
        );

        let report = close(dir.path(), spec);

        assert_eq!(report["qa"]["overall"], json!("pass"), "{report}");
        assert_eq!(report["completed"], json!(true), "{report}");
        assert_eq!(report["confirmation"]["taken"], json!(true), "{report}");
        assert_eq!(report["confirmation"]["ok"], json!(true), "{report}");
        assert_eq!(report["confirmation"]["confirmed"], json!(1), "{report}");
        assert_eq!(report["confirmation"]["unconfirmed"], json!(0), "{report}");
        assert_eq!(report["confirmation"]["unproven"], json!([]), "{report}");
        // The pass really ran: the ledger's second column moved.
        let entry = ledger_entry(&spec_dir, "AC-1");
        assert_eq!(entry["confirmation"], json!("green"), "{entry}");
        // …and the RED column it was earned with is untouched.
        assert_eq!(entry["proof"], json!("red"), "{entry}");

        // --- 2. No red proof to confirm ⇒ reported unproven, by name ------
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "confirm-no-proof";
        seed_spec_two_acs(dir.path(), spec, "echo ok");

        let report = close(dir.path(), spec);

        assert_eq!(report["qa"]["overall"], json!("pass"), "{report}");
        assert_eq!(report["confirmation"]["taken"], json!(true), "{report}");
        assert_eq!(
            report["confirmation"]["ok"],
            json!(false),
            "a criterion whose red half was never taken does not clear: {report}",
        );
        assert_eq!(report["confirmation"]["unproven"], json!(["AC-1"]), "{report}");

        // --- 3. QA failed ⇒ NOT TAKEN, and it says so --------------------
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "confirm-not-due";
        let spec_dir = seed_spec_two_acs(dir.path(), spec, "exit 3");
        seed_red_ledger(&spec_dir, spec, "exit 3");

        let report = close(dir.path(), spec);

        assert_eq!(report["qa"]["overall"], json!("fail"), "{report}");
        assert_eq!(report["completed"], json!(false), "{report}");
        assert_eq!(report["confirmation"]["taken"], json!(false), "{report}");
        assert_eq!(
            report["confirmation"]["ok"],
            Value::Null,
            "NOT TAKEN is null, never false: {report}",
        );
        assert!(
            report["confirmation"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("NOT TAKEN"),
            "the reason names what happened: {report}",
        );
        assert_eq!(
            ledger_entry(&spec_dir, "AC-1")["confirmation"],
            json!("not-taken"),
            "a confirmation nobody took writes nothing to the ledger",
        );
    }

    /// Degraded: an unknown spec degrades to QA `skip` — which is NOT a pass:
    /// `completed: false`, empty reviews, null summary.
    #[test]
    fn composite_close_pipeline_unknown_spec_skips_without_closing() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let report = close(dir.path(), "ghost-spec");
        assert_eq!(report["qa"]["overall"], json!("skip"), "{report}");
        assert_eq!(report["completed"], json!(false), "{report}");
        assert_eq!(report["reviews"], json!([]), "{report}");
        assert_eq!(report["summary"], Value::Null, "{report}");
    }
}
