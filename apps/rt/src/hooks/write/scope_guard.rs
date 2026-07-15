//! `scope_guard` — the Full-scope approval hard-gate (D5).
//!
//! ## Why this exists
//!
//! The observed defect: `/feature` Full scope amended **production code** right
//! after asking a question, never passing the `/spec` approval gate. A gate
//! keyed on `stage == Execute` is useless against that — the orchestrator can
//! self-emit Execute and walk straight past it. So this gate trusts an
//! **explicit approval event** instead: a `pipeline.status` event with
//! `to == "approved"`, which **only** the `/spec` approve flow emits.
//!
//! ## What it denies
//!
//! A `PreToolUse(Write|Edit)` of a **production file** when the active spec is
//! `scope = full`, `stage = Plan`, and carries **no approval event**. That is
//! the exact window where code must not be touched yet.
//!
//! ## What it ALWAYS allows (false-positive guards — Concern, high)
//!
//! - Editing the spec's own `spec.md` / `wave-plan.md`, and any `.claude/`
//!   artifact (the PLAN phase legitimately writes these).
//! - Light / Touch / non-`full` specs (no PLAN approval gate applies).
//! - A spec already past PLAN, or one with an approval event present (the
//!   resume-after-approve path).
//! - Task / Agent dispatches — the PLAN phase of a Full spec is itself a Task
//!   dispatch, so blocking those would trap the legitimate PLAN agent. The
//!   subagent's own production `Write`/`Edit` calls still fire this same gate,
//!   so the protection is not lost.
//!
//! ## Fail-open
//!
//! Any sensor error — no active spec, unreadable `meta.json`, unreadable events
//! dir — degrades to [`Verdict::Allow`]. Only a clear, positive signal (full +
//! Plan + no approval + a production file) denies. A hook bug must never block.

use mustard_core::platform::error::Error;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use serde_json::Value;
use std::path::Path;

use crate::shared::context::current_spec;

/// The Full-scope approval hard-gate module.
pub struct ScopeGuard;

/// Path prefixes that are spec artifacts / infrastructure — NEVER production
/// code. An edit under any of these during PLAN is legitimate (the spec.md, the
/// wave plan, the harness config). Mirrors `boundary_gate::META_PREFIXES` plus the
/// forward-slash-normalised spec dir.
const ARTIFACT_PREFIXES: &[&str] = &[".claude/", "dist/", "node_modules/", ".git/", "target/"];

/// `true` when `rel` (forward-slash, relative-to-cwd) is a spec artifact or
/// infrastructure path — i.e. NOT a production file. An empty/`../` path is
/// also treated as non-production (we cannot attribute it to production code,
/// so fail-open to allow).
fn is_artifact_path(rel: &str) -> bool {
    if rel.is_empty() {
        return true;
    }
    ARTIFACT_PREFIXES.iter().any(|p| rel.starts_with(p))
}

/// Resolve the `file_path` of a Write/Edit invocation (accepts the legacy
/// `path` key), forward-slash normalised.
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(Value::as_str)
        .map(|s| s.replace('\\', "/"))
}

/// Compute `file_path` relative to `cwd`, forward-slash normalised. Returns
/// `None` when the path escapes `cwd` — the caller treats that as non-
/// production (fail-open to allow). A relative input is taken as-is.
fn relative_to_cwd(cwd: &str, file_path: &str) -> Option<String> {
    let cwd_norm = cwd.replace('\\', "/");
    let fp = file_path.replace('\\', "/");
    let is_absolute = fp.starts_with('/')
        || (fp.len() >= 3
            && fp.as_bytes()[0].is_ascii_alphabetic()
            && fp.as_bytes()[1] == b':'
            && fp.as_bytes()[2] == b'/');
    if !is_absolute {
        return Some(fp);
    }
    let prefix = format!("{}/", cwd_norm.trim_end_matches('/'));
    fp.strip_prefix(&prefix).map(str::to_string)
}

/// `true` when the spec's `meta.json#scope` declares a Full-scope spec. The
/// scope string is `"full"` (spec-draft) or `"full (wave plan)"` (wave-scaffold)
/// — both start with `full` after a case-insensitive trim.
fn meta_scope_is_full(scope: Option<&str>) -> bool {
    scope
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false)
}

/// `true` when the spec's `meta.json#stage` is `Plan` — the only stage where
/// the approval gate applies. Past Plan (Execute/QaReview/Close) the gate is a
/// no-op (the approve already happened, or the spec is a Light inline flow that
/// never had a Plan gate).
fn meta_stage_is_plan(stage: Option<&str>) -> bool {
    stage.map(|s| s.trim().eq_ignore_ascii_case("Plan")).unwrap_or(false)
}

/// `true` when the spec's per-spec NDJSON event log carries an approval event —
/// a `pipeline.status` with `to == "approved"`. This is the ONLY signal that
/// counts as approval (D5): it is emitted exclusively by the `/spec` approve
/// flow, so a `/feature` self-emitting `pipeline.stage: execute` cannot forge
/// it. Fail-open: an absent / unreadable events dir returns `false` (no
/// approval seen) — combined with the other positive conditions, the gate then
/// denies, which is the safe direction for a not-yet-approved Full spec.
fn approval_event_present(cwd: &Path, spec: &str) -> bool {
    let Some(events_dir) = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
    else {
        return false;
    };
    let events = read_harness_events_from_ndjson_dir(&events_dir);
    events.iter().any(|ev| {
        ev.event == "pipeline.status"
            && ev.payload.get("to").and_then(Value::as_str) == Some("approved")
    })
}

/// The core decision for a Write/Edit. Returns `Verdict::Deny` ONLY when every
/// positive condition holds: an active Full spec, in stage Plan, with no
/// approval event, editing a production file. Anything else allows.
fn evaluate_write(input: &HookInput, cwd: &str) -> Verdict {
    // Resolve the production-file path; a missing/`../`/escaping path is not
    // attributable to production code → allow (fail-open).
    let Some(file_path) = file_path_of(input) else {
        return Verdict::Allow;
    };
    let Some(rel) = relative_to_cwd(cwd, &file_path) else {
        return Verdict::Allow;
    };
    // Artifact / `.claude/` / spec.md edits are ALWAYS allowed during PLAN.
    if is_artifact_path(&rel) {
        return Verdict::Allow;
    }

    // Resolve the active spec; without one there is no gate to apply.
    let Some(spec) = current_spec(cwd) else {
        return Verdict::Allow;
    };
    let cwd_path = Path::new(cwd);
    let meta = ClaudePaths::for_project(cwd_path)
        .and_then(|p| p.for_spec(&spec))
        .ok()
        .and_then(|sp| mustard_core::read_meta(&sp.dir().join("meta.json")));
    let Some(meta) = meta else {
        // No sidecar → cannot prove this is a Full/Plan spec → allow (fail-open).
        return Verdict::Allow;
    };
    if !meta_scope_is_full(meta.scope.as_deref()) {
        return Verdict::Allow; // Light / Touch / non-full → no gate.
    }
    if !meta_stage_is_plan(meta.stage.as_deref()) {
        return Verdict::Allow; // Past PLAN → already approved or never gated.
    }
    if approval_event_present(cwd_path, &spec) {
        return Verdict::Allow; // Resume-after-approve path.
    }

    // Full + Plan + no approval + a production file → the exact deny window.
    Verdict::Deny {
        reason: format!(
            "[scope-guard] spec '{spec}' is Full scope still in PLAN with no \
             approval — editing production file '{rel}' is blocked. Run `/spec` \
             to approve the plan first, then EXECUTE. (Editing the spec's own \
             spec.md / .claude artifacts during PLAN is always allowed.)"
        ),
    }
}

impl Check for ScopeGuard {
    /// Gate a `PreToolUse(Write|Edit)` of a production file for an unapproved
    /// Full-scope spec. Task/Agent dispatches pass through (the PLAN phase is
    /// itself a Task; the subagent's own Write/Edit calls re-enter this gate).
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        match input.tool_name.as_deref() {
            Some("Write" | "Edit") => {
                let cwd = ctx.project_dir_or_cwd(input);
                Ok(evaluate_write(input, &cwd))
            }
            // Task / Agent: allow — blocking would trap the legitimate Full-scope
            // PLAN dispatch (which runs before approval). The production-file
            // protection lives on the Write/Edit path the subagent itself hits.
            _ => Ok(Verdict::Allow),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    /// Seed a spec dir with a `meta.json` (scope/stage) under `cwd`.
    fn seed_spec(cwd: &Path, spec: &str, scope: &str, stage: &str) {
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec(spec).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            json!({ "scope": scope, "stage": stage, "outcome": "Active" }).to_string(),
        )
        .unwrap();
        // Seed a pipeline-state file so `current_spec` resolves this spec via
        // its FS fallback (no env mutation, which is `unsafe` under 2024).
        let states = ClaudePaths::for_project(cwd).unwrap().pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join(format!("{spec}.json")), "{}").unwrap();
    }

    /// Emit a `pipeline.status: approved` event into the spec's NDJSON log.
    fn seed_approval(cwd: &Path, spec: &str) {
        let ev = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-06-02T00:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Cli, id: Some("spec".to_string()), actor_type: None },
            event: "pipeline.status".to_string(),
            payload: json!({ "to": "approved" }),
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(cwd.to_str().unwrap(), &ev);
    }

    fn write_input(cwd: &Path, file_path: &str) -> HookInput {
        HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_path, "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            ..HookInput::default()
        }
    }

    fn ctx_for(cwd: &Path) -> Ctx {
        Ctx {
            project_dir: cwd.to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        }
    }

    /// DENY: Full + Plan + no approval + a production file.
    #[test]
    fn denies_production_edit_for_unapproved_full_plan() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "epic", "full (wave plan)", "Plan");
        let input = write_input(dir.path(), "src/main.rs");
        match ScopeGuard.evaluate(&input, &ctx_for(dir.path())).unwrap() {
            Verdict::Deny { reason } => assert!(reason.contains("scope-guard")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    /// ALLOW: the spec's own `spec.md` (and any `.claude/` artifact) during PLAN.
    #[test]
    fn allows_spec_md_and_claude_artifacts_during_plan() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "epic", "full", "Plan");
        // The spec.md lives under .claude/spec/epic/ → artifact prefix.
        let spec_md = ClaudePaths::for_project(dir.path())
            .unwrap()
            .for_spec("epic")
            .unwrap()
            .spec_md_path()
            .to_string_lossy()
            .into_owned();
        let input = write_input(dir.path(), &spec_md);
        assert_eq!(
            ScopeGuard.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
        // An arbitrary .claude/ artifact too.
        let claude_file =
            format!("{}/.claude/settings.json", dir.path().to_string_lossy());
        let input2 = write_input(dir.path(), &claude_file);
        assert_eq!(
            ScopeGuard.evaluate(&input2, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }

    /// ALLOW: the resume-after-approve path — an approval event is present.
    #[test]
    fn allows_production_edit_after_approval() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "epic", "full", "Plan");
        seed_approval(dir.path(), "epic");
        let input = write_input(dir.path(), "src/main.rs");
        assert_eq!(
            ScopeGuard.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }

    /// ALLOW: a Light spec is never gated, even editing production at Plan.
    #[test]
    fn allows_light_scope_inline_edit() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "small", "light", "Plan");
        let input = write_input(dir.path(), "src/main.rs");
        assert_eq!(
            ScopeGuard.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }

    /// ALLOW: a Full spec past PLAN (already in Execute) — approval happened.
    #[test]
    fn allows_full_spec_past_plan() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "epic", "full", "Execute");
        let input = write_input(dir.path(), "src/main.rs");
        assert_eq!(
            ScopeGuard.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }

    /// ALLOW (fail-open): no active spec at all.
    #[test]
    fn allows_when_no_active_spec() {
        let dir = tempdir().unwrap();
        let input = write_input(dir.path(), "src/main.rs");
        assert_eq!(
            ScopeGuard.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }

    /// ALLOW: a Task dispatch is never blocked (the PLAN phase is a Task).
    #[test]
    fn allows_task_dispatch() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "epic", "full", "Plan");
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "prompt": "do plan work" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        assert_eq!(
            ScopeGuard.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }
}
