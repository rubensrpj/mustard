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
//! - **It REPORTS, and does not gate `completed`.** QA blocked this close on
//!   the same commands moments earlier, so a confirmation red here requires the
//!   tree to change between the two runs — and the one case only this pass
//!   could see, a criterion that was never red, lands in `NotTaken`, which no
//!   honest refusal could include (it is "nobody looked", not "the work is
//!   missing"). So the report spells every column apart and the reader decides;
//!   `ok` is `null`, not `false`, when the pass was not taken at all, because
//!   NOT TAKEN and TAKEN-AND-FAILED ask for opposite actions. This was refusal
//!   for one working day and was reverted by explicit decision — the refusal
//!   could only re-catch what QA had just caught.
//!
//! ## The removal
//!
//! Red before and green after still leave a gap: a criterion can clear both
//! while asserting nothing the work actually DOES. The REMOVAL pass runs each
//! confirmed criterion against a scratch checkout with the work stripped, and
//! one still GREEN there verifies nothing. Until this composite called it,
//! nothing in the pipeline ever did — the engine's only caller outside its own
//! module was the CLI flag.
//!
//! It refuses on exactly one finding — a criterion that SURVIVED — and on
//! nothing else. A scratch tree that could not be cut is an engine error, not a
//! verdict about any criterion, and a criterion whose own evidence the strip
//! took away is one the pass declined to judge; neither withholds the close.
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
//!   "removal": { "taken": true, "ok": true, "red": 8, "survived": [],
//!                "evidence_removed": 0, "taken_away": [...], "reason": null },
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
use crate::commands::review::ac_negative_check::{self, Confirmation, Verdict};
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

    // 2. QA — reuse a still-valid record, or run.
    //
    // Mustard's law is that a pass is an OBSERVED exit code. Re-running is how
    // that used to be honoured here, unconditionally, even when the last green
    // was observed against the very tree in front of us — which is not
    // verification, it is repetition. What makes reuse honest is a key that
    // covers everything able to change the answer: `qa.result` now records the
    // fingerprint of the tree its criteria ran against, and
    // `code_state::still_current` is fail-closed, so *cannot tell* runs.
    //
    // The reuse is narrow ON PURPOSE — a recorded `pass`, nothing else. A `fail`
    // or a `skip` is re-run whatever the fingerprint says: those are the states
    // the operator is actively working out of, and answering them from a record
    // would tell someone who just fixed the code that it is still broken.
    let qa_json = reusable_qa_pass(cwd, spec).unwrap_or_else(|| {
        let qa = qa_run::run_qa_with_options(cwd, spec, QaRunOptions { self_invoked: true });
        json!({
            "overall": qa.overall,
            "criteria": qa_run::criteria_json(&qa.criteria),
        })
    });
    let qa_overall = qa_json
        .get("overall")
        .and_then(Value::as_str)
        .unwrap_or("skip")
        .to_string();

    // 3. Only a hard pass closes. `skip` (no AC / nothing ran) is NOT a pass
    //    here — an unverified spec must not be finalized by the composite.
    if qa_overall != "pass" {
        return json!({
            "completed": false,
            "confirmation": confirmation_not_taken(CONFIRMATION_NOT_DUE),
            "removal": removal_not_taken(REMOVAL_NOT_DUE),
            "qa": qa_json,
            "reviews": reviews,
            "summary": Value::Null,
        });
    }

    // The confirmation is TAKEN before the terminal event: the ledger's second
    // column is part of what this close records, not a note appended to a spec
    // that is already closed. Advisory — QA gated the same commands moments
    // earlier, so the report spells the verdicts apart and gates nothing.
    let confirmation = take_confirmation(cwd, spec);

    // The THIRD transition, taken here because nothing else ever took it: red
    // before and green after both clear a criterion that asserts nothing the
    // work actually did, and only removing the work again catches it.
    let (removal, removal_blockers) = take_removal(cwd, spec);
    if !removal_blockers.is_empty() {
        return json!({
            "completed": false,
            "confirmation": confirmation,
            "removal": removal,
            "qa": qa_json,
            "reviews": reviews,
            "summary": Value::Null,
        });
    }

    let complete_value = complete_spec::finalize(cwd, spec);
    let completed = complete_value
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let spec_dir = dispatch_plan::resolve_spec_dir(cwd, spec);
    // Advisory: an unreadable spec.md degrades the summary to null without
    // un-completing the close.
    let summary = pipeline_summary::build_for_dir(&spec_dir)
        .map(|(model, _header)| pipeline_summary::model_json(&model))
        .unwrap_or(Value::Null);

    json!({
        "completed": completed,
        "confirmation": confirmation,
        "removal": removal,
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
/// part a reader can act on — the report is advisory, and the module doc says
/// why: QA gated the same commands moments earlier, and the one case only this
/// pass could see lands in [`Confirmation::NotTaken`], which no honest refusal
/// could include.
fn take_confirmation(cwd: &Path, spec: &str) -> Value {
    let report = ac_negative_check::confirm_in_process(cwd, spec);
    let unproven: Vec<Value> = report
        .criteria
        .iter()
        .filter(|c| c.verdict == Verdict::Unproven)
        .map(unproven_entry)
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

/// Why a close reports NO removal: QA did not pass, so nothing closed and the
/// scratch tree was never due to be cut.
const REMOVAL_NOT_DUE: &str = "the REMOVAL was NOT TAKEN: QA did not pass, so this spec is not \
     closed and no scratch tree was due to be cut — clear the failing criteria, then close again";

/// TAKE the third transition over `spec` and report its verdict.
///
/// Runs [`ac_negative_check::take_removal`], the same engine
/// `ac-negative-check --removal` runs: each CONFIRMED criterion is run again
/// against a scratch checkout with the work taken away, and must come back red.
/// Until this call existed the only reference to that engine outside its own
/// module was the CLI flag, so [`ac_negative_check::Removal::Survived`] — the
/// finding the pass exists to produce — was a value no pipeline could reach.
///
/// The second return value is what the close is REFUSED on: the criteria that
/// SURVIVED the removal. One of those is satisfied by something the work did
/// not do, which is a criterion verifying nothing.
///
/// Everything else is advisory, and the direction is deliberate. A scratch tree
/// that could not be cut is an ENGINE error, never a verdict about a criterion
/// — the report carries the reason and the close proceeds, because refusing a
/// close over a `git worktree` failure would name an action about the criterion
/// that has nothing to do with it. `EvidenceRemoved` is likewise not a finding:
/// the pass declined to judge, and turning silence into a refusal is the same
/// error as turning it into a proof.
fn take_removal(cwd: &Path, spec: &str) -> (Value, Vec<String>) {
    let from = ac_negative_check::default_removal_from(cwd);
    let report = ac_negative_check::take_removal(cwd, spec, &from);
    if let Some(error) = report.error {
        return (removal_not_taken(&error), Vec::new());
    }
    let survived: Vec<String> = report
        .criteria
        .iter()
        .filter(|c| c.removal == ac_negative_check::Removal::Survived)
        .map(|c| c.id.clone())
        .collect();
    let value = json!({
        "taken": true,
        "ok": survived.is_empty(),
        "red": report.removed_red,
        "survived": survived.clone(),
        "evidence_removed": report.evidence_removed,
        "taken_away": report.taken_away,
        "reason": Value::Null,
    });
    (value, survived)
}

/// The report for a removal that was NEVER TAKEN — because it was not due, or
/// because the scratch tree could not be cut.
///
/// `ok` is `null`, never `false`, for the reason [`confirmation_not_taken`]
/// gives: a pass nobody took and a pass that came back wrong ask for opposite
/// actions.
fn removal_not_taken(reason: &str) -> Value {
    json!({
        "taken": false,
        "ok": Value::Null,
        "red": 0,
        "survived": [],
        "evidence_removed": 0,
        "taken_away": [],
        "reason": reason,
    })
}

/// One unproven criterion's entry in the close report: its id, the confirmation
/// column verbatim, and what that column SAYS ([`unproven_wording`]).
///
/// The entry used to be the bare id, which is why this exists at all — see
/// [`unproven_wording`] for what that collapsed.
fn unproven_entry(c: &ac_negative_check::AcProof) -> Value {
    json!({
        "id": c.id,
        "confirmation": c.confirmation,
        "says": unproven_wording(c.confirmation),
    })
}

/// What an unproven criterion's `confirmation` column actually says, in words.
///
/// The list this feeds used to be bare ids, which collapsed the two cases the
/// [`ac_negative_check::Confirmation`] enum exists to keep apart: a pass that
/// was NEVER TAKEN and a pass that was taken and came back RED. Its own doc
/// states they "ask for opposite actions — take the confirmation, versus finish
/// (or repair) the work", and until now only the ledger on disk drew that line;
/// the close report — the thing an operator actually reads — spelled both the
/// same. Every arm below names the ONE action that clears its case, and no two
/// arms open with the same words.
fn unproven_wording(confirmation: Confirmation) -> &'static str {
    match confirmation {
        Confirmation::NotTaken => {
            "NEVER TAKEN: the confirmation pass did not reach this criterion, so nothing is \
             known about it after its work landed — take the confirmation"
        }
        Confirmation::Red => {
            "TAKEN and came back RED: the command ran after its work landed and still failed — \
             finish the work, or repair a command that never asserted it"
        }
        Confirmation::NoVerdict => {
            "TAKEN but killed by its deadline: no verdict ever arrived — re-run it, or give the \
             criterion a command that can finish"
        }
        Confirmation::Inexecutable => {
            "TAKEN and could not be attempted AT ALL: the criterion itself is broken rather than \
             unmet — amend it with a replacement that runs"
        }
        // A green confirmation is what makes a criterion proven, so an unproven
        // entry cannot carry one. Say exactly that rather than invent an action.
        Confirmation::Green => {
            "GREEN confirmation on an UNPROVEN criterion — the ledger contradicts itself; re-take \
             the confirmation pass"
        }
    }
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

/// The last recorded `qa.result` for `spec` when it is a `pass` that still
/// describes THIS tree — shaped exactly like the `qa` block a fresh run
/// produces, so the report cannot tell the two apart. `None` means run.
///
/// Every one of these conditions must hold, and each is a way the answer could
/// otherwise be wrong rather than merely cheap:
///
/// | Condition | Why it is not optional |
/// |---|---|
/// | a `qa.result` exists for this spec | nothing to reuse |
/// | its `overall` is `pass` | a `fail`/`skip` is the state the operator is working out of; answering it from a record tells someone who just fixed the code that it is still broken |
/// | it carries a `codeState` | a record with no key describes no particular tree |
/// | that key still matches | otherwise the green was observed against something else |
///
/// The comparison itself is fail-closed
/// ([`crate::shared::code_state::still_current`]): no repository, no `git`, any
/// read error — all answer *cannot tell*, which runs.
fn reusable_qa_pass(cwd: &Path, spec: &str) -> Option<Value> {
    let events_dir = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())?;
    let mut events = read_harness_events_from_ndjson_dir(&events_dir);
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    let last = events.into_iter().rfind(|e| {
        e.event == "qa.result"
            && e.payload
                .get("spec")
                .and_then(Value::as_str)
                .is_none_or(|s| s == spec)
    })?;

    if last.payload.get("overall").and_then(Value::as_str) != Some("pass") {
        return None;
    }
    let recorded = last
        .payload
        .get(crate::shared::code_state::CODE_STATE_KEY)
        .and_then(Value::as_str)?;
    if !crate::shared::code_state::still_current(cwd, Some(recorded)) {
        return None;
    }
    Some(json!({
        "overall": "pass",
        "criteria": last.payload.get("criteria").cloned().unwrap_or_else(|| json!([])),
        // The one field that differs from a fresh run, and it must: a reader
        // deciding whether to trust this close has to be able to see that
        // nothing was executed here.
        "reused": true,
    }))
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

    /// Emit a `qa.result` for `spec`, optionally carrying a `codeState`
    /// fingerprint — the same shape `qa-run` records.
    fn emit_qa(project: &Path, spec: &str, overall: &str, code_state: Option<&str>) {
        let mut payload = json!({
            "spec": spec,
            "overall": overall,
            "criteria": [{ "id": "AC-1", "status": "pass" }],
        });
        if let (Some(map), Some(state)) = (payload.as_object_mut(), code_state) {
            map.insert(
                crate::shared::code_state::CODE_STATE_KEY.to_string(),
                json!(state),
            );
        }
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-01-01T00:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Cli,
                id: Some("qa-run".to_string()),
                actor_type: None,
            },
            event: "qa.result".to_string(),
            payload,
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(project.to_str().unwrap(), &event);
    }

    /// Turn `project` into a git repository, so a fingerprint can be taken.
    fn repo(project: &Path) {
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "t@example.com"][..],
            &["config", "user.name", "t"][..],
        ] {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(project)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed");
        }
        std::fs::write(project.join("src.rs"), "fn a() {}\n").unwrap();
        for args in [&["add", "-A"][..], &["commit", "-qm", "init"][..]] {
            let _ = std::process::Command::new("git")
                .args(args)
                .current_dir(project)
                .output();
        }
    }

    /// **A green already observed against THIS tree is not re-run.** The close
    /// used to re-execute every criterion unconditionally, even when the last
    /// pass was recorded against the very tree in front of it — repetition, not
    /// verification.
    ///
    /// The criterion here would FAIL if executed (`exit 1`), so a `pass` in the
    /// report can only have come from the record: the test cannot be satisfied
    /// by accidentally re-running.
    #[test]
    fn close_reuses_a_fresh_qa_record_instead_of_running() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        anchor(project);
        repo(project);
        seed_spec(project, "feat", "sh -c 'exit 1'");
        let state = crate::shared::code_state::fingerprint(project).expect("a repo has one");
        emit_qa(project, "feat", "pass", Some(&state));

        let reused =
            reusable_qa_pass(project, "feat").expect("the record still describes the tree");
        assert_eq!(reused.get("overall").and_then(Value::as_str), Some("pass"));
        assert_eq!(
            reused.get("reused").and_then(Value::as_bool),
            Some(true),
            "a reader must be able to see that nothing was executed"
        );
    }

    /// The three ways reuse is refused, each one a way the answer could be
    /// wrong rather than merely stale.
    #[test]
    fn close_refuses_to_reuse_a_record_it_cannot_trust() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        anchor(project);
        repo(project);
        seed_spec(project, "feat", "true");

        // 1. No fingerprint at all — the record describes no particular tree.
        emit_qa(project, "feat", "pass", None);
        assert!(reusable_qa_pass(project, "feat").is_none());

        // 2. A fingerprint that matches nothing — the green was observed
        //    elsewhere. THIS is the hole: without the key, this record looked
        //    exactly like a fresh one.
        emit_qa(project, "feat", "pass", Some("deadbeefdeadbeef"));
        assert!(reusable_qa_pass(project, "feat").is_none());

        // 3. A matching fingerprint on a NON-pass. The operator is working out
        //    of that state; answering from the record would tell someone who
        //    just fixed the code that it is still broken.
        let state = crate::shared::code_state::fingerprint(project).expect("a repo has one");
        emit_qa(project, "feat", "fail", Some(&state));
        assert!(reusable_qa_pass(project, "feat").is_none());
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
        // The entry names the criterion AND what its confirmation column says —
        // the two unproven cases are never spelled alike (see
        // `close_report_spells_the_two_unproven_cases_apart`).
        assert_eq!(report["confirmation"]["unproven"][0]["id"], json!("AC-1"), "{report}");
        assert_eq!(
            report["confirmation"]["unproven"][1],
            Value::Null,
            "exactly one criterion is unproven: {report}",
        );

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

    /// A still-red confirmation is REPORTED, verbatim and by name — and the
    /// close is not withheld on it.
    ///
    /// The advisory direction is a decision, not an omission (2026-07-29,
    /// reverting a refusal that shipped for one working day): QA gates the same
    /// commands moments earlier in this composite, so a red here requires the
    /// tree to change between the two runs, and the one case only this pass
    /// could see — a criterion that was never red — lands in `NotTaken`, which
    /// the refusal excluded anyway. The removal pass keeps the composite's only
    /// refusal (see `close_takes_the_removal_pass`).
    ///
    /// `mkdir` is the criterion, chosen for one property: it exits 0 the first
    /// time and non-zero the second, on `cmd.exe` and `sh` alike. So QA (which
    /// runs first) sees a green criterion and reports `pass`, and the
    /// confirmation — running the same command moments later — sees it red:
    /// exactly the state whose reporting this asserts.
    #[test]
    fn close_reports_a_still_red_criterion_without_withholding() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "confirm-reports";
        let spec_dir = seed_spec_two_acs(dir.path(), spec, "mkdir confirm-probe");
        seed_red_ledger(&spec_dir, spec, "mkdir confirm-probe");

        let report = close(dir.path(), spec);

        assert_eq!(report["qa"]["overall"], json!("pass"), "QA itself passed: {report}");
        // The still-red criterion is named, with what its column says.
        assert_eq!(report["confirmation"]["taken"], json!(true), "{report}");
        assert_eq!(report["confirmation"]["ok"], json!(false), "{report}");
        assert_eq!(report["confirmation"]["unproven"][0]["id"], json!("AC-1"), "{report}");
        assert!(
            report["confirmation"]["unproven"][0]["says"]
                .as_str()
                .unwrap_or_default()
                .contains("came back RED"),
            "the wording names the red, not a generic unproven: {report}",
        );
        // And the close is NOT withheld on it: QA passed, so the spec closes.
        assert_eq!(
            report["completed"],
            json!(true),
            "the confirmation reports; only the removal refuses: {report}",
        );
        // The close really landed.
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stage"], json!("Close"), "{meta}");
        assert_eq!(meta["outcome"], json!("Completed"), "{meta}");
    }

    /// Run `git` in `cwd`, discarding output — the fixture side of the removal
    /// test, mirroring `work_removed`'s own.
    fn git(cwd: &Path, args: &[&str]) {
        let _ = std::process::Command::new("git").args(args).current_dir(cwd).output();
    }

    /// AC-2 — the CLOSE composite TAKES the removal pass, and a criterion that
    /// SURVIVES the removal of its own work refuses the close.
    ///
    /// Before this caller existed the engine's only reference outside its own
    /// module was the CLI flag, so `Removal::Survived` — the finding the pass
    /// exists to produce — was a value no pipeline could reach.
    ///
    /// Two-sided:
    ///
    /// 1. **Survived refuses.** A real repo, a real scratch tree: the work
    ///    edits `probe.txt`, the criterion is `echo ok` — green with the work
    ///    and green without it, which is a criterion verifying nothing. The
    ///    close reports it under `removal.survived` and does NOT complete, and
    ///    the ledger's third column records `survived` on disk.
    /// 2. **An engine error never refuses.** The same close where no wave ever
    ///    cached a diff reports the removal NOT TAKEN with the reason, and the
    ///    close still lands — a scratch tree that could not be cut is a fact
    ///    about the engine, not about any criterion.
    #[test]
    fn close_takes_the_removal_pass() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            return; // no git on PATH — the engine's own guard, mirrored.
        }

        // --- 1. A survivor refuses the close -------------------------------
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let root = dir.path();
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.email", "t@e.x"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["config", "core.autocrlf", "false"]);
        std::fs::write(root.join("probe.txt"), "before\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "base"]);
        // THE WORK, on its own branch so the merge-base against the primary
        // base (`main` — empty `mustard.json#git.flow`) is the base commit.
        git(root, &["checkout", "-b", "work"]);
        std::fs::write(root.join("probe.txt"), "after survived_marker\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "work"]);

        let spec = "removal-survivor";
        let spec_dir = seed_spec_two_acs(root, spec, "echo ok");
        seed_red_ledger(&spec_dir, spec, "echo ok");
        // The record the pipeline already keeps: the wave's cached diff.
        std::fs::create_dir_all(spec_dir.join("wave-1-rt")).unwrap();
        std::fs::write(
            spec_dir
                .join("wave-1-rt")
                .join(crate::commands::review::work_removed::WAVE_DIFF),
            "- `probe.txt` (modified)\n",
        )
        .unwrap();

        let report = close(root, spec);

        assert_eq!(report["qa"]["overall"], json!("pass"), "{report}");
        assert_eq!(report["confirmation"]["ok"], json!(true), "{report}");
        assert_eq!(report["removal"]["taken"], json!(true), "the pass has a caller: {report}");
        assert_eq!(
            report["removal"]["survived"][0],
            json!("AC-1"),
            "green without its work is a criterion verifying nothing: {report}",
        );
        assert_eq!(report["completed"], json!(false), "a survivor refuses the close: {report}");
        assert_eq!(report["summary"], Value::Null, "{report}");
        // Recorded, not just reported: the ledger's third column moved.
        assert_eq!(ledger_entry(&spec_dir, "AC-1")["removal"], json!("survived"), "{report}");
        // The spec really is NOT closed.
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["outcome"], json!("Active"), "{meta}");

        // --- 2. An engine error reports and never refuses ------------------
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "removal-engine-error";
        let spec_dir = seed_spec_two_acs(dir.path(), spec, "echo ok");
        seed_red_ledger(&spec_dir, spec, "echo ok");

        let report = close(dir.path(), spec);

        assert_eq!(report["removal"]["taken"], json!(false), "{report}");
        assert!(
            report["removal"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("no-cached-diff"),
            "the reason names which half failed: {report}",
        );
        assert_eq!(
            report["completed"],
            json!(true),
            "an engine error is a fact about the engine, not the criteria: {report}",
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

    /// A criterion record with only the fields this report reads.
    fn proof(id: &str, confirmation: Confirmation) -> ac_negative_check::AcProof {
        ac_negative_check::AcProof {
            id: id.to_string(),
            command: "cargo test -p mustard-rt something".to_string(),
            expect: None,
            control_command: None,
            verdict: Verdict::Unproven,
            proof: ac_negative_check::Proof::Red,
            control: ac_negative_check::Control::NotDeclared,
            control_exit: None,
            confirmation,
            exit: Some(1),
            confirmation_exit: None,
            removal: ac_negative_check::Removal::NotTaken,
            removal_exit: None,
            reason: None,
            stderr_excerpt: String::new(),
            proof_tree: None,
        }
    }

    /// AC-3: the close report spells its two unproven cases apart.
    ///
    /// `Confirmation`'s own doc says [`Confirmation::NotTaken`] and
    /// [`Confirmation::Red`] "ask for opposite actions — take the confirmation,
    /// versus finish (or repair) the work". The ledger on disk drew that line;
    /// the report an operator actually reads emitted both as a bare id under one
    /// key, so a pass nobody took looked exactly like work that came back red.
    #[test]
    fn close_report_spells_the_two_unproven_cases_apart() {
        let never = unproven_entry(&proof("AC-1", Confirmation::NotTaken));
        let red = unproven_entry(&proof("AC-2", Confirmation::Red));

        // The id is still there — this widens the entry, it does not replace it.
        assert_eq!(never["id"], json!("AC-1"), "{never}");
        assert_eq!(red["id"], json!("AC-2"), "{red}");

        // The column itself, verbatim, so a machine reader never has to parse
        // the sentence.
        assert_eq!(never["confirmation"], json!("not-taken"), "{never}");
        assert_eq!(red["confirmation"], json!("red"), "{red}");

        // And the sentence a human reads, which is what AC-3 is about.
        let never_says = never["says"].as_str().unwrap_or_default();
        let red_says = red["says"].as_str().unwrap_or_default();
        assert_ne!(never_says, red_says, "the two unproven cases read alike");
        assert!(never_says.contains("NEVER TAKEN"), "{never_says}");
        assert!(red_says.contains("came back RED"), "{red_says}");
        assert!(
            !never_says.contains("came back RED"),
            "a pass nobody took must not read as a failure: {never_says}"
        );
        assert!(
            !red_says.contains("NEVER TAKEN"),
            "a failure must not read as a pass nobody took: {red_says}"
        );

        // Every other unproven column is distinct too — three shapes collapsing
        // into two is the same defect one step down.
        let all: Vec<String> = [
            Confirmation::NotTaken,
            Confirmation::Red,
            Confirmation::NoVerdict,
            Confirmation::Inexecutable,
            Confirmation::Green,
        ]
        .iter()
        .map(|c| unproven_wording(*c).to_string())
        .collect();
        let mut unique = all.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), all.len(), "two columns share a wording: {all:?}");
    }

    /// Every path under `root`, relative and slash-normalised, sorted.
    fn tree_snapshot(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
                if path.is_dir() {
                    walk(&path, root, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    /// AC-6: closing a spec removes no file outside that spec's own directory.
    ///
    /// Observed twice in one session: `MUSTARD-COMMANDS.md` (716 lines) and
    /// `install-retrieval.ps1` vanished from the repository root — once after an
    /// implementer returned, once during a close — and were restored byte-exact
    /// both times. No code under `apps/rt/src` names either file, so the
    /// invariant is what has to be held, not a call site.
    ///
    /// The check is a whole-tree before/after over a real `close`, not a probe
    /// of the two file names: a deletion this test is meant to catch would not
    /// announce itself by touching the same files twice. The close is driven to
    /// its LONGEST path — QA passes, so the confirmation, the finalize and the
    /// summary all run — because the short-circuit path executes almost nothing.
    #[test]
    fn close_removes_no_file_outside_the_spec_dir() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec = "close-keeps-the-tree";
        seed_spec(project, spec, "echo ok");
        emit_review(project, spec, "approved", 0, "2026-06-09T00:00:01.000Z");

        // Root files with nothing to do with this spec — the shape of the two
        // that disappeared.
        std::fs::write(project.join("MUSTARD-COMMANDS.md"), b"# commands\n").unwrap();
        std::fs::write(project.join("install-retrieval.ps1"), b"Write-Host ok\n").unwrap();
        std::fs::create_dir_all(project.join("apps/rt/src")).unwrap();
        std::fs::write(project.join("apps/rt/src/main.rs"), b"fn main() {}\n").unwrap();

        let before = tree_snapshot(project);
        let report = close(project, spec);
        assert_eq!(report["completed"], json!(true), "the close must really run: {report}");
        let after = tree_snapshot(project);

        let spec_prefix = format!(".claude/spec/{spec}");
        let gone: Vec<&String> = before
            .iter()
            .filter(|p| !after.contains(*p))
            .filter(|p| !p.starts_with(&spec_prefix))
            .collect();
        assert!(gone.is_empty(), "the close removed files outside its spec dir: {gone:?}");
    }
}
