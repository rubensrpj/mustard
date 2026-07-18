//! `rewave_observer` — auto re-wave on the first EXECUTE write (F4-c item 1).
//!
//! ## Decision 6 — auto-abertura por tipo (re-wave is *structural* → automatic)
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
//! 1. The trigger is **per-spec**: the observer only acts when the spec's
//!    latest `pipeline.phase` is `EXECUTE` (via
//!    [`crate::commands::event::emit_phase::last_phase_for_spec`]) and no
//!    `wave-plan.md` exists yet — so a second write after decomposition is a
//!    no-op (the plan now exists).
//! 2. `decompose_if_signaled` itself re-checks the `wave-plan.md` /
//!    pipeline-state guards, so even a racing double-fire decomposes at most
//!    once (`{ action: "skip", reason: "already-decomposed" }`).
//!
//! ## Role — observer, fail-open, NEVER denies
//!
//! Pure [`Observer`]: it returns `()` and is structurally incapable of
//! blocking a write. Every IO step degrades to a no-op. The `MUSTARD_REWAVE_OBSERVER_MODE`
//! env var gates it: `off` disables it entirely; any other value (default) is
//! `on`. There is no `deny`/`strict` mode by design — re-wave is advisory
//! restructuring, never a gate.

use crate::shared::events::economy;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::domain::model::event::ActorKind;
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

/// Resolve the active spec's `spec.md` path when the spec is **in EXECUTE** and
/// **not yet decomposed** (no `wave-plan.md`). Returns `None` (skip) otherwise.
///
/// This is the pure trigger predicate, separated from the side-effecting
/// [`Observer::observe`] so it is unit-testable without invoking the
/// decomposition. Every step fails open to `None`.
fn target_spec_md(cwd: &str) -> Option<PathBuf> {
    let spec = crate::shared::context::current_spec(cwd)?;
    if spec.is_empty() {
        return None;
    }
    // Only act in EXECUTE — the phase exec-rewave-check is meant to re-evaluate.
    let phase = crate::commands::event::emit_phase::last_phase_for_spec(cwd, &spec)?;
    if !phase.eq_ignore_ascii_case("EXECUTE") {
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
        let Some(spec_md) = target_spec_md(&cwd) else {
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
    use crate::shared::events::writer_ndjson::write_event;
    use tempfile::tempdir;

    /// Build a project skeleton with a spec dir + spec.md, and (optionally)
    /// drive its phase via a `pipeline.phase` event into the per-spec NDJSON.
    fn make_spec(project: &Path, spec: &str, files_section: &str, phase: Option<&str>) -> PathBuf {
        std::fs::write(project.join("mustard.json"), b"{}").unwrap();
        let sp = ClaudePaths::for_project(project).unwrap().for_spec(spec).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        let body = format!("# Spec\n\n## Summary\nx\n\n## Files\n{files_section}\n\n## Tasks\n- do it\n");
        std::fs::write(sp.spec_md_path(), body).unwrap();
        if let Some(p) = phase {
            let payload = json!({ "from": null, "to": p });
            let _ = write_event(
                project, Some(spec), None, "s", "pipeline.phase", "pipeline",
                Some(0), Some("s"), Some("test"), None, &payload,
            );
        }
        sp.spec_md_path()
    }

    #[test]
    fn target_skips_when_not_execute() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        make_spec(project, "specA", "- src/a.ts\n- src/b.ts", Some("PLAN"));
        // MUSTARD_ACTIVE_SPEC not set → current_spec falls back to FS; but with
        // no pipeline-state file it returns None. Drive via the FS hint instead.
        let states = project.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("specA.json"), "{}").unwrap();
        // PLAN phase → not EXECUTE → no target.
        assert!(target_spec_md(project.to_str().unwrap()).is_none());
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
            Some("EXECUTE"),
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
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": dir.path().join("x.rs").to_string_lossy(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        RewaveObserver.observe(&input, &ctx); // must not panic
    }
}
