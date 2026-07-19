//! `mustard-rt run plan-materialize` — composite PLAN-phase materialisation.
//!
//! Composes, **in-process** (module-qualified, no subprocess), the steps the
//! orchestrator used to relay one by one after the Plan agent produced the
//! plan JSON:
//!
//! 1. `wave-scaffold` — [`crate::commands::wave::wave_scaffold::scaffold`]
//!    materialises `wave-plan.md` + every `wave-N-{role}/spec.md` + sidecars.
//! 2. `analyze-validation` — [`crate::commands::review::analyze_validation::validate`]
//!    (WARN-level, includes the wave-2 AC-parseability check) over the root
//!    `spec.md`.
//! 3. `emit-pipeline --kind pipeline.scope` — the typed
//!    [`PipelineScopePayload`] with `scope: "full"` (this composite exists for
//!    the Full/wave-plan flow) + the scaffolded wave count.
//! 4. `emit-phase --to PLAN` — [`crate::commands::event::emit_phase::run_at`]
//!    (idempotent on the spec's last phase).
//!
//! Pressupposes `spec.md` + `meta.json` already materialised by `spec-draft`
//! (which stays a separate command: the Plan agent folds the narrative body
//! BETWEEN draft and scaffold). A missing `spec.md` degrades the validation to
//! an ERROR issue; it never blocks the scaffold.
//!
//! ## Output (single JSON document, byte-stable, ordered)
//!
//! ```json
//! {
//!   "events": ["pipeline.scope", "pipeline.phase"],
//!   "scaffold": { "created_files": [...], "skipped": [...] },
//!   "validation": { "ok": true, "issues": [] }
//! }
//! ```
//!
//! `events` lists the composed emission steps that ran (empty when the
//! scaffold failed — no phase transition is recorded for a plan that did not
//! materialise). Keys serialize sorted (serde_json default map); no
//! timestamps or volatile paths appear on stdout.

use crate::commands::event::emit_phase;
use crate::commands::review::analyze_validation;
use crate::commands::wave::wave_scaffold::{self, ScaffoldOutcome};
use crate::shared::context::session_id;
use mustard_core::domain::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineScopePayload, SCHEMA_VERSION, EVENT_PIPELINE_SCOPE,
};
use mustard_core::io::fs;
use mustard_core::time::now_iso8601;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run plan-materialize`.
#[derive(Debug, Clone)]
pub struct PlanMaterializeOpts {
    /// Target spec directory (the `.claude/spec/{slug}/` the draft created).
    pub spec_dir: String,
    /// Path to the plan JSON file the Plan agent authored.
    pub plan: String,
}

/// Stdout `scaffold.error` marker for a plan file that could not be read or
/// parsed. [`run`] maps this failure to exit 2 — the single source for the
/// string keeps the JSON field and the exit mapping in lockstep.
const ERR_PLAN_UNREADABLE: &str = "plan unreadable";

/// CLI entry — resolves the paths against the cwd and prints the composite
/// report. Exit code: 0 on success and on advisory failures (validation is
/// WARN-level; failures are expressed in the JSON), 2 when the plan file
/// could not be read/parsed (`scaffold.error: "plan unreadable"`) — aligned
/// with the standalone `wave-scaffold` contract (operator error → non-zero
/// exit so the orchestrator notices).
pub fn run(opts: PlanMaterializeOpts) {
    let project = PathBuf::from(crate::shared::context::project_dir());
    let spec_dir = absolutize(&project, &opts.spec_dir);
    let plan_path = absolutize(&project, &opts.plan);
    let report = materialize(&project, &spec_dir, &plan_path);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
    );
    if report["scaffold"]["error"].as_str() == Some(ERR_PLAN_UNREADABLE) {
        std::process::exit(2);
    }
}

/// Join a possibly-relative CLI path onto the project root.
fn absolutize(project: &Path, raw: &str) -> PathBuf {
    if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        project.join(raw)
    }
}

/// The composite miolo: scaffold + validate + emit, against an explicit
/// `project` root (testable without mutating the process cwd). Returns the
/// report Value [`run`] prints.
pub(crate) fn materialize(project: &Path, spec_dir: &Path, plan_path: &Path) -> Value {
    let spec = spec_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // 1. wave-scaffold (in-process miolo — same renderer the standalone
    //    subcommand uses; idempotent, skip-if-present).
    let outcome = wave_scaffold::scaffold(spec_dir, plan_path);
    let (scaffold_json, scaffold_ok) = match outcome {
        // `trace_block` (strict MUSTARD_TRACE_GATE_MODE) is deliberately ignored
        // here: the trace gate is on the standalone `wave-scaffold` command, not
        // the pipeline composite — plan-materialize reports the scaffold and
        // emits the PLAN transition regardless of an uncovered-criterion gap.
        ScaffoldOutcome::Created { created, skipped, trace_block: _ } => (
            json!({ "created_files": created, "skipped": skipped }),
            true,
        ),
        ScaffoldOutcome::EmptyPlan => (
            json!({
                "created_files": [],
                "skipped": [],
                "error": "plan.waves is empty",
            }),
            false,
        ),
        ScaffoldOutcome::Unreadable(msg) => {
            eprintln!("{msg}");
            (
                json!({
                    "created_files": [],
                    "skipped": [],
                    "error": ERR_PLAN_UNREADABLE,
                }),
                false,
            )
        }
    };

    // 2. analyze-validation over the root spec.md (the spec-draft output).
    //    WARN-level by contract — never blocks the scaffold or the events.
    let validation = validate_root_spec(spec_dir);

    // 3 + 4. Events — only for a plan that actually materialised (no PLAN
    //    transition for a spec whose scaffold failed) and a resolvable slug.
    let mut events: Vec<String> = Vec::new();
    if scaffold_ok && !spec.is_empty() {
        emit_scope_full(project, spec_dir, &spec);
        events.push(EVENT_PIPELINE_SCOPE.to_string());
        // Idempotent: a re-run whose last phase is already PLAN skips the
        // write inside `run_at`. PLAN never trips the CLOSE gate, so the
        // Err arm is unreachable in practice — degrade by omission.
        if emit_phase::run_at(project, &spec, "PLAN", None).is_ok() {
            events.push("pipeline.phase".to_string());
        }
    }

    json!({
        "events": events,
        "scaffold": scaffold_json,
        "validation": validation,
    })
}

/// Run the WARN-level structural validation over `<spec_dir>/spec.md`,
/// reusing the exact `analyze-validation` checks (layer coverage, file refs,
/// task counts, AC parseability). A missing/unreadable `spec.md` degrades to
/// `ok: false` with a single ERROR issue — `plan-materialize` pressupposes the
/// draft already ran, so the gap is surfaced, not silently skipped.
fn validate_root_spec(spec_dir: &Path) -> Value {
    let spec_md = spec_dir.join("spec.md");
    if !fs::exists(&spec_md) {
        return json!({
            "ok": false,
            "issues": [{
                "severity": "ERROR",
                "type": "missing-spec",
                "message": "spec.md not found — run spec-draft before plan-materialize",
            }],
        });
    }
    match fs::read_to_string(&spec_md) {
        Ok(content) => {
            let issues = analyze_validation::validate(&spec_md, &content);
            json!({ "ok": issues.is_empty(), "issues": issues })
        }
        Err(e) => json!({
            "ok": false,
            "issues": [{
                "severity": "ERROR",
                "type": "unreadable-spec",
                "message": format!("cannot read spec.md: {e}"),
            }],
        }),
    }
}

/// Emit the typed `pipeline.scope` event (`scope: "full"`, `isWavePlan`,
/// `totalWaves` from the freshly-reconciled root `meta.json`) through the
/// canonical event router against the explicit `project` root.
///
/// The event is built with the same shape `emit-pipeline --kind pipeline.scope`
/// produces (a `pipeline.scope` carries no alias fan-out and no meta sync, so
/// routing it directly is behaviour-identical — the precedent is
/// `complete_spec::emit_ndjson`, which also writes its events without a
/// subprocess round-trip).
fn emit_scope_full(project: &Path, spec_dir: &Path, spec: &str) {
    let total_waves = mustard_core::read_meta(&spec_dir.join("meta.json"))
        .and_then(|m| m.total_waves);
    let payload = serde_json::to_value(PipelineScopePayload {
        scope: "full".to_string(),
        lang: None,
        model: None,
        is_wave_plan: Some(true),
        total_waves,
    })
    .unwrap_or(Value::Null);

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("plan-materialize".to_string()),
            actor_type: None,
        },
        event: EVENT_PIPELINE_SCOPE.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(&project.to_string_lossy(), &event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// Seed a project root + a drafted spec dir (spec.md present, as
    /// `spec-draft` leaves it) and a 2-wave plan JSON. Returns
    /// `(project, spec_dir, plan_path)`.
    fn seed(project: &Path, slug: &str) -> (PathBuf, PathBuf) {
        let spec_dir = project.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Demo\n\n## Files\n- `a.rs` (create)\n\n### Backend Agent\n- [ ] t1\n- [ ] t2\n",
        )
        .unwrap();
        let plan_path = project.join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "rt", "summary": "base", "depends_on": [],
                      "tasks": ["do the thing"] },
                    { "n": 2, "role": "cli", "summary": "wire", "depends_on": ["wave-1-rt"],
                      "tasks": ["wire it"] }
                ],
                "total_waves": 2,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();
        (spec_dir, plan_path)
    }

    /// Happy path: scaffold materialises the layout, validation passes, and
    /// both events (`pipeline.scope` then `pipeline.phase` PLAN) land in the
    /// spec's `.events/` log.
    #[test]
    fn composite_plan_materialize_scaffolds_validates_and_emits() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let (spec_dir, plan_path) = seed(project, "demo-pm");

        let report = materialize(project, &spec_dir, &plan_path);

        // Scaffold: wave-plan + 2 wave specs created.
        let created = report["scaffold"]["created_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(created.contains(&"wave-plan.md".to_string()), "{report}");
        assert!(created.contains(&"wave-1-rt/spec.md".to_string()), "{report}");
        assert!(created.contains(&"wave-2-cli/spec.md".to_string()), "{report}");
        assert!(spec_dir.join("wave-plan.md").exists());
        assert!(spec_dir.join("wave-1-rt").join("spec.md").exists());

        // Validation: the seeded spec is clean.
        assert_eq!(report["validation"]["ok"], json!(true), "{report}");

        // Events: scope (full) + phase (PLAN), in emission order.
        assert_eq!(
            report["events"],
            json!(["pipeline.scope", "pipeline.phase"]),
            "{report}"
        );
        let events_dir = spec_dir.join(".events");
        let events =
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        let scope = events
            .iter()
            .find(|e| e.event == "pipeline.scope")
            .expect("pipeline.scope landed");
        assert_eq!(scope.payload["scope"], json!("full"), "{:?}", scope.payload);
        assert_eq!(scope.payload["total_waves"], json!(2), "{:?}", scope.payload);
        let phase = events
            .iter()
            .find(|e| e.event == "pipeline.phase")
            .expect("pipeline.phase landed");
        assert_eq!(phase.payload["to"], json!("PLAN"), "{:?}", phase.payload);

        // Idempotent re-run: nothing re-created, phase emit skipped inside
        // run_at (last phase already PLAN), report stays coherent.
        let again = materialize(project, &spec_dir, &plan_path);
        assert!(again["scaffold"]["created_files"].as_array().unwrap().is_empty());
        let phases = mustard_core::view::projection::read_harness_events_from_ndjson_dir(
            &events_dir,
        )
        .into_iter()
        .filter(|e| e.event == "pipeline.phase")
        .count();
        assert_eq!(phases, 1, "PLAN phase is idempotent — no duplicate emit");
    }

    /// Degraded: a nonexistent plan file (spec never drafted) yields the
    /// error-tagged scaffold, a missing-spec validation ERROR, and NO events.
    #[test]
    fn composite_plan_materialize_missing_spec_degrades_without_events() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("ghost");
        let plan_path = project.join("nope.json");

        let report = materialize(project, &spec_dir, &plan_path);

        assert_eq!(report["scaffold"]["error"], json!("plan unreadable"), "{report}");
        assert!(report["scaffold"]["created_files"].as_array().unwrap().is_empty());
        assert_eq!(report["validation"]["ok"], json!(false), "{report}");
        assert_eq!(
            report["validation"]["issues"][0]["type"],
            json!("missing-spec"),
            "{report}"
        );
        assert_eq!(report["events"], json!([]), "no events for a failed scaffold");
        // No phantom .events dir for the ghost spec.
        let events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(
            &spec_dir.join(".events"),
        );
        assert!(events.is_empty());
    }

    /// Degraded: an empty plan (operator error) reports the W10.T10.3 gate
    /// message in the scaffold slot and emits nothing.
    #[test]
    fn composite_plan_materialize_empty_plan_reports_gate_error() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let (spec_dir, _) = seed(project, "demo-empty");
        let plan_path = project.join("empty-plan.json");
        std::fs::write(&plan_path, r#"{"waves":[]}"#).unwrap();

        let report = materialize(project, &spec_dir, &plan_path);
        assert_eq!(
            report["scaffold"]["error"],
            json!("plan.waves is empty"),
            "{report}"
        );
        assert_eq!(report["events"], json!([]), "{report}");
        // The drafted spec.md is still validated (advisory step is independent).
        assert_eq!(report["validation"]["ok"], json!(true), "{report}");
    }
}
