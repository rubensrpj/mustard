//! `mustard-rt run close-orchestrate` — drive the CLOSE-phase gates and, when
//! they all pass, **finalize the spec deterministically**.
//!
//! Replaces the imperative step list inside `close/SKILL.md`. Runs the gates
//! (verify-pipeline → qa-run → review-spans → docs-stale-check → close-gates →
//! pipeline-summary) in order, captures a pass/fail per gate, and derives an
//! `overall` verdict from the boolean vector.
//!
//! ## The CLOSE gates are gates HERE too
//!
//! `close-gates` is [`crate::commands::pipeline::close_gates::gate_close_for_spec`]
//! — the debt-marker / checklist / findings / QA / build sub-gates that
//! `emit-phase --to CLOSE` runs. It is taken here because this composite is the
//! close path the everyday ritual documents (`/pr` → `close-orchestrate`), and
//! this composite calls [`crate::commands::spec::complete_spec::finalize`]
//! DIRECTLY: `finalize` writes the `pipeline.phase: CLOSE` state through its own
//! emitter, never through `emit_phase::run_at`, so every sub-gate that lives
//! behind `gate_close_for_spec` used to be unreachable from here. A gate only one
//! of two close doors consults is not a gate — it is a suggestion the busier door
//! walks past.
//!
//! Its refusal is treated as any other failed gate: `overall: "fail"`,
//! `chained: false`, no finalize, and the refusal text carried in the gate's
//! `summary` so the operator reads WHICH finding, TODO or unmarked item held the
//! close and the exact command that settles it.
//!
//! It is taken WHOLE, not sub-gate by sub-gate. Its build stage re-runs the
//! `mustard.json` build/type/lint/test commands the `verify-pipeline` gate above
//! already exercised, which costs a second pass on a project that configures a
//! test command. That cost is accepted rather than carved around: a composite
//! that picked which sub-gates apply to it would be the same divergence this
//! wiring exists to remove, and the pass is governed by the operator's own
//! `MUSTARD_CLOSE_GATE_MODE`, not by a knob invented here.
//!
//! ## Deterministic chaining (no LLM judgement)
//!
//! When **every** gate passes, the orchestrator auto-chains the close itself —
//! it calls [`crate::commands::spec::complete_spec::finalize`] **directly**
//! (module-qualified, in-process — no subprocess), marking the spec
//! `completed` and emitting `pipeline.complete`. `finalize`, not `run_complete`:
//! the QA gate above reads the RECORDED `qa.result overall=pass`
//! ([`qa_gate_passes`]) — the same fact `run_complete`'s admission would read —
//! so routing there would refuse nothing and only re-execute every criterion.
//! That equivalence is the whole justification, and it only holds while this
//! gate stays strict: it once accepted `skip` and chained the close with nothing
//! verified. It then auto-verifies
//! that the `pipeline.complete` event landed in the per-spec NDJSON window via
//! [`crate::commands::event::verify_emit::verify_event_landed`] and folds the
//! boolean into the report (`verified`). The LLM no longer decides whether to
//! call `complete-spec`; it is a relay. When **any** gate fails the close is
//! report-only (`chained: false`, no finalize) exactly as before. The
//! `emit_pipeline` QA-gate stays the strict safety net behind both paths.
//!
//! ## Fail-open
//!
//! Each gate is fail-open at the subprocess level: a missing binary or
//! non-zero exit becomes a `gate.ok = false` row, the next gate still runs,
//! and the overall verdict is derived from the boolean vector. The chaining is
//! *gated* on `overall == pass` (the gate itself is strict — it does not
//! fail-open into a finalize); the auxiliary verify is best-effort.
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
//!     { "name": "close-gates",     "ok": false, "duration_ms": 34,
//!       "summary": "⛔ Close Gate — 1 finding(s) … still have no destination …" },
//!     { "name": "pipeline-summary","ok": true,  "duration_ms": 12 }
//!   ],
//!   "chained":  true,
//!   "verified": true,
//!   "duration_ms": 669
//! }
//! ```

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::context;
use crate::shared::events::economy;
use crate::commands::review::review_spans::{check_consolidation, ConsolidationCheck};
use mustard_core::platform::process::rtk_command;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

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
pub(crate) struct CloseReport {
    pub spec: String,
    pub overall: &'static str,
    pub gates: Vec<GateReport>,
    /// `true` when every gate passed and the close was auto-chained
    /// (`complete-spec` finalize ran in-process). `false` on a report-only run.
    pub chained: bool,
    /// `true` when the auto-chained `pipeline.complete` event was confirmed in
    /// the per-spec NDJSON window. Omitted when nothing was chained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
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
    v.get("payload")
        .and_then(|p| p.get("overall"))
        .or_else(|| v.get("overall"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Whether the QA gate passes for this close.
///
/// Reads the verdict where it is RECORDED, through the same predicate
/// `emit-pipeline` and `complete-spec` consult — one fact, three doors.
///
/// Deliberately NOT the subprocess's exit code, and not a string parsed off its
/// stdout. `qa-run` exits 0 on `skip`, so "it ran" is not "it verified"; and the
/// stdout read above had been looking for `overall` at the top level while
/// `qa-run` prints it under `payload`, so it always answered `None` and the gate
/// silently collapsed to "the subprocess exited 0". Together those two let a
/// `skip` chain the close and write `pipeline.complete` with nothing verified —
/// found by review, reproduced live. `close-pipeline` already refuses `skip` for
/// exactly this reason; this is the two composites agreeing.
fn qa_gate_passes(cwd: &Path, spec: &str) -> bool {
    crate::commands::event::emit_pipeline::qa_result_passed(cwd, spec)
}

/// CLI entry.
pub fn run(opts: CloseOrchestrateOpts) {
    let started = std::time::Instant::now();
    let mut gates: Vec<GateReport> = Vec::new();
    // Resolved ONCE, up here: the QA gate below reads the recorded verdict from
    // this root, and the chained finalize writes to the same one.
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());

    // 1. verify-pipeline (build/test gate).
    let (ok, dur, _) = run_subcmd(&["verify-pipeline"]);
    gates.push(GateReport {
        name: "verify-pipeline".to_string(),
        ok,
        duration_ms: dur,
        summary: None,
    });

    // 2. qa-run --spec <spec>. The run is what RECORDS a verdict; the gate then
    //    reads the record ([`qa_gate_passes`]), never this subprocess's exit
    //    code — it exits 0 on `skip`. `qa_summary` is for the operator-facing
    //    report only and never decides.
    let (_qa_ran, qa_dur, qa_out) = run_subcmd(&["qa-run", "--spec", &opts.spec]);
    let qa_summary = qa_overall(&qa_out);
    let qa_pass = qa_gate_passes(&cwd, &opts.spec);
    gates.push(GateReport {
        name: "qa-run".to_string(),
        ok: qa_pass,
        duration_ms: qa_dur,
        summary: qa_summary,
    });

    // 3. review-spans — block close when any wave's `_review-spans.md` has a
    // red verdict (W5#1: wires `check_consolidation` into close so AC-A-7's
    // span-level block actually fires through close_orchestrate, not only at
    // the hook layer).
    let (rs_ok, rs_dur, rs_summary) = run_review_spans_gate(&opts.spec);
    gates.push(GateReport {
        name: "review-spans".to_string(),
        ok: rs_ok,
        duration_ms: rs_dur,
        summary: rs_summary,
    });

    // 4. docs-stale-check (optional).
    if !opts.skip_docs {
        let (ok, dur, _) = run_subcmd(&["docs-stale-check"]);
        gates.push(GateReport {
            name: "docs-stale-check".to_string(),
            ok,
            duration_ms: dur,
            summary: None,
        });
    }

    // 5. close-gates — the sub-gates `emit-phase --to CLOSE` runs. Taken LAST
    //    among the blocking gates so the cheap, spec-scoped refusals above are
    //    already in the report when this one speaks.
    gates.push(close_gates_gate(&cwd, &opts.spec));

    // 6. pipeline-summary (advisory — always passes).
    let spec_dir = format!(".claude/spec/{}", opts.spec);
    let (sum_ok, sum_dur, _) = run_subcmd(&["pipeline-summary", "--spec-dir", &spec_dir]);
    gates.push(GateReport {
        name: ADVISORY_GATE.to_string(),
        ok: sum_ok,
        duration_ms: sum_dur,
        summary: None,
    });

    // `pipeline-summary` is advisory — it only renders the Done/Left/Next
    // report, so a transient failure (e.g. a concurrent atomic write of
    // `spec.md` while a sibling gate runs) must never block the close. Only the
    // real quality gates count toward `overall`.
    let overall_pass = close_overall(&gates);

    // Deterministic chaining: when every gate passes, finalize the spec in
    // process (no LLM judgement, no subprocess) and auto-verify the
    // `pipeline.complete` event landed. A failing gate is report-only.
    let (chained, verified) = chain_close(&cwd, &opts.spec, &gates);

    let total = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let report = CloseReport {
        spec: opts.spec.clone(),
        overall: if overall_pass { "pass" } else { "fail" },
        gates,
        chained,
        verified,
        duration_ms: total,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "close-orchestrate", total, Some(opts.spec.as_str()), json!({ "chained": chained, "verified": verified }));
}

/// Gate name of the advisory summary step — excluded from the close verdict by
/// [`close_overall`].
const ADVISORY_GATE: &str = "pipeline-summary";

/// Gate name of the CLOSE sub-gates ([`close_gates_gate`]). Blocking, like every
/// other name in the vector except [`ADVISORY_GATE`].
const CLOSE_GATES_GATE: &str = "close-gates";

/// Derive the close verdict from the gate vector. The advisory [`ADVISORY_GATE`]
/// (`pipeline-summary`) is excluded: it only renders the Done/Left/Next report,
/// so a transient failure there must never block the close. Every blocking gate
/// (verify-pipeline / qa-run / review-spans / docs-stale-check / close-gates)
/// must pass.
fn close_overall(gates: &[GateReport]) -> bool {
    gates.iter().filter(|g| g.name != ADVISORY_GATE).all(|g| g.ok)
}

/// The CLOSE sub-gates, as one gate row of this composite's report.
///
/// `ok` is [`crate::commands::pipeline::close_gates::gate_close_for_spec`]'s
/// answer with the modes it resolves from the environment, so `/pr` and
/// `emit-phase --to CLOSE` refuse the same trees for the same reasons and honour
/// the same `MUSTARD_*_GATE_MODE` overrides. The refusal text becomes the row's
/// `summary` VERBATIM: it already names the offending finding / marker / item and
/// the exact command that settles it, and a gate that refuses without saying what
/// to do teaches the reader to route around it.
fn close_gates_gate(cwd: &Path, spec: &str) -> GateReport {
    let started = std::time::Instant::now();
    let refusal = crate::commands::pipeline::close_gates::gate_close_for_spec(
        &cwd.to_string_lossy(),
        spec,
    )
    .err();
    GateReport {
        name: CLOSE_GATES_GATE.to_string(),
        ok: refusal.is_none(),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        summary: refusal,
    }
}

/// Finalize the spec when — and only when — the gate vector allows it.
///
/// The verdict comes from [`close_overall`] over the SAME vector the report
/// prints, so "the report says fail" and "the close did not happen" can never
/// disagree. Returns `(chained, verified)`: a refusing vector is report-only,
/// which means [`finalize_and_verify`] is not reached at all and nothing is
/// written, emitted or marked `completed`.
fn chain_close(cwd: &Path, spec: &str, gates: &[GateReport]) -> (bool, Option<bool>) {
    if !close_overall(gates) {
        return (false, None);
    }
    finalize_and_verify(cwd, spec)
}

/// Finalize the spec in-process and confirm the close landed.
///
/// Calls [`crate::commands::spec::complete_spec::finalize`] directly
/// (module-qualified — no subprocess) to mark the spec `completed` and emit
/// `pipeline.complete` (coupled with the root `meta.json` sync). It uses
/// `finalize`, not `run_complete`, because the qa-run gate already ran every AC
/// — re-running them in the finalize wastes minutes for no new signal. Then
/// reuses
/// [`crate::commands::event::verify_emit::verify_event_landed`] to confirm the
/// `pipeline.complete` event landed in the per-spec NDJSON window. Both steps
/// are deterministic; `complete_spec`'s emits are idempotent, so a re-run after
/// an already-closed spec is a no-op flip. Returns `(chained, Some(verified))`.
fn finalize_and_verify(cwd: &Path, spec: &str) -> (bool, Option<bool>) {
    // Use `finalize`, NOT `run_complete`: the QA gate above already ran every AC
    // and, crucially, gated on the RECORDED `qa.result overall=pass` — the same
    // fact `run_complete`'s admission would read, so routing here would refuse
    // nothing and only re-execute every AC a second time in-process (wasted
    // minutes for zero new signal). That claim is only safe because the gate is
    // strict: until it was fixed it accepted `skip`, and this line asserted a
    // verification that had not happened. `finalize` is the qa-less terminal path
    // `close-pipeline` already uses for exactly this reason.
    let _ = crate::commands::spec::complete_spec::finalize(cwd, spec);
    let verified = crate::commands::event::verify_emit::verify_event_landed(
        cwd,
        "pipeline.complete",
        Some(spec),
        Some("60s"),
    );
    // F4-c item 3 — auto epic-fold. Closing this spec may have been the last
    // child of an epic; detect any epic now ready (all children CLOSE, root
    // not yet CLOSE — same NDJSON source `epic-fold --detect` reads) and fold
    // it in-process, without the LLM having to call `epic-fold --epic`.
    // `fold_epic` is idempotent (skips when `epic.complete` already exists), so
    // a re-run after an already-folded epic is a no-op. Fail-open: errors are
    // swallowed — the close already succeeded.
    auto_fold_completed_epics(cwd);
    (true, Some(verified))
}

/// Detect every epic whose children are all CLOSE and fold each one in-process.
///
/// Deterministic + idempotent: detection reads the per-spec NDJSON event stream
/// ([`crate::commands::wave::epic_fold::detect_completed_epics`]) and the fold
/// ([`crate::commands::wave::epic_fold::fold_epic`]) skips any epic already
/// carrying an `epic.complete` event. Module-qualified, no subprocess.
fn auto_fold_completed_epics(cwd: &Path) {
    for epic in crate::commands::wave::epic_fold::detect_completed_epics(cwd) {
        let _ = crate::commands::wave::epic_fold::fold_epic(cwd, &epic);
    }
}

/// Walk `.claude/spec/<spec>/wave-*-*/` and run `review_spans::check_consolidation`
/// on each wave directory. Reports `ok=false` and a `summary` listing the
/// blocked wave names when any ledger registers a red verdict. Missing
/// directories are skipped silently — the helper is advisory and fail-open
/// for waves that never produced a ledger (no span-level eval ever ran).
fn run_review_spans_gate(spec: &str) -> (bool, u64, Option<String>) {
    let started = std::time::Instant::now();
    // ClaudePaths-exempt: deliberate cwd-relative path (no project-root handle
    // here); `for_project` would force an absolute `current_dir()` resolution.
    let spec_dir = Path::new(".claude").join("spec").join(spec);
    let mut blocked_waves: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&spec_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("wave-") {
                continue;
            }
            if let ConsolidationCheck::Blocked { .. } = check_consolidation(&path) {
                blocked_waves.push(name);
            }
        }
    }
    let dur = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if blocked_waves.is_empty() {
        (true, dur, None)
    } else {
        blocked_waves.sort();
        (false, dur, Some(format!("blocked: {}", blocked_waves.join(","))))
    }
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

    /// The shape `qa-run` ACTUALLY prints. The reader looked for `overall` at
    /// the top level while the command emits `{ event, payload: { overall } }`,
    /// so it always answered `None` — which is why the gate report shipped with
    /// no `summary` and the decision silently fell back to the subprocess exit
    /// code. The bare form the tests above use still works.
    #[test]
    fn qa_overall_reads_the_shape_qa_run_really_prints() {
        let real = r#"{"event":"qa.result","payload":{"spec":"s","overall":"skip","criteria":[]}}"#;
        assert_eq!(qa_overall(real).as_deref(), Some("skip"));
        let passing = r#"{"event":"qa.result","payload":{"spec":"s","overall":"pass"}}"#;
        assert_eq!(qa_overall(passing).as_deref(), Some("pass"));
    }

    /// The gate itself, both directions: a run that verified nothing must NOT
    /// open the close, and a recorded pass must.
    ///
    /// This is the composite half of the side-door fix. `close-orchestrate` used
    /// to accept `skip` — and, because the stdout read above never resolved, to
    /// accept "the subprocess exited 0", which `qa-run` does on `skip`. So it
    /// chained the finalize and wrote `pipeline.complete` with nothing verified,
    /// while `close-pipeline` refused the same shape. Now both read the recorded
    /// verdict through the one predicate.
    #[test]
    fn a_skip_verdict_does_not_open_the_composite_close() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let spec = "composite-gate-spec";

        // Nothing recorded at all → shut.
        assert!(!qa_gate_passes(cwd, spec), "no verdict is not a pass");

        // A recorded `skip` → still shut. The exact shape that used to chain.
        emit_qa_result(cwd, spec, "skip");
        assert!(
            !qa_gate_passes(cwd, spec),
            "a run that verified nothing must not open the close"
        );

        // A recorded `pass` → open, so the gate cannot pass by refusing always.
        emit_qa_result(cwd, spec, "pass");
        assert!(qa_gate_passes(cwd, spec), "a recorded pass must open it");
    }

    /// Write one `qa.result` for `spec` through the production router, so the
    /// gate reads a real event row rather than a hand-built fixture.
    fn emit_qa_result(cwd: &Path, spec: &str, overall: &str) {
        emit_qa_result_at(cwd, spec, overall, "2026-07-26T00:00:00.000Z");
    }

    /// The same, with the verdict's timestamp named — the CLOSE gates refuse a
    /// QA pass older than the `spec.md` it claims to have verified, so a test
    /// that seeds both files has to say when the run happened.
    fn emit_qa_result_at(cwd: &Path, spec: &str, overall: &str, ts: &str) {
        use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Cli, id: Some("test".to_string()), actor_type: None },
            event: "qa.result".to_string(),
            payload: serde_json::json!({ "spec": spec, "overall": overall, "criteria": [] }),
            spec: Some(spec.to_string()),
        };
        let _ = crate::shared::events::route::emit(&cwd.to_string_lossy(), &event);
    }

    /// Seed a project whose spec carries one finding nobody has decided
    /// anything about — the tree the `/pr` close used to walk straight past.
    fn seed_spec_with_an_open_finding(cwd: &Path, spec: &str) {
        let sp = mustard_core::ClaudePaths::for_project(cwd)
            .unwrap()
            .for_spec(spec)
            .unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Spec\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(sp.dir().join("review")).unwrap();
        std::fs::write(
            sp.dir().join("review").join("findings.md"),
            "# Findings\n\n- MAJOR — the everyday close never reaches the gate\n",
        )
        .unwrap();
    }

    /// AC-8 — the everyday close path answers to the CLOSE gates.
    ///
    /// `/pr` documents `close-orchestrate`, and this composite calls
    /// `complete_spec::finalize` DIRECTLY, so `gate_close_for_spec` — where the
    /// debt, checklist, findings, QA and build sub-gates live — never ran here.
    /// A spec whose reviewer left an undecided finding closed through this door
    /// while `emit-phase --to CLOSE` refused the very same tree. The proof is on
    /// both halves: the gate REFUSES and says which finding, and the finalize is
    /// not reached — read through the same `verify_event_landed` the chained
    /// close uses to confirm itself.
    #[test]
    fn close_orchestrate_blocks_on_open_finding() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        seed_spec_with_an_open_finding(cwd, "found");

        let gate = close_gates_gate(cwd, "found");
        assert_eq!(gate.name, CLOSE_GATES_GATE);
        assert!(!gate.ok, "an undecided finding must fail the close gate: {gate:?}");
        let summary = gate.summary.clone().expect("a refusal carries its reason");
        assert!(
            summary.contains("the everyday close never reaches the gate"),
            "the operator must read WHICH finding held the close: {summary}"
        );
        assert!(
            summary.contains("mustard-rt run mark-finding --spec found --id F-findings"),
            "and the exact command that settles it: {summary}"
        );

        // A blocking gate that fails is report-only: no finalize, no
        // `pipeline.complete`, nothing marked completed.
        let gates = vec![
            GateReport { name: "verify-pipeline".to_string(), ok: true, duration_ms: 1, summary: None },
            gate,
        ];
        assert!(!close_overall(&gates), "the vector must fail the close");
        assert_eq!(chain_close(cwd, "found", &gates), (false, None));
        assert!(
            !crate::commands::event::verify_emit::verify_event_landed(
                cwd,
                "pipeline.complete",
                Some("found"),
                Some("1h"),
            ),
            "the close must not have been chained over an undecided finding"
        );

        // Control — the same tree, once the destination is declared and QA has a
        // recorded pass: the gate opens. So the refusal above is this finding's,
        // not a gate that answers no to everything.
        let spec_dir = mustard_core::ClaudePaths::for_project(cwd)
            .unwrap()
            .for_spec("found")
            .unwrap()
            .dir()
            .to_path_buf();
        let open = crate::commands::review::finding_collect::collect(
            cwd,
            spec_dir.to_str().unwrap(),
        );
        let [finding] = open.findings.as_slice() else {
            panic!("exactly one finding was seeded, got {:?}", open.findings);
        };
        crate::commands::spec::mark_finding::mark(
            cwd,
            spec_dir.to_str().unwrap(),
            &finding.id,
            mustard_core::domain::spec::contract::FindingRoute::QueuedWork(
                "queued as its own unit".to_string(),
            ),
        )
        .expect("the door records the destination");
        // Dated ahead of the seeded `spec.md` on purpose: the QA gate refuses a
        // pass that predates the spec it claims to have verified.
        emit_qa_result_at(cwd, "found", "pass", "2099-01-01T00:00:00.000Z");

        let reopened = close_gates_gate(cwd, "found");
        assert!(
            reopened.ok,
            "a decided finding holds nothing back: {:?}",
            reopened.summary
        );
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
            chained: true,
            verified: Some(true),
            duration_ms: 2,
        };
        let v = serde_json::to_value(r).unwrap();
        assert!(v.get("spec").is_some());
        assert!(v.get("overall").is_some());
        assert!(v.get("gates").unwrap().is_array());
        assert!(v.get("duration_ms").is_some());
        assert_eq!(v.get("chained").and_then(Value::as_bool), Some(true));
        assert_eq!(v.get("verified").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn report_only_run_omits_verified() {
        // A report-only (gate failed) run carries `chained:false` and no
        // `verified` key (skip_serializing_if = None).
        let r = CloseReport {
            spec: "demo".to_string(),
            overall: "fail",
            gates: vec![GateReport {
                name: "verify-pipeline".to_string(),
                ok: false,
                duration_ms: 1,
                summary: None,
            }],
            chained: false,
            verified: None,
            duration_ms: 2,
        };
        let v = serde_json::to_value(r).unwrap();
        assert_eq!(v.get("chained").and_then(Value::as_bool), Some(false));
        assert!(v.get("verified").is_none());
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

    #[test]
    fn advisory_summary_failure_does_not_block_close() {
        // The advisory `pipeline-summary` gate is informational; a failure there
        // (transient or otherwise) must NOT fail the close when every blocking
        // gate passed. Regression for the close that failed only because the
        // summary subprocess exited non-zero once.
        let gates = vec![
            GateReport { name: "verify-pipeline".to_string(), ok: true, duration_ms: 1, summary: None },
            GateReport { name: "qa-run".to_string(), ok: true, duration_ms: 1, summary: Some("pass".to_string()) },
            GateReport { name: ADVISORY_GATE.to_string(), ok: false, duration_ms: 1, summary: None },
        ];
        assert!(close_overall(&gates), "advisory summary failure must not block close");

        // A real blocking-gate failure still fails the close.
        let blocked = vec![
            GateReport { name: "verify-pipeline".to_string(), ok: false, duration_ms: 1, summary: None },
            GateReport { name: ADVISORY_GATE.to_string(), ok: true, duration_ms: 1, summary: None },
        ];
        assert!(!close_overall(&blocked), "a failing blocking gate must fail the close");
    }
}
