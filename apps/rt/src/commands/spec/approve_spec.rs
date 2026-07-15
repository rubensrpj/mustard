//! `mustard-rt run approve-spec` — deterministic spec-approval event sequence.
//!
//! Replaces the hand-assembled `emit-pipeline` sequence the approve-flow SKILL
//! used to make the LLM run by hand (`approve-only-flow.md` step 5: emit
//! `pipeline.stage Plan` then `pipeline.status from:draft,to:approved`; step 4:
//! patch the wave-1 `meta.json` for dispatch). The orchestrator now relays a
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
#[derive(Debug, Serialize)]
struct ApproveReport {
    ok: bool,
    spec: String,
    approved: bool,
    resumed: bool,
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
// T5 — the approval gate.
//
// `approve-spec` may emit the `draft→approved` signal ONLY once a real user has
// approved the plan. The `approval_marker_observer` records that human answer as
// `<spec>/.approved-by-user` — a marker the model cannot author, since it is born
// from the user's `AskUserQuestion` `tool_response`. Without the marker, `strict`
// refuses: a gate the gated could open by running this very command is not a
// gate. Mode reads exactly like the `MUSTARD_*_GATE_MODE` close-gate family.
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

/// Didactic refusal surfaced as the report `error` when strict mode finds no
/// user-approval marker. The flow relays `{ok:false,error}` straight to the user.
const APPROVAL_REQUIRED_MSG: &str = "approval must come from the user — present \
the plan in plan mode and let the user accept it (ExitPlanMode), which records \
the .approved-by-user marker; when plan mode is unavailable, ask via \
AskUserQuestion (fallback — the user's own answer records the same marker). \
approve-spec will not self-approve a Full plan (that is what the field \
incident did). To temporarily relax, set MUSTARD_APPROVAL_MODE=warn or off.";

/// CLI entry — `mustard-rt run approve-spec --spec <name> [--wave-plan] [--resume]`.
///
/// Emits the approval sequence by delegating each step to the canonical
/// [`crate::commands::event::emit_pipeline::run`] (process-global cwd, like
/// `wave_complete_observer`). Prints the JSON report to stdout. Exit code is
/// always 0 — the only failure (empty spec name) is reported in the JSON.
pub fn run(opts: ApproveSpecOpts) {
    if opts.spec.trim().is_empty() {
        let err = ApproveError {
            ok: false,
            error: "empty spec name".to_string(),
        };
        println!(
            "{}",
            serde_json::to_string(&err).unwrap_or_else(|_| "{\"ok\":false}".to_string())
        );
        return;
    }

    // T5 approval gate — refuse (strict) to emit the approval signal without a
    // recorded HUMAN approval. The `<spec>/.approved-by-user` marker is written
    // ONLY by `approval_marker_observer` from the user's real AskUserQuestion
    // answer, so a background job (no user, no answer, no marker) halts cleanly
    // here and the Full spec stays in PLAN instead of auto-approving.
    let mode = resolve_approval_mode();
    if mode != ApprovalMode::Off {
        let cwd = crate::shared::context::cwd();
        let marker_present = crate::shared::context::approval_marker_path(&cwd, &opts.spec)
            .map(|p| p.exists())
            .unwrap_or(false);
        match approval_gate(mode, marker_present) {
            ApprovalGate::Block => {
                let err = ApproveError {
                    ok: false,
                    error: APPROVAL_REQUIRED_MSG.to_string(),
                };
                println!(
                    "{}",
                    serde_json::to_string(&err).unwrap_or_else(|_| "{\"ok\":false}".to_string())
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
                std::process::exit(1);
            }
            ApprovalGate::Warn => {
                eprintln!(
                    "[approval] proceeding without a user-approval marker for spec '{}' \
                     (MUSTARD_APPROVAL_MODE=warn) — the plan was not confirmed by the user; \
                     strict mode would refuse this.",
                    opts.spec
                );
            }
            ApprovalGate::Proceed => {}
        }
    }

    for (kind, payload) in approval_sequence(opts.wave_plan, opts.resume) {
        // Reuse the canonical emitter module-qualified — no subprocess, no
        // duplicated NDJSON logic, no facade. It routes the event, fans out the
        // canonical alias, and patches the (wave-1 when `wave:1`) `meta.json`.
        crate::commands::event::emit_pipeline::run(
            crate::commands::event::emit_pipeline::EmitPipelineOpts {
                kind: kind.to_string(),
                spec: opts.spec.clone(),
                payload: Some(payload.to_string()),
                allow_no_qa: false,
                intent: None,
                base: None,
            },
        );
    }

    let report = ApproveReport {
        ok: true,
        spec: opts.spec.clone(),
        approved: true,
        resumed: opts.resume,
    };
    println!(
        "{}",
        serde_json::to_string(&report).unwrap_or_else(|_| "{\"ok\":true}".to_string())
    );
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
}
