//! `mustard-rt run plan-materialize` — composite PLAN-phase materialisation.
//!
//! Composes, **in-process** (module-qualified, no subprocess), the steps the
//! orchestrator used to relay one by one after the Plan agent produced the
//! plan JSON:
//!
//! 1. the wave-scaffold renderer —
//!    [`crate::commands::wave::wave_scaffold::scaffold`] materialises
//!    `wave-plan.md` + every `wave-N-{role}/spec.md` + sidecars. It is NOT a
//!    published subcommand; this composite is its only entry point.
//! 2. `analyze-validation` — [`crate::commands::review::analyze_validation::validate`]
//!    (WARN-level, includes the wave-2 AC-parseability check) over the root
//!    `spec.md`.
//! 3. `wave-dependency`'s import-DAG cycle check —
//!    [`crate::commands::wave::wave_dependency::validate_plan_dag`] over the
//!    plan's file union (WARN-level). Folded in so the check runs every time,
//!    not only when the orchestrator relays a separate `wave-dependency` call.
//! 4. `wave-dependency`'s SAME-LEVEL FILE COLLISION check —
//!    [`crate::commands::wave::wave_dependency::plan_file_collisions`] over the
//!    plan's declared per-wave censuses. BLOCKING, unlike 3: two waves that share
//!    a dispatch level have no edge between them and go out together, so a file
//!    both declare puts two agents in it with nothing ordering them. The refusal
//!    names the minimal chaining that zeroes each overlap. Like the coverage gate
//!    below, it has NO env knob — the condition carries no false positive by
//!    construction (same level IS parallel dispatch), and the advisory reading of
//!    the same fact (`wave-overlap-check`, at the approval gate) was measured
//!    insufficient in the field.
//! 5. `emit-pipeline --kind pipeline.scope` — the typed
//!    [`PipelineScopePayload`] with `scope: "full"` (this composite exists for
//!    the Full/wave-plan flow) + the scaffolded wave count.
//! 7. `emit-phase --to PLAN` — [`crate::commands::event::emit_phase::run_at`]
//!    (idempotent on the spec's last phase).
//!
//! Pressupposes `spec.md` + `meta.json` already materialised by `spec-draft`.
//! A missing `spec.md` degrades the validation to an ERROR issue; it never
//! blocks the scaffold.
//!
//! ## Two doors, one composite
//!
//! The steps above are the whole of the PLAN-phase materialisation, so both
//! doors that materialise a plan call THIS module rather than growing a second
//! copy of the sequence:
//!
//! - `spec-draft --plan` is the FIRST materialisation — it drafts `spec.md` +
//!   `meta.json` and runs the composite in the same invocation
//!   ([`materialize_fresh`], which additionally undoes the layout when the
//!   composite refuses).
//! - `mustard-rt run plan-materialize` (this command) is the RE-materialisation
//!   door: it reconciles an existing layout onto an edited plan before approval,
//!   which is why a refusal here deliberately leaves the layout in place for the
//!   next pass to repair.
//!
//! ## Output (single JSON document, byte-stable, ordered)
//!
//! ```json
//! {
//!   "events": ["pipeline.scope", "pipeline.phase"],
//!   "scaffold": {
//!     "created_files": [], "skipped": [], "refreshed": [], "removed": [],
//!     "untraced_waves": []
//!   },
//!   "validation": { "ok": true, "issues": [] },
//!   "dependencies": { "ok": true, "issues": [] },
//!   "sharedFiles": { "ok": true, "issues": [] }
//! }
//! ```
//!
//! `events` lists the composed emission steps that ran (empty when the
//! scaffold failed — no phase transition is recorded for a plan that did not
//! materialise). The four scaffold lists are ALWAYS present (empty when nothing
//! changed) and `refreshed` / `removed` are sorted, so re-running an unchanged
//! plan prints the same bytes. `untraced_waves` is advisory — a wave with tasks
//! that traces to no existing criterion, or a `satisfies` id no criterion
//! defines — and travels whether or not the scaffold refused, like
//! `validation.issues`. `refreshed` / `removed` are non-empty only before
//! the user approves the spec — see
//! [`crate::commands::wave::wave_scaffold`]'s write modes. Keys serialize in
//! insertion order (the workspace enables serde_json's `preserve_order`), which
//! is fixed by this module, so the document is byte-stable either way; no
//! timestamps or volatile paths appear on stdout.

use crate::commands::event::emit_phase;
use crate::commands::review::analyze_validation;
use crate::commands::wave::wave_dependency;
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

/// Stdout `scaffold.error` marker for a scaffold that materialised but left a
/// parent/plan acceptance criterion uncovered by every wave. [`run`] maps it to
/// exit 2 and [`materialize`] withholds the PLAN transition — the coverage gate,
/// enforced unconditionally in the pipeline (no env knob).
const ERR_UNCOVERED_ACS: &str = "uncovered acceptance criteria";

/// Stdout `scaffold.error` marker for a plan that CLAIMS a criterion it cannot
/// support — a wave doing work that satisfies a criterion while declaring no
/// files. [`run`] maps it to exit 2 like the coverage gate, and for the same
/// reason: it is a fact the plan's own contents settle, not a judgement about
/// whether the declared files would have been enough.
const ERR_UNSUPPORTABLE_CLAIMS: &str = "unsupportable acceptance-criteria claims";

/// Stdout `scaffold.error` marker for a criterion whose command inspects a path
/// no wave claiming it declares — the SUFFICIENCY gate. Apart from the coverage
/// marker on purpose: coverage asks whether SOME wave claimed the id, this asks
/// whether the claiming wave can actually satisfy it, and the two ask the
/// reader to edit different lines. Mapped to exit 2 like its two siblings.
const ERR_CRITERIA_OUTSIDE_CLAIMANTS: &str = "acceptance criteria outside their claimants";

/// Stdout `sharedFiles.error` marker for a plan whose dispatch-parallel waves
/// declare the same file. [`run`] maps it to exit 2 and [`materialize`] withholds
/// the PLAN transition — like the coverage gate, and with no env knob: waves of
/// one level are dispatched together by definition, so nothing else in the plan
/// is sequencing them, and the condition has no false positive to leave room for
/// a mode.
const ERR_SAME_LEVEL_FILE_COLLISION: &str = "same-level file collision";

/// The `type` each collision issue carries — one string, so the report's rows and
/// the prose that teaches them cannot drift apart.
const TYPE_SAME_LEVEL_FILE_COLLISION: &str = "same-level-file-collision";

/// CLI entry — resolves the paths against the cwd and prints the composite
/// report. Exit code: 0 on success and on advisory failures (validation and the
/// dependency DAG are WARN-level; failures are expressed in the JSON), 2 when
/// the plan file could not be read/parsed, an uncovered acceptance criterion
/// tripped the coverage gate, two dispatch-parallel waves declared the same
/// file, or an unproven criterion tripped the negative-test gate — either way a
/// non-zero exit so the orchestrator notices.
pub fn run(opts: PlanMaterializeOpts) {
    let project = PathBuf::from(crate::shared::context::project_dir());
    // Accept the three spec-dir spellings (a directory, a `…/spec.md` path, a
    // bare slug) through the shared normaliser before absolutizing.
    let spec_dir = absolutize(
        &project,
        crate::shared::context::normalise_spec_dir(&project, &opts.spec_dir),
    );
    let plan_path = absolutize(&project, &opts.plan);
    let report = materialize(&project, &spec_dir, &plan_path);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
    );
    if refused(&report) {
        std::process::exit(2);
    }
}

/// `true` when the composite REFUSED — the single reading of the report that
/// maps to exit 2, shared by both doors so the CLI's exit code and the fused
/// caller's rollback can never disagree about what a refusal is.
///
/// A refusal is a blocking gate: the plan could not be read, one of the three
/// scaffold gates (coverage / unsupportable claims / sufficiency) fired, or two
/// dispatch-parallel waves declared the same file. The WARN-level signals (`validation`, `dependencies`, and the
/// scaffold's `untraced_waves`) are NOT refusals — they are expressed in the
/// JSON and the plan still materialises.
pub(crate) fn refused(report: &Value) -> bool {
    let scaffold_err = report["scaffold"]["error"].as_str();
    // Every report this module builds carries the slot, so an absent one is a
    // malformed document, not a clean plan.
    let disjoint = report["sharedFiles"]["ok"].as_bool().unwrap_or(false);
    scaffold_err == Some(ERR_PLAN_UNREADABLE)
        || scaffold_err == Some(ERR_UNCOVERED_ACS)
        || scaffold_err == Some(ERR_UNSUPPORTABLE_CLAIMS)
        || scaffold_err == Some(ERR_CRITERIA_OUTSIDE_CLAIMANTS)
        || !disjoint
}

/// The FIRST-materialisation entry point — the same composite [`run`] performs,
/// with one added obligation: a refusal leaves NO layout behind.
///
/// The difference is not a mode, it is the caller's situation. `plan-materialize`
/// is the RE-materialisation door: it reconciles a layout onto an edited plan,
/// so the artefacts it meets are its own from a previous pass and leaving them
/// in place after a refusal is what makes the re-run repair them. `spec-draft
/// --plan` is the first pass over a directory that had no layout at all; if it
/// half-materialised and then exited 2, the operator's retry would meet wave
/// directories THIS run created and report them as `skipped`, so the report
/// would stop describing what the call produced.
///
/// Only what the scaffold CREATED in this pass is removed (`created_files`),
/// deepest path first, then the directories that were left empty. Nothing that
/// pre-existed is touched: the root `spec.md` / `meta.json` are the draft's own
/// artefacts (and the criterion the operator must fix lives in the first of
/// them), and the proof LEDGER is the record of the refusal — deleting either
/// would take away what the retry is supposed to act on.
pub(crate) fn materialize_fresh(project: &Path, spec_dir: &Path, plan_path: &Path) -> Value {
    let before = immediate_entries(spec_dir);
    let report = materialize(project, spec_dir, plan_path);
    if refused(&report) {
        rollback_layout(spec_dir, &report, &before);
    }
    report
}

/// The names directly under `spec_dir`, read BEFORE the composite runs — the
/// baseline that tells what the materialisation brought into being from what was
/// already there. An unreadable directory yields an empty set, which makes the
/// rollback narrower (it removes only what the scaffold itself reported), never
/// wider.
fn immediate_entries(spec_dir: &Path) -> std::collections::BTreeSet<String> {
    fs::read_dir(spec_dir)
        .map(|entries| entries.into_iter().map(|e| e.file_name).collect())
        .unwrap_or_default()
}

/// Undo the layout this pass materialised: every file the scaffold reported as
/// CREATED, plus every directory that did not exist before the pass.
///
/// Two rules rather than one because the two artefacts are known differently. A
/// file is governed by the scaffold's own ledger, so `wave-plan.md` goes and a
/// file that was merely `skipped` or `refreshed` stays. A DIRECTORY that was not
/// there beforehand exists only because this pass ran, so it goes whole —
/// including the sidecars written inside it that never pass through the file
/// ledger, which is exactly what a per-file undo would leave stranded.
///
/// Everything else is untouched on purpose: `spec.md` and `meta.json` are the
/// draft's own artefacts (and the criterion to fix lives in the first), and the
/// proof LEDGER is the record of why this refusal happened.
fn rollback_layout(
    spec_dir: &Path,
    report: &Value,
    before: &std::collections::BTreeSet<String>,
) {
    for rel in report["scaffold"]["created_files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let _ = fs::remove_file(spec_dir.join(rel));
    }
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return;
    };
    for entry in entries.into_iter().filter(|e| e.is_dir) {
        if !before.contains(&entry.file_name) {
            let _ = fs::remove_dir_all(spec_dir.join(&entry.file_name));
        }
    }
}

/// Join a possibly-relative CLI path onto the project root.
fn absolutize(project: &Path, raw: impl AsRef<Path>) -> PathBuf {
    let raw = raw.as_ref();
    if raw.is_absolute() {
        raw.to_path_buf()
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

    // 1. the wave-scaffold renderer (called in-process — this composite is its
    //    only entry point). Idempotent; reconciled before approval, frozen
    //    after (see `wave_scaffold::WriteMode`).
    let outcome = wave_scaffold::scaffold(spec_dir, plan_path);
    // 1b. Size audit of what was just materialised — advisory, stderr only.
    //     `wave-size-check` computed these numbers from the day it was ported
    //     and NO step of the pipeline read them, so a plan carrying a 19-file,
    //     13-task wave was accepted in silence. This is the moment the shape is
    //     visible and still cheap to change. It never blocks and never touches
    //     stdout, so the machine-read report stays byte-stable.
    crate::commands::wave::wave_size_check::warn_oversized_waves(spec_dir);
    let (scaffold_json, scaffold_ok) = match outcome {
        // Coverage gate (unconditional — no env knob): a parent/plan acceptance
        // criterion that no wave covers BLOCKS the PLAN transition. The layout
        // was materialised (idempotent), but `scaffold_ok=false` withholds the
        // events and `run` exits non-zero, so the gap is fixed before EXECUTE.
        //
        // `untraced_waves` travels in BOTH arms and decides neither: it is
        // advisory, like `validation.issues` — a wave with tasks and no
        // criterion is dispatched with its `## ACCEPTANCE` collapsed, and the
        // report says so without withholding the plan.
        ScaffoldOutcome::Created {
            created,
            skipped,
            refreshed,
            removed,
            uncovered_acs,
            unsupportable_claims,
            criteria_outside_claimants,
            untraced_waves,
        } if uncovered_acs.is_empty()
            && unsupportable_claims.is_empty()
            && criteria_outside_claimants.is_empty() =>
        {
            (
                json!({
                    "created_files": created,
                    "skipped": skipped,
                    "refreshed": refreshed,
                    "removed": removed,
                    "untraced_waves": untraced_waves,
                }),
                true,
            )
        }
        // All three lists are settled facts about the plan's own contents, so
        // they share one refusal — but they are REPORTED apart, because a
        // criterion that is claimed-but-unsupportable is not the same thing as
        // one nobody claimed, nor as one whose claimant cannot reach a path its
        // command inspects, and a reader acting on the wrong one fixes the
        // wrong plan.
        ScaffoldOutcome::Created {
            created,
            skipped,
            refreshed,
            removed,
            uncovered_acs,
            unsupportable_claims,
            criteria_outside_claimants,
            untraced_waves,
        } => (
            json!({
                "created_files": created,
                "skipped": skipped,
                "refreshed": refreshed,
                "removed": removed,
                // Coverage first, then the contradiction, then sufficiency: a
                // criterion nobody claimed cannot also be judged on whether its
                // claimant reaches its paths, so the earlier question owns the
                // headline while every list travels in full.
                "error": if !uncovered_acs.is_empty() {
                    ERR_UNCOVERED_ACS
                } else if !unsupportable_claims.is_empty() {
                    ERR_UNSUPPORTABLE_CLAIMS
                } else {
                    ERR_CRITERIA_OUTSIDE_CLAIMANTS
                },
                "uncovered_acs": uncovered_acs,
                "unsupportable_claims": unsupportable_claims,
                "criteria_outside_claimants": criteria_outside_claimants,
                "untraced_waves": untraced_waves,
            }),
            false,
        ),
        ScaffoldOutcome::EmptyPlan => (
            json!({
                "created_files": [],
                "skipped": [],
                "refreshed": [],
                "removed": [],
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
                    "refreshed": [],
                    "removed": [],
                    "error": ERR_PLAN_UNREADABLE,
                }),
                false,
            )
        }
    };

    // 2. analyze-validation over the root spec.md (the spec-draft output).
    //    WARN-level by contract — never blocks the scaffold or the events.
    let validation = validate_root_spec(project, spec_dir);

    // 2b. Dependency-DAG validation over the plan's file union — folded in from
    //     `wave-dependency` (it reads the same `plan.json`), so the import-cycle
    //     check runs as part of materialisation instead of depending on the
    //     orchestrator relaying a separate `wave-dependency` call it may skip.
    //     WARN-level, like the analyze-validation above: a cycle never blocks the
    //     scaffold (the planner's explicit boundaries stand), it flags a wave
    //     order the imports say is not executable. Reuses the DAG builder — no
    //     second import parser.
    let dependencies = wave_dependency::validate_plan_dag(plan_path, project);

    // 2bb. The SAME-LEVEL file collision gate, read from the same plan.json —
    //      BLOCKING, unlike 2b. A dependency level IS the dispatch round: two
    //      waves that share one have no edge between them, so a file both
    //      declare puts two agents in it concurrently with nothing sequencing
    //      them. The advisory reading of this same fact already exists
    //      (`wave-overlap-check`, at the approval gate) and was measured
    //      insufficient — the field report took three manual rounds to zero a
    //      four-wave, three-file overlap. Here the plan is still cheap to edit,
    //      and the refusal hands over the one edge that fixes each pair.
    let collisions = wave_dependency::plan_file_collisions(plan_path);
    let disjoint = collisions.is_empty();
    let collision_issues: Vec<Value> = collisions
        .iter()
        .map(|c| {
            json!({
                "severity": "ERROR",
                "type": TYPE_SAME_LEVEL_FILE_COLLISION,
                "level": c.level,
                "waves": c.waves,
                "files": c.files,
                "chain": c.chain,
                "message": format!(
                    "waves {a} and {b} share dispatch level {level} — nothing sequences them — \
                     and both declare {files}. Minimal chaining: {chain}. Splitting the file \
                     between the two waves is the other repair, and stays the author's call.",
                    a = c.waves[0],
                    b = c.waves[1],
                    level = c.level,
                    files = c.files.join(", "),
                    chain = c.chain,
                ),
            })
        })
        .collect();
    let mut shared_files = json!({ "ok": disjoint, "issues": collision_issues });
    if !disjoint {
        shared_files["error"] = json!(ERR_SAME_LEVEL_FILE_COLLISION);
    }

    // 3 + 4. Events — only for a plan that actually materialised (no PLAN
    //    transition for a spec whose scaffold failed) and a resolvable slug.
    let mut events: Vec<String> = Vec::new();
    if scaffold_ok && disjoint && !spec.is_empty() {
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
        "dependencies": dependencies,
        "sharedFiles": shared_files,
    })
}

/// Run the WARN-level structural validation over `<spec_dir>/spec.md`,
/// reusing the exact `analyze-validation` checks (layer coverage, file refs,
/// task counts, AC parseability). A missing/unreadable `spec.md` degrades to
/// `ok: false` with a single ERROR issue — `plan-materialize` pressupposes the
/// draft already ran, so the gap is surfaced, not silently skipped.
///
/// `project` is the root [`run`] already resolved, handed down so the file-ref
/// check resolves against the SAME tree this composite works in (off-root — a
/// worktree — the process working directory is a different project).
fn validate_root_spec(project: &Path, spec_dir: &Path) -> Value {
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
            let issues = analyze_validation::validate(project, &spec_md, &content);
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
        // O `## Acceptance Criteria` do pai é load-bearing: o `satisfies` de cada
        // onda só é régua se nomear um critério que EXISTE. Sem esta seção os
        // dois ids são fantasmas e o plano é recusado — que é o caso que a
        // fixture NÃO quer medir.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Demo\n\n## Files\n- `a.rs` (create)\n\n### Backend Agent\n- [ ] t1\n- [ ] t2\n\n\
             ## Acceptance Criteria\n\
             - **AC-1** — o comportamento novo vale. Command: `cd no-such-directory-abc`\n\
             - **AC-2** — build green. Command: `cd .`\n",
        )
        .unwrap();
        let plan_path = project.join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                // `files` + `satisfies` are load-bearing, not decoration: a wave
                // that does work and traces to NO criterion is refused (its
                // dispatched prompt would carry no ruler), and one that claims a
                // criterion while declaring nowhere to do the work is refused by
                // the claim-support gap. This fixture is the HAPPY path, so it
                // has to clear both.
                "waves": [
                    { "n": 1, "role": "rt", "summary": "base", "depends_on": [],
                      "tasks": ["do the thing"], "files": ["src/rt.rs"],
                      "satisfies": ["AC-1"] },
                    { "n": 2, "role": "cli", "summary": "wire", "depends_on": ["wave-1-rt"],
                      "tasks": ["wire it"], "files": ["src/cli.rs"],
                      "satisfies": ["AC-2"] }
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
    ///
    /// AC-4 rides along: re-running with an UNCHANGED plan must create,
    /// refresh and remove nothing (the four scaffold lists are always present,
    /// so stdout stays byte-stable) and must not re-emit the PLAN phase.
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
        // The two reconcile lists are always published, empty on a first pass.
        assert_eq!(report["scaffold"]["refreshed"], json!([]), "{report}");
        assert_eq!(report["scaffold"]["removed"], json!([]), "{report}");

        // Validation: the seeded spec is clean.
        assert_eq!(report["validation"]["ok"], json!(true), "{report}");

        // Dependency-DAG check is folded in and always present in the report
        // (clean here — the seed plan declares no import edges).
        assert_eq!(report["dependencies"]["ok"], json!(true), "{report}");

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

        // Idempotent re-run on an UNCHANGED plan: nothing created, nothing
        // rewritten, nothing pruned — the whole report stays byte-stable — and
        // the phase emit is skipped inside run_at (last phase already PLAN).
        let again = materialize(project, &spec_dir, &plan_path);
        assert!(again["scaffold"]["created_files"].as_array().unwrap().is_empty());
        assert_eq!(again["scaffold"]["refreshed"], json!([]), "{again}");
        assert_eq!(again["scaffold"]["removed"], json!([]), "{again}");
        assert!(
            !again["scaffold"]["skipped"].as_array().unwrap().is_empty(),
            "an unchanged plan skips every artefact: {again}"
        );
        // Byte-stable: a third pass prints exactly what the second did.
        let third = materialize(project, &spec_dir, &plan_path);
        assert_eq!(
            serde_json::to_string_pretty(&again).unwrap(),
            serde_json::to_string_pretty(&third).unwrap(),
            "re-running an unchanged plan must be byte-identical on stdout"
        );
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

    /// Coverage gate (unconditional — no env knob): a parent spec.md AC that no
    /// wave claims BLOCKS — the scaffold carries the `error` marker, `scaffold_ok`
    /// is false so NO events are emitted, and `run` maps it to exit 2.
    #[test]
    fn composite_plan_materialize_uncovered_parent_ac_blocks() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("demo-cov");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Parent declares two ACs; the plan routes only AC-1 onto a wave.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Demo\n\n## Files\n- `a.rs` (create)\n\n## Acceptance Criteria\n- **AC-1** — a. Command: `true`\n- **AC-2** — b. Command: `true`\n",
        )
        .unwrap();
        let plan_path = project.join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    // `files` declared so this fixture trips ONLY the coverage
                    // gate: a wave that does work and claims a criterion while
                    // declaring nowhere to do it is a different refusal, and
                    // leaving it in would make this test pass by the error-marker
                    // precedence rather than by isolating what it names.
                    { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                      "files": ["src/a.rs"], "satisfies": ["AC-1"] }
                ],
                "total_waves": 1,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();

        let report = materialize(project, &spec_dir, &plan_path);

        // Blocked: error marker + the uncovered AC-2 listed.
        assert_eq!(
            report["scaffold"]["error"],
            json!("uncovered acceptance criteria"),
            "{report}"
        );
        assert!(
            report["scaffold"]["uncovered_acs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|g| g.as_str().unwrap_or_default().contains("AC-2")),
            "AC-2 must be listed as uncovered: {report}"
        );
        // No PLAN transition on a blocked scaffold.
        assert_eq!(report["events"], json!([]), "blocked scaffold emits nothing: {report}");
    }

    /// AC-4: a duty declared in the PLAN reaches the dispatched agent's prompt
    /// as its OWN section — the whole path, plan JSON → wave scaffold → rendered
    /// prompt, because each hop alone proves nothing about the one after it.
    ///
    /// Two-sided: the sibling wave declares no duty and must render NO such
    /// section, so the assertion cannot pass by the section appearing everywhere.
    #[test]
    fn plan_reality_obligations_reach_wave_prompt() {
        use crate::commands::agent::render::{render_prompt_at, RenderMode};

        let dir = tempdir().unwrap();
        let project = dir.path();
        std::fs::write(project.join("mustard.json"), b"{}").unwrap();
        let spec_dir = project.join(".claude").join("spec").join("reality-plan");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Os critérios que o `satisfies` das ondas nomeia: um id que não existe
        // não é régua, e o plano seria recusado por uma razão que não é a desta
        // fixture.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Demo\n\n## Files\n- `a.rs` (create)\n\n\
             ## Acceptance Criteria\n\
             - **AC-1** — o comportamento novo vale. Command: `cd no-such-directory-abc`\n\
             - **AC-2** — build green. Command: `cd .`\n",
        )
        .unwrap();
        let plan_path = project.join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    // `satisfies` on both waves: a wave with tasks and no
                    // criterion is refused, and this fixture is about the DUTY
                    // path — it must not trip an unrelated gate.
                    { "n": 1, "role": "rt", "summary": "wire it", "tasks": ["wire the webhook"],
                      "files": ["src/hook.rs"], "satisfies": ["AC-1"],
                      "reality_obligations": [
                          "read the provider's official webhook doc for the retry semantics"
                      ] },
                    { "n": 2, "role": "cli", "summary": "render it", "tasks": ["render it"],
                      "files": ["src/cli.rs"], "satisfies": ["AC-2"] }
                ],
                "total_waves": 2,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();

        let report = materialize(project, &spec_dir, &plan_path);
        assert!(report["scaffold"]["error"].is_null(), "{report}");

        // The scaffold materialised the duty into the wave's own spec.
        let wave1 = std::fs::read_to_string(spec_dir.join("wave-1-rt").join("spec.md")).unwrap();
        assert!(wave1.contains("## Reality Obligations"), "{wave1}");
        assert!(wave1.contains("**RO-1.1**"), "the duty carries an id: {wave1}");

        // ...and the dispatch prompt carries it as its own section.
        let rendered = render_prompt_at(
            project,
            Some("reality-plan"),
            Some(1),
            "impl",
            Path::new("."),
            RenderMode::First,
            None,
            None,
            None,
        );
        assert!(
            rendered.contains("## REALITY OBLIGATIONS"),
            "no reality section in the prompt: {rendered}"
        );
        assert!(
            rendered.contains("RO-1.1") && rendered.contains("official webhook doc"),
            "the duty did not reach the prompt: {rendered}"
        );
        assert!(
            rendered.contains("account for each duty BY ITS ID"),
            "the agent is never told how to report back: {rendered}"
        );

        // The sibling wave declared none: no heading at all, not an empty one.
        let sibling = render_prompt_at(
            project,
            Some("reality-plan"),
            Some(2),
            "impl",
            Path::new("."),
            RenderMode::First,
            None,
            None,
            None,
        );
        assert!(
            !sibling.contains("## REALITY OBLIGATIONS"),
            "a wave with no duty must render no section: {sibling}"
        );
    }
}
