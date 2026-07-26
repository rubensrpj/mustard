//! `mustard-rt run approve-spec` — deterministic spec-approval event sequence.
//!
//! Replaces the hand-assembled `emit-pipeline` sequence the legacy approve
//! flow (now `plugin/refs/spec/resume-loop.md § A`) used to make the LLM run
//! by hand (emit `pipeline.stage Plan` then `pipeline.status
//! from:draft,to:approved`; patch the wave-1 `meta.json` for dispatch). The orchestrator now relays a
//! single `mustard-rt run approve-spec` invocation and acts on the JSON report.
//!
//! ## Emitted sequence (in order)
//!
//! 1. `pipeline.stage` → `{"stage":"Plan"}` — records planning complete.
//! 2. `pipeline.status` → `{"from":"draft","to":"approved"}` — the canonical
//!    D5 approval signal.
//! 3. *(only with `--resume`)* `pipeline.stage` → `{"stage":"Execute"}` — the
//!    inline-resume case; without `--resume` the flow STOPS at `approved` so a
//!    fresh session resumes EXECUTE with clean context.
//!
//! With `--wave-plan`, each `pipeline.stage` payload carries `"wave":1` so the
//! existing `emit_pipeline` machinery patches the wave-1 `meta.json` sidecar
//! for dispatch (it resolves `wave-1-*` and runs the canonical
//! `Meta` read-modify-write — no parallel writer here).
//!
//! ## Reuse, not duplication
//!
//! The event sequence is defined once by [`approval_sequence`]. The CLI entry
//! [`run`] feeds each step to [`crate::commands::event::emit_pipeline::run`]
//! (module-qualified — exactly the precedent set by
//! [`crate::hooks::observe::wave_complete_observer`]); no subprocess, no
//! duplicated NDJSON-writing logic, no facade. The cwd-aware
//! [`emit_via_route`] used in tests routes the same events through
//! [`crate::shared::events::route::emit`] — the identical write path
//! `emit_pipeline::run` ends in — so the tests assert the real on-disk
//! `.events/` log without mutating the process working directory.
//!
//! ## Fail-open contract
//!
//! Every emit is best-effort (the underlying `emit_pipeline::run` / `route::emit`
//! swallow store/IO errors). The command never panics on a DB/IO error; it
//! prints a JSON report `{"ok":true,"spec":"<name>","approved":true,
//! "resumed":<bool>}` on success (mirroring the report style of
//! `tactical-fix-create`), or `{"ok":false,"error":"..."}` on a real failure
//! (an empty spec name).

use serde::Serialize;
use serde_json::{json, Value};

use crate::shared::context::MarkerProvenance;

/// Options for `mustard-rt run approve-spec`.
#[derive(Debug, Clone)]
pub struct ApproveSpecOpts {
    /// Spec slug under `.claude/spec/` whose approval to emit.
    pub spec: String,
    /// The spec is a wave plan — tag each `pipeline.stage` with `wave:1` so the
    /// wave-1 `meta.json` sidecar is patched for dispatch.
    pub wave_plan: bool,
    /// Inline-resume: also emit `pipeline.stage Execute` so the same session
    /// can jump straight into EXECUTE (the `r`-suffix branch of the flow).
    pub resume: bool,
}

/// JSON success report. Mirrors the `tactical-fix-create` style (flat, typed).
///
/// `approvedVia` / `approvedAt` echo the provenance the approval marker already
/// records — the door the approval came through and when. Both are omitted (not
/// null) when the marker's body is unreadable or predates those keys: the marker
/// EXISTENCE is what governed the gate above, so a missing echo degrades the
/// report to silence, never to a failure.
#[derive(Debug, Serialize)]
struct ApproveReport {
    ok: bool,
    spec: String,
    approved: bool,
    resumed: bool,
    #[serde(rename = "approvedVia", skip_serializing_if = "Option::is_none")]
    approved_via: Option<String>,
    #[serde(rename = "approvedAt", skip_serializing_if = "Option::is_none")]
    approved_at: Option<String>,
}

/// The approval provenance to echo, read through the single reader in
/// [`crate::shared::context::read_marker_provenance`].
///
/// `None` when the marker is absent OR its body is unreadable — the gate in
/// [`run`] already decided approval from the marker's EXISTENCE, so this read is
/// purely informative and can never itself refuse a spec.
fn approval_provenance(cwd: &str, spec: &str) -> Option<MarkerProvenance> {
    let path = crate::shared::context::approval_marker_path(cwd, spec)?;
    crate::shared::context::read_marker_provenance(&path)
}

/// JSON failure report.
#[derive(Debug, Serialize)]
struct ApproveError {
    ok: bool,
    error: String,
}

/// One step of the approval sequence: an `emit-pipeline` kind + its JSON payload.
type Step = (&'static str, Value);

/// Build the ordered approval event sequence for the given options.
///
/// This is the single source of truth for *what* approve-spec emits; both the
/// CLI entry (via `emit_pipeline::run`) and the tests (via `route::emit`)
/// consume it, so there is exactly one definition of the order + payloads.
///
/// - `pipeline.stage {stage:"Plan"}` — planning complete.
/// - `pipeline.status {from:"draft",to:"approved"}` — the D5 approval signal.
/// - `pipeline.stage {stage:"Execute"}` — only when `resume` (inline EXECUTE).
///
/// When `wave_plan`, the two `pipeline.stage` steps additionally carry
/// `"wave":1` so `emit_pipeline` syncs the wave-1 sidecar instead of the parent.
fn approval_sequence(wave_plan: bool, resume: bool) -> Vec<Step> {
    let stage_payload = |stage: &str| -> Value {
        if wave_plan {
            json!({ "stage": stage, "wave": 1 })
        } else {
            json!({ "stage": stage })
        }
    };

    let mut steps: Vec<Step> = vec![
        ("pipeline.stage", stage_payload("Plan")),
        (
            "pipeline.status",
            json!({ "from": "draft", "to": "approved" }),
        ),
    ];
    if resume {
        steps.push(("pipeline.stage", stage_payload("Execute")));
    }
    steps
}

// ---------------------------------------------------------------------------
// The approval gate — clarify (F6) + user-approval (T5), evaluated together.
//
// `approve-spec` may emit the `draft→approved` signal ONLY once BOTH preconditions
// hold: a Full plan is CLARIFIED (`<spec>/.clarified`, F6) and a real user has
// APPROVED it (`<spec>/.approved-by-user`, T5). Each marker is born from an act the
// model cannot author — the deliberate clarification finalize, and the user's own
// `AskUserQuestion` / `ExitPlanMode` answer echoed in `tool_response`. A gate the
// gated could open by running this very command is not a gate.
//
// The two preconditions are checked in ONE pass so a single refusal names EVERY
// unmet marker with its minting path — the pre-refactor gate exited on the first
// miss and hid the second, costing the user a second failed run to discover it.
// Mode reads exactly like the `MUSTARD_*_GATE_MODE` close-gate family.
// ---------------------------------------------------------------------------

/// Three-state mode for the user-approval requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalMode {
    /// Emit approval unconditionally (pre-T5 behaviour).
    Off,
    /// Warn on a missing marker but proceed.
    Warn,
    /// Refuse (exit≠0) on a missing marker — the default.
    Strict,
}

/// Map a mode string to [`ApprovalMode`]; an absent/unknown value is `strict`
/// (the safe default — an approval must be proven, never assumed).
fn parse_approval_mode(s: &str) -> ApprovalMode {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => ApprovalMode::Off,
        "warn" => ApprovalMode::Warn,
        _ => ApprovalMode::Strict,
    }
}

/// Resolve `MUSTARD_APPROVAL_MODE` (default `strict`), mirroring the cascade the
/// close-gate family uses for `MUSTARD_QA_GATE_MODE` / `MUSTARD_COMMIT_GATE_MODE`
/// (`resolve_mode`): a non-empty env value wins; absent or blank → `strict`.
fn resolve_approval_mode() -> ApprovalMode {
    std::env::var("MUSTARD_APPROVAL_MODE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map_or(ApprovalMode::Strict, |v| parse_approval_mode(&v))
}

/// The gate outcome for a resolved mode + marker presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalGate {
    /// Emit the approval sequence as normal.
    Proceed,
    /// Emit, but first warn that no user approval was recorded.
    Warn,
    /// Refuse — no user approval recorded and the mode is strict.
    Block,
}

/// Decide the gate outcome. Pure: the env read and the marker existence check are
/// resolved by the caller, so the whole policy is unit-testable without touching
/// process-global state (env / cwd).
fn approval_gate(mode: ApprovalMode, marker_present: bool) -> ApprovalGate {
    match mode {
        ApprovalMode::Off => ApprovalGate::Proceed,
        _ if marker_present => ApprovalGate::Proceed,
        ApprovalMode::Warn => ApprovalGate::Warn,
        ApprovalMode::Strict => ApprovalGate::Block,
    }
}

/// What the clarify marker says — the three states the gate distinguishes, plus
/// the case where clarify does not apply at all.
///
/// The middle state is the one this wave exists for: a marker that EXISTS but
/// recorded nothing. Minting it costs the orchestrator one command it can run
/// seconds before the approval the marker unlocks, so existence alone proves
/// only that the command ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClarifyState {
    /// Not a Full spec — Light / task specs are never clarify-gated.
    NotGated,
    /// No `<spec>/.clarified` at all.
    Missing,
    /// The marker exists but names no captured term and states no reason.
    Hollow,
    /// The marker records what was settled — terms, or a stated reason.
    Recorded,
}

/// Read the clarify marker for `spec` under `root` and classify what it carries.
///
/// **Fail-CLOSED, deliberately.** Every other check in this crate degrades to
/// "allow" when a read fails — a hook must never block a session over its own
/// I/O error. This one is not a hook: it guards a VERDICT (the `draft→approved`
/// signal), and its whole job is to refuse when the clarification cannot be
/// shown to have happened. An unreadable or unparsable body therefore reads as
/// [`ClarifyState::Hollow`] and REFUSES, for the same reason an absent marker
/// already refused today. A reader who assumes the crate-wide fail-open rule
/// here would get this backwards, which is why it is spelled out.
fn clarify_state(root: &str, spec: &str) -> ClarifyState {
    if !spec_is_full(root, spec) {
        return ClarifyState::NotGated;
    }
    let Some(path) = crate::shared::context::clarified_marker_path(root, spec) else {
        return ClarifyState::Missing;
    };
    if !path.exists() {
        return ClarifyState::Missing;
    }
    match crate::shared::context::read_marker_provenance(&path) {
        Some(p) if p.records_substance() => ClarifyState::Recorded,
        _ => ClarifyState::Hollow,
    }
}

/// Build the aggregated refusal that names EVERY unmet approval precondition at
/// once — clarify (F6, `<spec>/.clarified`) and/or user-approval (T5,
/// `<spec>/.approved-by-user`) — each with the path that mints it. One message so
/// the user sees everything missing in a single run, instead of the pre-refactor
/// gate's first-miss-only refusal. `spec` is interpolated into the clarify
/// finalize command. Returns `None` when nothing is missing — the caller then
/// proceeds silently. Surfaced as the report `error`; the flow relays
/// `{ok:false,error}` straight to the user.
///
/// A [`ClarifyState::Hollow`] marker earns its OWN wording: the remedy is not
/// "run the finalize" (it already ran) but "run it saying what it settled", so
/// the refusal names the grill to run or tells the caller to state a reason.
fn unmet_gate_message(
    spec: &str,
    clarify: ClarifyState,
    approval_missing: bool,
) -> Option<String> {
    let mut unmet: Vec<String> = Vec::new();
    if clarify == ClarifyState::Missing {
        unmet.push(format!(
            "clarify — no `<spec>/.clarified`: run the clarification finalize \
             `mustard-rt run grill-capture --finalize --spec {spec} --term <term>` (once per \
             term the ANALYZE glossary grill confirmed), or state why no grill applied \
             `--reason \"<sentence>\"` — a complete glossary is a legitimate reason, so a \
             clear-glossary spec is never deadlocked"
        ));
    }
    if clarify == ClarifyState::Hollow {
        unmet.push(format!(
            "clarify — `<spec>/.clarified` recorded NOTHING: the marker exists but names no \
             captured term and states no reason, so it proves only that the finalize ran. \
             Re-run it saying what was settled: `mustard-rt run grill-capture --finalize \
             --spec {spec} --term <term>` for each term the ANALYZE glossary grill confirmed \
             (`mustard-rt run glossary-coverage --intent \"<intent>\" --context <CONTEXT.md>` \
             lists the uncovered ones), or `--reason \"<sentence>\"` stating why no grill \
             applied"
        ));
    }
    if approval_missing {
        unmet.push(
            "approval — no `<spec>/.approved-by-user`: the user accepts the plan in plan \
             mode (ExitPlanMode) or answers the approval AskUserQuestion (fallback); the \
             model cannot forge either"
                .to_string(),
        );
    }
    if unmet.is_empty() {
        return None;
    }
    let list = unmet
        .iter()
        .map(|u| format!("- {u}"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "approve-spec will not self-approve a Full plan (that is what the field incident \
         did). Unmet precondition(s):\n{list}\nTo temporarily relax, set \
         MUSTARD_APPROVAL_MODE=warn or off."
    ))
}

/// `true` when `spec`'s `meta.json#scope` declares a Full-scope spec (starts with
/// `full` after a case-insensitive trim — `"full"` or `"full (wave plan)"`). Only
/// a Full plan carries the clarify gate; Light / task specs are never gated.
/// Fail-open: an unreadable `meta.json` / absent scope returns `false` (not
/// gated), the safe direction — the user-approval gate still applies to all.
fn spec_is_full(root: &str, spec: &str) -> bool {
    let Some(sp) = mustard_core::ClaudePaths::for_project(std::path::Path::new(root))
        .and_then(|p| p.for_spec(spec))
        .ok()
    else {
        return false;
    };
    mustard_core::read_meta(&sp.meta_json_path())
        .and_then(|m| m.scope)
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false)
}

/// Why no approval happened — the `error` to print, and whether the process must
/// exit non-zero.
///
/// A gate refusal exits 1 (a caller that ignores stdout still sees the failure);
/// an argument error (empty spec) stays exit 0, as it always has.
#[derive(Debug)]
struct Refused {
    error: String,
    exit_nonzero: bool,
}

/// Decide the approval against `root` and, only if it passes, feed every event
/// of the sequence to `emit`.
///
/// The seam that makes the gate testable: on a refusal `emit` is NEVER called,
/// which is the entire point of a gate, and a test can assert exactly that by
/// passing a recorder. `run` passes the canonical
/// [`crate::commands::event::emit_pipeline::run`].
///
/// Approval gate — refuse (strict) to emit the approval signal until BOTH
/// preconditions hold. Clarify (F6) precedes approval and applies only to Full
/// specs; user-approval (T5) applies to every spec. Both markers are born from
/// acts the model cannot forge (the deliberate `grill-capture --finalize`, and
/// the observer recording the user's real AskUserQuestion / ExitPlanMode answer).
/// A background job (no user, no answer, no marker) halts cleanly here and the
/// spec stays in PLAN instead of auto-approving. The two are evaluated TOGETHER
/// so one refusal names every unmet marker with its remedy.
fn approve_at(
    root: &str,
    opts: &ApproveSpecOpts,
    mode: ApprovalMode,
    emit: &mut dyn FnMut(&str, Value),
) -> Result<ApproveReport, Refused> {
    if opts.spec.trim().is_empty() {
        return Err(Refused {
            error: "empty spec name".to_string(),
            exit_nonzero: false,
        });
    }

    if mode != ApprovalMode::Off {
        // Clarify only gates Full specs, and gates them on what the marker
        // CARRIES, not merely that it is there. The spec is known explicitly here
        // via `--spec`, so there is no resolution problem (unlike the write-time
        // `scope_guard`).
        let clarify = clarify_state(root, &opts.spec);
        let approval_missing = !crate::shared::context::approval_marker_path(root, &opts.spec)
            .map(|p| p.exists())
            .unwrap_or(false);

        // A single decision over both preconditions: any unmet one yields an
        // aggregated message; `approval_gate(mode, false)` then maps mode → Block
        // (strict, exit 1) or Warn (stderr nudge, proceed). Nothing unmet → the
        // builder returns `None` and the flow proceeds silently.
        if let Some(message) = unmet_gate_message(&opts.spec, clarify, approval_missing) {
            match approval_gate(mode, false) {
                ApprovalGate::Block => {
                    return Err(Refused {
                        error: message,
                        exit_nonzero: true,
                    })
                }
                ApprovalGate::Warn => eprintln!("{message}"),
                ApprovalGate::Proceed => {}
            }
        }
    }

    for (kind, payload) in approval_sequence(opts.wave_plan, opts.resume) {
        emit(kind, payload);
    }

    // Echo the provenance the approval marker already recorded. Read AFTER the
    // gate, and independent of it: an unreadable body yields `None` here but
    // never changes `approved`, which the gate above already settled.
    let prov = approval_provenance(root, &opts.spec);
    Ok(ApproveReport {
        ok: true,
        spec: opts.spec.clone(),
        approved: true,
        resumed: opts.resume,
        approved_via: prov.as_ref().map(|p| p.via.clone()),
        approved_at: prov
            .as_ref()
            .map(|p| p.at.clone())
            .filter(|s| !s.is_empty()),
    })
}

/// CLI entry — `mustard-rt run approve-spec --spec <name> [--wave-plan] [--resume]`.
///
/// Delegates the decision + emission to [`approve_at`] against the process cwd,
/// emitting each step through the canonical
/// [`crate::commands::event::emit_pipeline::run`] (process-global cwd, like
/// `wave_complete_observer`) — no subprocess, no duplicated NDJSON logic, no
/// facade. Prints the JSON report to stdout; a gate refusal exits 1 (a gate the
/// gated cannot open by running this very command), an empty spec name exits 0.
pub fn run(opts: ApproveSpecOpts) {
    let root = crate::shared::context::cwd();
    let spec = opts.spec.clone();
    let mut emit = |kind: &str, payload: Value| {
        crate::commands::event::emit_pipeline::run(
            crate::commands::event::emit_pipeline::EmitPipelineOpts {
                kind: kind.to_string(),
                spec: spec.clone(),
                payload: Some(payload.to_string()),
                allow_no_qa: false,
                intent: None,
                base: None,
            },
        );
    };

    match approve_at(&root, &opts, resolve_approval_mode(), &mut emit) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string(&report).unwrap_or_else(|_| "{\"ok\":true}".to_string())
        ),
        Err(refused) => {
            let err = ApproveError {
                ok: false,
                error: refused.error,
            };
            println!(
                "{}",
                serde_json::to_string(&err).unwrap_or_else(|_| "{\"ok\":false}".to_string())
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
            if refused.exit_nonzero {
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use std::path::Path;
    use tempfile::tempdir;

    /// Route one approval step through the event-router against an explicit
    /// project root — the identical write path `emit_pipeline::run` ends in,
    /// minus the process-global cwd. Lets the tests assert the on-disk
    /// `.events/` log without `set_current_dir`.
    fn emit_via_route(project: &Path, spec: &str, kind: &str, payload: Value) {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-06-02T00:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("approve-spec".to_string()),
                actor_type: None,
            },
            event: kind.to_string(),
            payload,
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(project.to_str().unwrap(), &event);
    }

    /// Drive the full approval sequence (as `run` would) against a tempdir,
    /// returning the chronologically-sorted `(event, payload)` pairs read back
    /// from the spec's `.events/` log.
    fn emit_sequence_and_read(
        project: &Path,
        spec: &str,
        wave_plan: bool,
        resume: bool,
    ) -> Vec<(String, Value)> {
        for (kind, payload) in approval_sequence(wave_plan, resume) {
            emit_via_route(project, spec, kind, payload);
        }
        let events_dir = project
            .join(".claude")
            .join("spec")
            .join(spec)
            .join(".events");
        let mut events =
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        events.sort_by(|a, b| a.ts.cmp(&b.ts));
        // Keep only the first-class approval kinds the sequence emits — drop the
        // per-write economy breadcrumbs and any alias fan-out (which would carry
        // `legacy_alias`). We assert the explicit emitted set, in order.
        events
            .into_iter()
            .filter(|e| matches!(e.event.as_str(), "pipeline.stage" | "pipeline.status"))
            .filter(|e| !e.payload.get("legacy_alias").is_some_and(|v| v == &json!(true)))
            .map(|e| (e.event, e.payload))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Sequence shape (unit — no I/O)
    // -----------------------------------------------------------------------

    #[test]
    fn approval_sequence_default_stops_at_approved() {
        let steps = approval_sequence(false, false);
        let kinds: Vec<&str> = steps.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, vec!["pipeline.stage", "pipeline.status"]);
        assert_eq!(steps[0].1, json!({ "stage": "Plan" }));
        assert_eq!(steps[1].1, json!({ "from": "draft", "to": "approved" }));
    }

    #[test]
    fn approval_sequence_resume_appends_execute_stage() {
        let steps = approval_sequence(false, true);
        let kinds: Vec<&str> = steps.iter().map(|(k, _)| *k).collect();
        // Resume adds a trailing pipeline.stage Execute after approval.
        assert_eq!(
            kinds,
            vec!["pipeline.stage", "pipeline.status", "pipeline.stage"]
        );
        assert_eq!(steps[2].1, json!({ "stage": "Execute" }));
    }

    #[test]
    fn approval_sequence_wave_plan_tags_stage_with_wave_one() {
        let steps = approval_sequence(true, true);
        // Both pipeline.stage steps carry wave:1 so the wave-1 sidecar is
        // patched; the status step never carries a wave.
        assert_eq!(steps[0].1, json!({ "stage": "Plan", "wave": 1 }));
        assert_eq!(steps[2].1, json!({ "stage": "Execute", "wave": 1 }));
        assert!(steps[1].1.get("wave").is_none());
    }

    // -----------------------------------------------------------------------
    // Event order lands in the spec's `.events/` log (integration via route)
    // -----------------------------------------------------------------------

    #[test]
    fn fresh_session_emits_plan_then_approved_in_order() {
        let dir = tempdir().unwrap();
        let got = emit_sequence_and_read(dir.path(), "demo-approve", false, false);
        assert_eq!(
            got,
            vec![
                ("pipeline.stage".to_string(), json!({ "stage": "Plan" })),
                (
                    "pipeline.status".to_string(),
                    json!({ "from": "draft", "to": "approved" })
                ),
            ]
        );
    }

    #[test]
    fn resume_branch_emits_execute_stage_after_approval() {
        let dir = tempdir().unwrap();
        let got = emit_sequence_and_read(dir.path(), "demo-resume", false, true);
        let kinds: Vec<&str> = got.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            kinds,
            vec!["pipeline.stage", "pipeline.status", "pipeline.stage"]
        );
        // The last emitted event is the Execute stage transition.
        assert_eq!(got[2].1, json!({ "stage": "Execute" }));
    }

    // -----------------------------------------------------------------------
    // --wave-plan: emit_pipeline patches the wave-1 meta.json sidecar
    // -----------------------------------------------------------------------

    /// Seed a wave-plan spec dir with a `wave-1-general/meta.json` sidecar.
    fn seed_wave_plan(root: &Path, spec: &str) -> std::path::PathBuf {
        let wave_dir = root
            .join(".claude")
            .join("spec")
            .join(spec)
            .join("wave-1-general");
        std::fs::create_dir_all(&wave_dir).unwrap();
        let meta_path = wave_dir.join("meta.json");
        std::fs::write(
            &meta_path,
            br#"{"stage":"Draft","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null}"#,
        )
        .unwrap();
        meta_path
    }

    /// A `pipeline.stage {stage:"Plan","wave":1}` event patches the wave-1
    /// `meta.json` (the dispatch-readiness patch the ref step 4 did), reusing
    /// the canonical `emit_pipeline::patch_meta_for_transition` via the
    /// wave-aware payload path. Asserted by driving that helper directly with a
    /// wave payload — the same call `route`/`run` make after writing the event.
    #[test]
    fn wave_plan_stage_patches_wave_one_meta() {
        let dir = tempdir().unwrap();
        let meta_path = seed_wave_plan(dir.path(), "demo-wave");

        // The approval sequence tags the Plan stage with wave:1 under --wave-plan.
        let steps = approval_sequence(true, false);
        let (_kind, plan_payload) = &steps[0];
        assert_eq!(plan_payload, &json!({ "stage": "Plan", "wave": 1 }));

        // emit_pipeline resolves `wave-1-*` from a wave-tagged payload and runs
        // the canonical Meta read-modify-write. Exercise that exact path (the
        // same routine `run()` calls after writing the pipeline.stage event).
        crate::commands::event::emit_pipeline::patch_meta_for_transition(
            dir.path(),
            "demo-wave",
            "pipeline.stage",
            plan_payload,
            "2026-06-02T10:00:00Z",
        );

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        // Wave-1 sidecar advanced to Plan/PLAN; other fields preserved.
        assert_eq!(v["stage"], json!("Plan"), "{v}");
        assert_eq!(v["phase"], json!("PLAN"), "{v}");
        assert_eq!(v["scope"], json!("full"), "{v}");
        assert_eq!(v["checkpoint"], json!("2026-06-02T10:00:00Z"), "{v}");
    }

    // -----------------------------------------------------------------------
    // T5 — the approval gate (AC6)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_approval_mode_maps_values() {
        assert_eq!(parse_approval_mode("off"), ApprovalMode::Off);
        assert_eq!(parse_approval_mode("warn"), ApprovalMode::Warn);
        assert_eq!(parse_approval_mode("strict"), ApprovalMode::Strict);
        assert_eq!(parse_approval_mode("STRICT"), ApprovalMode::Strict);
        assert_eq!(parse_approval_mode("  warn "), ApprovalMode::Warn);
        // Unknown / empty → strict (the safe default: prove approval, don't assume).
        assert_eq!(parse_approval_mode(""), ApprovalMode::Strict);
        assert_eq!(parse_approval_mode("banana"), ApprovalMode::Strict);
    }

    #[test]
    fn approval_gate_blocks_strict_without_marker_and_proceeds_with_it() {
        // AC6 core: SEM marcador → strict FALHA (Block ⇒ exit≠0); COM marcador → procede.
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
        assert_eq!(approval_gate(ApprovalMode::Strict, true), ApprovalGate::Proceed);
        // Warn surfaces a nudge but never blocks; off restores pre-T5 behaviour.
        assert_eq!(approval_gate(ApprovalMode::Warn, false), ApprovalGate::Warn);
        assert_eq!(approval_gate(ApprovalMode::Warn, true), ApprovalGate::Proceed);
        assert_eq!(approval_gate(ApprovalMode::Off, false), ApprovalGate::Proceed);
        assert_eq!(approval_gate(ApprovalMode::Off, true), ApprovalGate::Proceed);
    }

    #[test]
    fn background_job_without_user_stops_at_plan() {
        // A background job poses no AskUserQuestion, so the observer records no
        // marker; strict `approve-spec` then refuses (Block), and the Full spec
        // cannot leave PLAN without a human. This is AC6's bg-job scenario.
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    #[test]
    fn approval_marker_presence_toggles_with_the_file() {
        // The exact path `approve-spec` gates on is the one the observer writes
        // — `<spec>/.approved-by-user`. Toggling the file flips the decision.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "epic";
        std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
        let marker = crate::shared::context::approval_marker_path(root.to_str().unwrap(), spec)
            .expect("marker path resolves");
        assert!(marker.ends_with(".approved-by-user"));

        assert!(!marker.exists(), "no marker yet");
        assert_eq!(approval_gate(ApprovalMode::Strict, marker.exists()), ApprovalGate::Block);

        std::fs::write(&marker, b"spec=epic\n").unwrap();
        assert!(marker.exists(), "marker present after the observer writes it");
        assert_eq!(approval_gate(ApprovalMode::Strict, marker.exists()), ApprovalGate::Proceed);
    }

    // -----------------------------------------------------------------------
    // F6 — the clarify gate (clarify precedes approval)
    // -----------------------------------------------------------------------

    #[test]
    fn spec_is_full_reads_meta_scope() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        for (spec, scope, want) in [
            ("epic", "full (wave plan)", true),
            ("epic2", "full", true),
            ("small", "light", false),
        ] {
            let sp = mustard_core::ClaudePaths::for_project(root)
                .unwrap()
                .for_spec(spec)
                .unwrap();
            std::fs::create_dir_all(sp.dir()).unwrap();
            std::fs::write(
                sp.meta_json_path(),
                format!(r#"{{"scope":"{scope}","stage":"Plan","outcome":"Active"}}"#),
            )
            .unwrap();
            assert_eq!(spec_is_full(root_str, spec), want, "scope={scope}");
        }
        // No meta.json → not full (fail-open: the clarify gate does not apply).
        assert!(!spec_is_full(root_str, "ghost"));
    }

    /// Seed a Full spec dir + `meta.json` so `spec_is_full` says yes, and return
    /// the project root string.
    fn seed_full_spec(root: &Path, spec: &str) {
        let sp = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec(spec)
            .unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.meta_json_path(),
            r#"{"scope":"full (wave plan)","stage":"Plan","outcome":"Active"}"#,
        )
        .unwrap();
    }

    #[test]
    fn clarify_gate_blocks_full_without_marker_and_proceeds_with_a_record() {
        // Mirrors the user-approval gate, for the F6 clarify precondition —
        // except the clarify marker must CARRY something: absent → Missing, a
        // body that recorded nothing → Hollow, a recorded term → Recorded.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        seed_full_spec(root, spec);
        let marker = crate::shared::context::clarified_marker_path(root_str, spec)
            .expect("clarified marker path resolves");
        assert!(marker.ends_with(".clarified"));

        assert_eq!(clarify_state(root_str, spec), ClarifyState::Missing);
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);

        // The legacy body — the finalize ran, and said nothing.
        std::fs::write(&marker, b"spec=epic\nvia=grill-finalize\n").unwrap();
        assert_eq!(clarify_state(root_str, spec), ClarifyState::Hollow);

        std::fs::write(
            &marker,
            crate::shared::context::clarify_marker_body(
                spec,
                "s-1",
                "2026-07-25T10:00:00.000Z",
                &["Payable".to_string()],
                "",
            ),
        )
        .unwrap();
        assert_eq!(clarify_state(root_str, spec), ClarifyState::Recorded);
    }

    #[test]
    fn light_spec_is_not_clarify_gated() {
        // A Light spec: `spec_is_full` is false, so `run` skips the clarify gate
        // entirely — approval never requires `.clarified` for a Light / task spec.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let sp = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec("small")
            .unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.meta_json_path(),
            r#"{"scope":"light","stage":"Plan","outcome":"Active"}"#,
        )
        .unwrap();
        assert!(
            !spec_is_full(root_str, "small"),
            "a Light spec is not full → the clarify gate is skipped"
        );
    }

    // -----------------------------------------------------------------------
    // Combined gate — one refusal names EVERY unmet marker (clarify + approval)
    // -----------------------------------------------------------------------

    #[test]
    fn combined_refusal_lists_all_missing_gates() {
        // Both markers absent → the single refusal names BOTH, each with its own
        // minting path, instead of exiting on the first miss and hiding the second.
        let msg = unmet_gate_message("epic", ClarifyState::Missing, true)
            .expect("both missing → a refusal");
        assert!(msg.contains(".clarified"), "names the clarify marker: {msg}");
        assert!(
            msg.contains("grill-capture --finalize --spec epic"),
            "names the clarify minting path with the spec: {msg}"
        );
        assert!(msg.contains(".approved-by-user"), "names the approval marker: {msg}");
        assert!(
            msg.contains("ExitPlanMode") && msg.contains("AskUserQuestion"),
            "names the approval minting path: {msg}"
        );
        // Strict refuses (Block ⇒ exit≠0) when a marker is missing.
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    #[test]
    fn clarify_only_missing_refuses_with_single_requirement() {
        // Clarify absent, approval present → refuse, but name ONLY the clarify
        // requirement (no stray approval line).
        let msg = unmet_gate_message("epic", ClarifyState::Missing, false)
            .expect("clarify missing → a refusal");
        assert!(msg.contains(".clarified"), "names the clarify marker: {msg}");
        assert!(
            msg.contains("grill-capture --finalize --spec epic"),
            "names the clarify minting path: {msg}"
        );
        assert!(
            !msg.contains(".approved-by-user"),
            "must NOT name the met approval marker: {msg}"
        );
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    #[test]
    fn approval_only_missing_refuses_with_single_requirement() {
        // Approval absent, clarify present (or not a Full spec) → refuse, but name
        // ONLY the approval requirement.
        let msg = unmet_gate_message("epic", ClarifyState::Recorded, true)
            .expect("approval missing → a refusal");
        assert!(msg.contains(".approved-by-user"), "names the approval marker: {msg}");
        assert!(
            msg.contains("ExitPlanMode") && msg.contains("AskUserQuestion"),
            "names the approval minting path: {msg}"
        );
        assert!(
            !msg.contains(".clarified"),
            "must NOT name the met clarify marker: {msg}"
        );
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    #[test]
    fn both_present_approves() {
        // Neither precondition unmet → no refusal message, and strict proceeds.
        assert_eq!(unmet_gate_message("epic", ClarifyState::Recorded, false), None);
        // A Light spec skips clarify entirely — same silence.
        assert_eq!(unmet_gate_message("small", ClarifyState::NotGated, false), None);
        assert_eq!(approval_gate(ApprovalMode::Strict, true), ApprovalGate::Proceed);
    }

    // -----------------------------------------------------------------------
    // The marker must CARRY something — a hollow `.clarified` refuses
    // -----------------------------------------------------------------------

    /// The input for which the gate now says no: a marker holding only the spec
    /// name. Approval reports `ok:false` with a reason naming the remedy, and —
    /// the load-bearing half — NOT ONE approval event is emitted.
    #[test]
    fn approve_refuses_a_marker_that_recorded_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        seed_full_spec(root, spec);

        // The user really approved — only the clarification is hollow, so the
        // refusal can come from nothing else.
        std::fs::write(
            crate::shared::context::approval_marker_path(root_str, spec).unwrap(),
            crate::shared::context::marker_body(
                spec,
                "ExitPlanMode",
                "s-1",
                "2026-07-25T10:00:00.000Z",
            ),
        )
        .unwrap();
        let clarified = crate::shared::context::clarified_marker_path(root_str, spec).unwrap();
        std::fs::write(&clarified, b"spec=epic\n").unwrap();

        let opts = ApproveSpecOpts {
            spec: spec.to_string(),
            wave_plan: true,
            resume: false,
        };
        // A recorder in place of the real emitter — `RefCell` so the log can be
        // inspected between the two `approve_at` calls below.
        let emitted: std::cell::RefCell<Vec<(String, Value)>> = std::cell::RefCell::new(Vec::new());
        let mut record =
            |kind: &str, payload: Value| emitted.borrow_mut().push((kind.to_string(), payload));

        let refused = approve_at(root_str, &opts, ApprovalMode::Strict, &mut record)
            .err()
            .expect("a marker that recorded nothing must refuse the approval");
        assert!(refused.exit_nonzero, "a gate refusal exits non-zero");
        assert!(
            refused.error.contains(".clarified") && refused.error.contains("recorded NOTHING"),
            "the refusal names the hollow marker: {}",
            refused.error
        );
        assert!(
            refused.error.contains("--term") && refused.error.contains("--reason"),
            "the refusal names the remedy — run the grill, or state a reason: {}",
            refused.error
        );
        assert!(
            emitted.borrow().is_empty(),
            "the approval events must NOT be emitted: {:?}",
            emitted.borrow()
        );

        // The report the caller sees.
        let err = ApproveError {
            ok: false,
            error: refused.error,
        };
        let json: Value = serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(json["ok"], false);

        // Recording what was settled is what unblocks it — same spec, same
        // markers, one honest line added.
        std::fs::write(
            &clarified,
            crate::shared::context::clarify_marker_body(
                spec,
                "s-1",
                "2026-07-25T10:00:00.000Z",
                &[],
                "the glossary already defines every matched term",
            ),
        )
        .unwrap();
        let report = approve_at(root_str, &opts, ApprovalMode::Strict, &mut record)
            .expect("a recorded clarification approves");
        assert!(report.approved);
        assert_eq!(
            emitted.borrow().len(),
            2,
            "plan + approved: {:?}",
            emitted.borrow()
        );
    }

    // -----------------------------------------------------------------------
    // Provenance echo — the report surfaces the door + instant the marker holds
    // -----------------------------------------------------------------------

    #[test]
    fn approve_spec_echoes_provenance() {
        // A marker written by the shared body writer reads back as the door and
        // the instant, and the report serialises both under stable keys.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "epic";
        std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
        let marker =
            crate::shared::context::approval_marker_path(root.to_str().unwrap(), spec).unwrap();
        std::fs::write(
            &marker,
            crate::shared::context::marker_body(
                spec,
                "AskUserQuestion",
                "s-1",
                "2026-07-24T10:00:00.000Z",
            ),
        )
        .unwrap();

        let prov = approval_provenance(root.to_str().unwrap(), spec).expect("provenance reads back");
        assert_eq!(prov.via, "AskUserQuestion");
        assert_eq!(prov.at, "2026-07-24T10:00:00.000Z");

        let report = ApproveReport {
            ok: true,
            spec: spec.to_string(),
            approved: true,
            resumed: false,
            approved_via: Some(prov.via.clone()),
            approved_at: Some(prov.at.clone()),
        };
        let json: Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(json["approvedVia"], "AskUserQuestion");
        assert_eq!(json["approvedAt"], "2026-07-24T10:00:00.000Z");
    }

    #[test]
    fn unreadable_marker_body_still_approves() {
        // The marker EXISTS (so the gate proceeds) but its body has no `via=`
        // line: the provenance read degrades to `None` and the echoed keys are
        // omitted — never a rejection, and never a null key.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "epic";
        std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
        let marker =
            crate::shared::context::approval_marker_path(root.to_str().unwrap(), spec).unwrap();
        std::fs::write(&marker, b"garbage with no via line\n").unwrap();

        // Existence still governs the gate.
        assert!(marker.exists());
        assert_eq!(
            approval_gate(ApprovalMode::Strict, marker.exists()),
            ApprovalGate::Proceed
        );
        // But the provenance read degrades to nothing.
        assert!(approval_provenance(root.to_str().unwrap(), spec).is_none());

        // The report a degraded read produces: approved, with the echo keys
        // simply absent (skip_serializing_if), not present-and-null.
        let report = ApproveReport {
            ok: true,
            spec: spec.to_string(),
            approved: true,
            resumed: false,
            approved_via: None,
            approved_at: None,
        };
        let json: Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(json["approved"], true);
        assert!(json.get("approvedVia").is_none(), "no null key: {json}");
        assert!(json.get("approvedAt").is_none(), "no null key: {json}");
    }
}
