//! `rewave_observer` — auto re-wave on the first EXECUTE write.
//!
//! ## Automatic by kind — re-wave is *structural*, so it runs on its own
//!
//! The re-wave signal has always been Rust
//! ([`crate::commands::wave::exec_rewave_check`] decides decomposition by
//! `layerCount >= 2`), but firing it used to require the SKILL to call the
//! subcommand. This observer closes that gap: on a `PreToolUse(Write|Edit)` of
//! a spec that is **in EXECUTE** and **not yet decomposed**, it invokes
//! [`crate::commands::wave::exec_rewave_check::decompose_if_signaled`] directly
//! (module-qualified — no subprocess, no facade). The decomposition writes the
//! `wave-plan.md` + per-wave `spec.md` structure exactly as the manual
//! subcommand did.
//!
//! ## Idempotency
//!
//! Two layers, both deterministic:
//!
//! 1. The trigger is **per-spec**: the observer only acts when the state of
//!    the current spec, in its `spec.ndjson`, is in the `running` phase and no
//!    `wave-plan.md` exists yet — so a second write after decomposition is a
//!    no-op (the plan now exists).
//! 2. `decompose_if_signaled` itself re-checks the `wave-plan.md` guard and
//!    the user's refusal of the waves, so even a racing double-fire decomposes
//!    at most once (`{ action: "skip", reason: "already-decomposed" }`), and a
//!    spec the user joined into one is never split again.
//!
//! ## Role — observer, fail-open, NEVER denies
//!
//! Pure [`Observer`]: it returns `()` and is structurally incapable of
//! blocking a write. Every IO step degrades to a no-op. The `MUSTARD_REWAVE_OBSERVER_MODE`
//! env var gates it: `off` disables it entirely; any other value (default) is
//! `on`. There is no `deny`/`strict` mode by design — re-wave is advisory
//! restructuring, never a gate.

use crate::shared::events::economy;
use crate::shared::spec_state::DiskSpecState;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::domain::model::event::ActorKind;
use mustard_core::domain::spec_state::SpecState as _;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The auto re-wave observer.
pub struct RewaveObserver;

/// Whether the observer is enabled. `off` (case-insensitive) disables it; any
/// other value — including unset — is `on`. There is deliberately no
/// `deny`/`strict` mode (this is advisory restructuring, never a gate).
fn is_off() -> bool {
    std::env::var("MUSTARD_REWAVE_OBSERVER_MODE")
        .unwrap_or_default()
        .eq_ignore_ascii_case("off")
}

/// Resolve the current spec's `spec.md` path when the spec is **in execution**
/// (the `running` phase of its state) and **not yet decomposed** (no
/// `wave-plan.md`). Returns `None` (skip) otherwise.
///
/// The spec comes from the one ladder, for the session of the write. This is
/// the pure trigger predicate, separated from the side-effecting
/// [`Observer::observe`] so it is unit-testable without invoking the
/// decomposition. Every step fails open to `None`.
fn target_spec_md(cwd: &str, session: Option<&str>) -> Option<PathBuf> {
    let spec = crate::shared::spec_state::active_spec(cwd, session)?;
    // Only act in execution — the phase exec-rewave-check is meant to re-evaluate.
    let state = DiskSpecState::new(Path::new(cwd)).state(&spec)?;
    if state.phase != Some("running") {
        return None;
    }
    let sp = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(&spec))
        .ok()?;
    // Idempotency layer 1: already decomposed → skip (the plan exists).
    if sp.dir().join("wave-plan.md").exists() {
        return None;
    }
    let spec_md = sp.spec_md_path();
    if !spec_md.exists() {
        return None;
    }
    Some(spec_md)
}

impl Observer for RewaveObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if is_off() {
            return;
        }
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return;
        }
        if !matches!(input.tool_name.as_deref(), Some("Write" | "Edit")) {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        // Skip writes that target nothing on disk (defensive — Write/Edit always
        // carry a path, but a malformed payload must not panic).
        if input.file_path().is_none() {
            return;
        }
        let Some(spec_md) = target_spec_md(&cwd, input.session_id.as_deref()) else {
            return;
        };
        // Decompose in-process (idempotency layer 2 lives inside the call).
        let result = crate::commands::wave::exec_rewave_check::decompose_if_signaled(&spec_md);
        let action = result.get("action").and_then(Value::as_str).unwrap_or("skip");
        // Surface the structural restructuring to the user via the observer's
        // stderr channel (same mechanism `delegation_advisory` uses). Re-wave
        // rewrites spec.md silently otherwise — in a tool that sells itself as
        // deterministic, an unannounced archive of the user's spec reads as data
        // loss. Advisory only: `eprintln!` is a pure side-effect, it can never
        // abort the write (the observer stays fail-safe).
        if action == "decomposed" {
            let total = result.get("totalWaves").and_then(Value::as_i64).unwrap_or(0);
            eprintln!(
                "[rewave] Re-wave automático: spec.md foi arquivado como spec.original.md \
                 e os critérios globais migraram para wave-plan.md (decomposição em {total} \
                 waves na entrada do EXECUTE)."
            );
        }
        economy::emit(
            &cwd,
            ActorKind::Hook,
            "rewave_observer",
            "pipeline.economy.operation.invoked",
            None,
            json!({ "operation": "rewave_observer.decompose", "action": action, "duration_ms": 0, "tokens_used": 0 }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    /// Record the phase `phase` in the state of the spec's `spec.ndjson`.
    fn record_phase(project: &Path, spec: &str, phase: &str) {
        let sp = ClaudePaths::for_project(project).unwrap().for_spec(spec).unwrap();
        let state = json!({ "phase": phase });
        store::write(&sp.dir().join("spec.ndjson"), "state", state.as_object().cloned().unwrap(), &[]).unwrap();
    }

    /// Build a project skeleton with a spec dir + spec.md, and (optionally)
    /// record its phase in the state of its `spec.ndjson`.
    fn make_spec(project: &Path, spec: &str, files_section: &str, phase: Option<&str>) -> PathBuf {
        std::fs::write(project.join("mustard.json"), b"{}").unwrap();
        let sp = ClaudePaths::for_project(project).unwrap().for_spec(spec).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        let body = format!("# Spec\n\n## Summary\nx\n\n## Files\n{files_section}\n\n## Tasks\n- do it\n");
        std::fs::write(sp.spec_md_path(), body).unwrap();
        if let Some(p) = phase {
            record_phase(project, spec, p);
        }
        sp.spec_md_path()
    }

    /// O gatilho é o estado da spec atual: em plano, nada; em execução, o
    /// `spec.md`; já decomposta, nada de novo.
    #[test]
    fn the_trigger_is_the_running_state_of_the_current_spec() {
        // An inherited override answers first; the branch rung is what is
        // under test.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let project = dir.path();
        let cwd = project.to_str().unwrap();
        let spec_md = make_spec(project, "specA", "- src/a.ts\n- src/b.ts", Some("plan"));
        crate::shared::spec_state::stand_on_spec_branch(project, "specA");
        assert!(target_spec_md(cwd, None).is_none(), "a plan is not the execution");

        record_phase(project, "specA", "running");
        assert_eq!(target_spec_md(cwd, None), Some(spec_md.clone()), "in execution, the spec is the target");

        std::fs::write(spec_md.with_file_name("wave-plan.md"), "# Wave Plan\n").unwrap();
        assert!(target_spec_md(cwd, None).is_none(), "already decomposed");
    }

    #[test]
    fn decompose_is_idempotent_second_call_skips() {
        // A multi-layer EXECUTE spec decomposes once; the second call no-ops
        // because `wave-plan.md` now exists (idempotency layer 2).
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec_md = make_spec(
            project,
            "specB",
            "- src/domain/user.rs\n- src/api/handler.rs",
            Some("running"),
        );
        let first = crate::commands::wave::exec_rewave_check::decompose_if_signaled(&spec_md);
        let first_action = first.get("action").and_then(Value::as_str).unwrap_or("");
        // Either it decomposed (multi-layer) or kept-single (DAG had no depth);
        // both are valid first-call outcomes. If it decomposed, the second call
        // must report already-decomposed.
        if first_action == "decomposed" {
            let sp = ClaudePaths::for_project(project).unwrap().for_spec("specB").unwrap();
            // After decompose the original spec.md is renamed to spec.original.md;
            // a second call on the now-missing spec.md still hits the wave-plan
            // guard first (skip).
            let second = crate::commands::wave::exec_rewave_check::decompose_if_signaled(&spec_md);
            assert_eq!(
                second.get("action").and_then(Value::as_str),
                Some("skip"),
                "second call must skip; wave-plan.md present at {:?}",
                sp.dir().join("wave-plan.md")
            );
        }
    }

    #[test]
    fn observer_never_returns_a_verdict() {
        // Structural: RewaveObserver is an Observer (returns `()`), so it cannot
        // deny. This test exists to document the contract — calling observe on a
        // bare project is a fail-open no-op.
        let dir = tempdir().unwrap();
        let ctx = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::PreToolUse));
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": dir.path().join("x.rs").to_string_lossy(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        RewaveObserver.observe(&input, &ctx); // must not panic
    }
}
