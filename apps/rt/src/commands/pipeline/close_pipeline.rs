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
//! 3. **Complete + summary — only with `overall == "pass"`** —
//!    [`crate::commands::spec::complete_spec::finalize`] (the QA-less tail of
//!    `run_complete`; QA already ran above) then
//!    [`crate::commands::pipeline::pipeline_summary::build_for_dir`].
//!
use crate::commands::pipeline::{dispatch_plan, pipeline_summary};
use crate::commands::review::qa_run::{self, QaRunOptions};
use crate::commands::spec::complete_spec;
use serde_json::{json, Value};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

/// CLI entry — `mustard-rt run close-pipeline --spec <slug>`. The command has
/// left the flow: it refuses at the door with exit 1, writes nothing and says
/// to wait for the close. [`close`] keeps its body, with its tests, until the
/// command leaves.
pub fn run(_spec: &str) {
    crate::commands::retired::refuse(
        Path::new(&crate::shared::context::project_dir()),
        "wait-for-close",
        "retired.wait_close",
        &[("{command}", "close-pipeline")],
    );
}

/// The composite miolo against an explicit `cwd` root (testable without
/// mutating the process cwd). Returns the report the door used to print.
/// `session` is the one closing, read from the environment by the `run` entry.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn close(cwd: &Path, spec: &str, session: Option<&str>) -> Value {
    // 1. Reviews — advisory listing of every verdict in the spec file.
    let reviews = collect_review_verdicts(cwd, spec);

    // 2. QA — run the criteria. A pass is an OBSERVED exit code, and a run
    //    recorded in the spec file does not say which tree it ran against, so
    //    nothing recorded is reused here.
    let qa = qa_run::run_qa_with_options(cwd, spec, QaRunOptions { self_invoked: true });
    let qa_json = json!({
        "overall": qa.overall,
        "criteria": qa_run::criteria_json(&qa.criteria),
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
            "qa": qa_json,
            "reviews": reviews,
            "summary": Value::Null,
        });
    }

    let complete_value = complete_spec::finalize(cwd, spec, session);
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
        "qa": qa_json,
        "reviews": reviews,
        "summary": summary,
    })
}

/// Every verdict recorded in the spec's `spec.ndjson`, in event order — the
/// wave and its result. Advisory: the composite lists them; it never blocks on
/// a rejection. A spec with no event file yields `[]`.
fn collect_review_verdicts(cwd: &Path, spec: &str) -> Vec<Value> {
    use mustard_core::domain::spec_events::{Block, BlockQuery};
    use mustard_core::domain::spec_state::SpecState as _;
    let Some(log) = crate::shared::spec_state::DiskSpecState::new(cwd).log(spec) else {
        return Vec::new();
    };
    log.block(BlockQuery::Block(Block::Review))
        .into_iter()
        .filter(|event| event.event_type == "verdict")
        .map(|event| json!({ "wave": event.wave(), "verdict": event.str_field("result") }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// A passing run recorded in the spec file closes nothing on its own: the
    /// composite close runs the criteria, and a criterion that fails now holds
    /// the spec, whatever the file says.
    #[test]
    fn close_runs_the_criteria_whatever_the_spec_file_recorded() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        anchor(project);
        seed_spec(project, "feat", "exit 3");
        crate::shared::spec_state::seed_runs(project, "feat", &[Some("pass")]);

        let report = close(project, "feat", None);
        assert_eq!(report["qa"]["overall"], json!("fail"), "{report}");
        assert_eq!(report["completed"], json!(false), "{report}");
    }

    /// Record the verdict `verdict` of wave 1 in the spec's `spec.ndjson`.
    fn emit_review(project: &Path, spec: &str, verdict: &str) {
        let criteria = crate::shared::spec_state::seed_runs(project, spec, &[None]);
        crate::shared::spec_state::seed_verdict(project, spec, 1, verdict, criteria[0]);
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
        emit_review(project, spec, "approved");

        let report = close(project, spec, None);

        assert_eq!(report["qa"]["overall"], json!("pass"), "{report}");
        assert_eq!(report["completed"], json!(true), "{report}");
        // Reviews listed verbatim, advisory.
        assert_eq!(report["reviews"][0]["verdict"], json!("approved"), "{report}");
        assert_eq!(report["reviews"][0]["wave"], json!(1), "{report}");
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
        emit_review(project, spec, "rejected");

        let report = close(project, spec, None);

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
        // And no close reached the spec file.
        assert!(!crate::commands::event::verify_emit::closed_state_landed(project, spec, 0));
    }

    /// Degraded: an unknown spec reports QA `spec-not-found` — which is NOT a
    /// pass: `completed: false`, empty reviews, null summary.
    ///
    /// It used to read `skip`, the same word a spec with nothing to verify gets.
    /// The two demand opposite next moves (fix the slug versus author a
    /// criterion), so they no longer share a word — the close is refused either
    /// way, since anything but `pass` is.
    #[test]
    fn composite_close_pipeline_unknown_spec_skips_without_closing() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let report = close(dir.path(), "ghost-spec", None);
        assert_eq!(report["qa"]["overall"], json!("spec-not-found"), "{report}");
        assert_eq!(report["completed"], json!(false), "{report}");
        assert_eq!(report["reviews"], json!([]), "{report}");
        assert_eq!(report["summary"], Value::Null, "{report}");
    }

}
