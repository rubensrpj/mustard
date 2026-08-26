//! `stop_gate` — the `Stop`-event QA verification gate
//! (close-the-qa-verification-loop, wave-1-gate).
//!
//! The harness fires `Stop` when the **main orchestrator** finishes a turn.
//! This `Check` closes the verification loop: when there is an active spec that
//! is **approved** and carries an **executable** acceptance criterion, it runs
//! those criteria — by EXECUTING their commands through the qa-run executor,
//! never by asking a model to read the conversation — and, if one fails, blocks
//! the stop and returns the failing criterion as the next turn's guidance. The
//! spec then drives execution until the criteria hold, with no human turn spent
//! per attempt.
//!
//! ## It executes, it does not judge
//!
//! The verdict is the AC command's real exit code, read through the SAME
//! [`crate::commands::review::qa_run`] parser + executor `/qa` runs (see
//! [`run_for_stop_gate`]). There is no second AC parser to drift, and a parity
//! test proves the gate's verdict equals qa-run's for the same spec.
//!
//! ## The loop is bounded by a counter Mustard owns
//!
//! The platform is documented to force a stop after a run of consecutive
//! blocks (signalled by `stop_hook_active`), but that protection may not be
//! implemented — relying on it alone would risk a loop that never ends. So the
//! gate carries its OWN per-spec counter of *consecutive* blocks (a marker on
//! disk, [`stop_gate_counter_path`], reset the moment the criteria pass) and
//! honours `stop_hook_active` only as a secondary signal; at the ceiling it
//! releases the stop.
//!
//! ## The belt only tightens when there is something to verify
//!
//! `Stop` fires at every turn end with no matcher, so an indiscriminate gate
//! would trap ordinary use. This one **self-restricts**: it acts only for an
//! active+approved spec with an executable criterion, never on a subagent stop
//! ([`HookInput::is_subagent`]); anything else releases silently.
//!
//! ## The text speaks the project's language
//!
//! What returns to Claude is user-facing text, so it comes from the
//! [`mustard_core::platform::i18n`] catalogue (`stopgate.*` keys) in the
//! project's language — no prose embedded in this module. The turn ceiling is a
//! documented constant, not a new configuration knob.
//!
//! ## Fail-open
//!
//! The one blocking verdict here is a deliberate `Deny` on a red criterion; the
//! dispatcher degrades any `Err` to `Allow`, and every internal IO step falls
//! back so the gate can never block on its own failure. No `unwrap`/`expect`
//! outside tests.
//!
//! ## Departures from the `rt-gate-pattern` Write-gate mold (deliberate)
//!
//! This is a `Stop`-event `Check`, not a `PreToolUse(Write|Edit)` gate, so it
//! lives under `hooks/task/` (beside the other non-Write Checks
//! `context_budget_gate` / `*_counter`) at the file path the spec fixes. It has
//! NO `MUSTARD_*_MODE` three-state cascade — the spec forbids a new
//! configuration door, so the gate is unconditional (the `scan_gate` precedent),
//! bounded instead by the documented block ceiling. And the reason is built from
//! the i18n catalogue, not `util::format_gate_message`, because the block text
//! is config-language feedback with no embedded prose.

use crate::commands::review::qa_run::{run_for_stop_gate, spec_has_executable_acs, QaRunOptions};
use crate::shared::context::{
    approval_marker_path, current_spec, project_config_cached, spec_for_session,
    stop_gate_counter_path,
};
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::{apply_tone, translate};
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::path::Path;

/// Ceiling on CONSECUTIVE Stop blocks for one spec before the gate releases.
///
/// Mirrors the platform's documented "force a stop after 8 consecutive blocks"
/// figure, but is enforced by Mustard itself: even if the platform protection is
/// inert, this counter guarantees the loop terminates. Deliberately a constant,
/// not a `MUSTARD_*_MODE` env knob (the spec forbids a new configuration door).
const STOP_GATE_MAX_CONSECUTIVE_BLOCKS: u32 = 8;

/// The `Stop`-event QA verification gate.
pub struct StopGate;

impl Check for StopGate {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // The registry routes only `Stop` here, but a `check <id>` invocation
        // can reach any module for any trigger — re-assert the event.
        if ctx.trigger != Some(Trigger::Stop) {
            return Ok(Verdict::Allow);
        }
        // Only the main session is verified; a subagent stop never blocks.
        if input.is_subagent() {
            return Ok(Verdict::Allow);
        }

        let project_dir = ctx.project_dir_or_cwd(input);

        // Self-restriction: no active+approved spec with an executable
        // criterion ⇒ release in silence.
        let Some(spec) = resolve_gated_spec(&project_dir, input) else {
            return Ok(Verdict::Allow);
        };

        // Loop guards, BEFORE running QA so a capped / already-looping stop is
        // released without paying for another run. `stop_hook_active` is the
        // platform's repeat signal (honoured when present); the own per-spec
        // counter is the guarantee that does not depend on it.
        if stop_hook_active(input) {
            return Ok(Verdict::Allow);
        }
        let blocks = read_block_count(&project_dir, &spec);
        if blocks >= STOP_GATE_MAX_CONSECUTIVE_BLOCKS {
            // Ceiling reached: 8 auto-retries did not turn the criteria green.
            // Release and DELIBERATELY do not reset — the marker persists, so the
            // gate stays quiet for this spec until it genuinely passes (the reset
            // is the `pass` arm below) or the spec closes. The spec's own rule is
            // "reset when the criteria pass", not "reset at the ceiling"; resetting
            // here would re-arm every later turn and turn a stuck spec into a
            // per-turn block storm. The fail-safe hands a stuck spec to the human.
            return Ok(Verdict::Allow);
        }

        // Verify by EXECUTING the criteria through the qa-run executor.
        // `self_invoked`: this IS the `mustard-rt` process, so an AC that
        // rebuilds it is skipped rather than deadlocking on the exe lock.
        let outcome = run_for_stop_gate(
            Path::new(&project_dir),
            &spec,
            QaRunOptions { self_invoked: true },
        );

        match outcome.overall.as_str() {
            // A red criterion blocks the stop; the reason names it.
            "fail" => {
                write_block_count(&project_dir, &spec, blocks.saturating_add(1));
                Ok(Verdict::Deny {
                    reason: compose_block_reason(&project_dir, outcome.first_failing_ac.as_deref()),
                })
            }
            // Every criterion passed: the loop closed — reset and release.
            "pass" => {
                reset_block_count(&project_dir, &spec);
                Ok(Verdict::Allow)
            }
            // `skip` / `timeout` verified nothing cleanly — never loop on them.
            _ => Ok(Verdict::Allow),
        }
    }
}

/// Resolve the spec the gate should verify: the session's bound spec (falling
/// back to the ambient active spec) that is BOTH approved AND carries an
/// executable acceptance criterion. `None` short-circuits the gate to a silent
/// allow.
fn resolve_gated_spec(project_dir: &str, input: &HookInput) -> Option<String> {
    let session = input.session_id.as_deref().unwrap_or_default();
    let spec = spec_for_session(project_dir, session).or_else(|| current_spec(project_dir))?;
    // Approved: the `.approved-by-user` marker minted only from the user's real
    // PLAN approval (an orchestrator cannot forge it). Its existence is the gate.
    let approved = approval_marker_path(project_dir, &spec).is_some_and(|p| p.exists());
    if !approved {
        return None;
    }
    // Lifecycle: the marker above is minted at the PLAN approval, which is NOT
    // the moment EXECUTE is unlocked. In the window between the two, verifying
    // is asking for something the product forbids: `ac-negative-check` has
    // already proven every criterion RED (that redness is what qualified them to
    // enter the plan at all), and `scope_guard` denies the production edit that
    // would turn them green. Blocking there has no state of the world in which
    // it passes, so the gate releases and waits for EXECUTE.
    if meta_stage_is_plan(project_dir, &spec) {
        return None;
    }
    // Executable ACs: the exact union qa-run would run (an empty union is the
    // `overall: skip` case). Reuses the qa-run predicate so the two agree.
    if !spec_has_executable_acs(Path::new(project_dir), &spec) {
        return None;
    }
    Some(spec)
}

/// `true` when the spec's `meta.json#stage` reads `Plan` — the window where the
/// plan is approved but EXECUTE has not been unlocked yet.
///
/// Mirrors the normalisation `scope_guard::meta_stage_is_plan` uses (trim +
/// case-insensitive) from the other side of the same window: that gate denies
/// the write while this one declines to verify. The two-line predicate is
/// deliberately duplicated rather than hoisted to `shared` — two copies, not the
/// three that earned [`crate::shared::gate_mode`] its own module — so this fix
/// touches one file instead of dragging a sibling gate's test battery along.
///
/// **Positive-only, and that is the whole point.** An absent, unreadable or
/// stage-less `meta.json` returns `false`, leaving the gate verifying exactly as
/// it did before this condition existed. Releasing on a MISSING signal would
/// silently disarm the gate for every spec without a sidecar — including this
/// module's own test battery, which seeds `spec.md` and no `meta.json` at all.
fn meta_stage_is_plan(project_dir: &str, spec: &str) -> bool {
    ClaudePaths::for_project(Path::new(project_dir))
        .and_then(|p| p.for_spec(spec))
        .ok()
        .and_then(|sp| mustard_core::read_meta(&sp.dir().join("meta.json")))
        .and_then(|m| m.stage)
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("Plan"))
}

/// `true` when the harness marks this stop as a repeat driven by a prior
/// Stop-hook block. Read from the lenient `raw` bag (the field is not modelled),
/// defaulting to `false` when absent — the own counter is the real guard.
fn stop_hook_active(input: &HookInput) -> bool {
    input
        .raw
        .get("stop_hook_active")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Read the current consecutive-block count for `spec` (0 when the marker is
/// absent / unreadable / non-numeric — fail-open).
fn read_block_count(project_dir: &str, spec: &str) -> u32 {
    stop_gate_counter_path(project_dir, spec)
        .and_then(|p| fs::read_to_string(&p).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

/// Persist the consecutive-block count for `spec` (best-effort; creates the
/// spec dir if needed). A write failure never affects the verdict.
fn write_block_count(project_dir: &str, spec: &str, count: u32) {
    let Some(path) = stop_gate_counter_path(project_dir, spec) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write_atomic(&path, count.to_string().as_bytes());
}

/// Reset the consecutive-block count for `spec` by removing its marker (absence
/// == zero). A missing marker is a no-op.
fn reset_block_count(project_dir: &str, spec: &str) {
    if let Some(path) = stop_gate_counter_path(project_dir, spec) {
        let _ = fs::remove_file(&path);
    }
}

/// Compose the block reason from the `stopgate.*` i18n catalogue in the
/// project's language, interpolating the failing criterion id. No prose lives
/// in this module — only the `{ac}` slot fill and the tone application.
fn compose_block_reason(project_dir: &str, failing_ac: Option<&str>) -> String {
    let i18n = project_config_cached(Path::new(project_dir)).i18n();
    let ac = failing_ac.unwrap_or_default();
    let reason = translate("stopgate.block.reason", i18n.lang).replace("{ac}", ac);
    let guidance = translate("stopgate.block.guidance", i18n.lang);
    apply_tone(&format!("{reason} {guidance}"), i18n.tone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::context::bind_session_spec;
    use tempfile::tempdir;

    /// An AC that always PASSES cross-platform (`echo` is a builtin in both
    /// `cmd.exe` and `sh`; exit 0, no `Expect:` regex).
    const AC_PASS: &str = "- **AC-1** — always green.\n  Command: `echo ok`";
    /// An AC that always FAILS cross-platform: `echo` exits 0, but the
    /// `Expect:` evidence regex misses ⇒ the criterion is downgraded to `fail`.
    const AC_FAIL: &str = "- **AC-1** — never green.\n  Command: `echo hi`\n  Expect: `NOPE_MISSING_TOKEN`";

    fn ctx(project: &Path) -> Ctx {
        Ctx {
            project_dir: project.to_string_lossy().into_owned(),
            trigger: Some(Trigger::Stop),
            workspace_root: None,
            inject_only: None,
        }
    }

    fn stop_input(session: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some(session.to_string()),
            ..HookInput::default()
        }
    }

    /// Seed `<project>/.claude/spec/{spec}/spec.md` with an `## Acceptance
    /// Criteria` section body of `ac_body`.
    fn seed_spec(project: &Path, spec: &str, ac_body: &str) {
        let spec_dir = project.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            format!("# {spec}\n\n## Acceptance Criteria\n{ac_body}\n"),
        )
        .unwrap();
    }

    /// Mint the `.approved-by-user` marker so the spec reads as approved.
    fn approve(project: &Path, spec: &str) {
        let p = approval_marker_path(project.to_str().unwrap(), spec).unwrap();
        std::fs::write(&p, b"spec=x\nvia=test\n").unwrap();
    }

    /// Bind `session` → `spec` so `resolve_gated_spec` finds it without env.
    fn bind(project: &Path, session: &str, spec: &str) {
        bind_session_spec(project.to_str().unwrap(), session, spec);
    }

    /// Seed the `meta.json` sidecar with `stage` — the shape a Full plan parked
    /// at PLAN really has on disk. Every OTHER test here deliberately omits it,
    /// which is what pins the positive-only reading in `meta_stage_is_plan`.
    fn seed_meta(project: &Path, spec: &str, stage: &str) {
        let spec_dir = project.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            format!(r#"{{"stage":"{stage}","outcome":"Active","scope":"full"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn stop_gate_releases_a_spec_still_in_plan() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        // The exact on-disk shape of an approved Full plan awaiting `/spec`: a
        // red criterion, the PLAN approval marker, and stage still `Plan`.
        seed_spec(project, "plan-spec", AC_FAIL);
        seed_meta(project, "plan-spec", "Plan");
        approve(project, "plan-spec");
        bind(project, "s-plan", "plan-spec");

        let v = StopGate.evaluate(&stop_input("s-plan"), &ctx(project)).expect("no error");
        assert!(
            !v.is_blocking(),
            "a spec still in PLAN is forbidden to turn its criteria green, so the gate must release, got {v:?}"
        );
        // And it releases WITHOUT paying for a QA run — no consecutive block is
        // recorded, so the 8-block ceiling is never spent on this window.
        assert!(
            !stop_gate_counter_path(project.to_str().unwrap(), "plan-spec")
                .unwrap()
                .exists(),
            "releasing during PLAN must not record a consecutive block"
        );
    }

    #[test]
    fn stop_gate_still_blocks_once_the_spec_leaves_plan() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        // Same spec, same red criterion — only the stage moved on. The release
        // above must be about the WINDOW, never about the sidecar's presence.
        seed_spec(project, "exec-spec", AC_FAIL);
        seed_meta(project, "exec-spec", "Execute");
        approve(project, "exec-spec");
        bind(project, "s-exec", "exec-spec");

        let v = StopGate.evaluate(&stop_input("s-exec"), &ctx(project)).expect("no error");
        assert!(v.is_blocking(), "past PLAN a red criterion still blocks, got {v:?}");
    }

    // -- AC-1 -----------------------------------------------------------------

    #[test]
    fn stop_gate_allows_when_all_acs_pass() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_spec(project, "pass-spec", AC_PASS);
        approve(project, "pass-spec");
        bind(project, "s-1", "pass-spec");

        let v = StopGate.evaluate(&stop_input("s-1"), &ctx(project)).expect("no error");
        assert!(!v.is_blocking(), "all ACs pass ⇒ the stop is released, got {v:?}");
        // Counter reset (marker absent) after a green run.
        assert!(
            !stop_gate_counter_path(project.to_str().unwrap(), "pass-spec")
                .unwrap()
                .exists(),
            "a passing run resets the consecutive-block counter"
        );
    }

    // -- AC-2 -----------------------------------------------------------------

    #[test]
    fn stop_gate_blocks_and_names_the_failing_ac() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_spec(project, "fail-spec", AC_FAIL);
        approve(project, "fail-spec");
        bind(project, "s-2", "fail-spec");

        let v = StopGate.evaluate(&stop_input("s-2"), &ctx(project)).expect("no error");
        match v {
            Verdict::Deny { reason } => {
                assert!(reason.contains("AC-1"), "the reason names the failing AC: {reason}");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
        // The consecutive-block counter advanced to 1.
        let count = std::fs::read_to_string(
            stop_gate_counter_path(project.to_str().unwrap(), "fail-spec").unwrap(),
        )
        .unwrap();
        assert_eq!(count.trim(), "1", "one block ⇒ counter at 1");
    }

    // -- AC-3 -----------------------------------------------------------------

    #[test]
    fn stop_gate_is_inert_without_an_approved_spec() {
        // (a) A bound spec with a real (failing) AC but NOT approved ⇒ inert.
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_spec(project, "unapproved", AC_FAIL);
        bind(project, "s-3a", "unapproved"); // no approve()
        let v = StopGate.evaluate(&stop_input("s-3a"), &ctx(project)).expect("no error");
        assert!(!v.is_blocking(), "an unapproved spec never blocks the stop");

        // (b) An approved spec whose `## Acceptance Criteria` has no parseable
        //     item (no executable criterion) ⇒ inert.
        let dir2 = tempdir().unwrap();
        let project2 = dir2.path();
        seed_spec(project2, "no-acs", "nothing parseable here");
        approve(project2, "no-acs");
        bind(project2, "s-3b", "no-acs");
        let v = StopGate.evaluate(&stop_input("s-3b"), &ctx(project2)).expect("no error");
        assert!(!v.is_blocking(), "an approved spec with no executable AC never blocks");
    }

    // -- AC-4 -----------------------------------------------------------------

    #[test]
    fn stop_gate_ignores_subagent_stops() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        // A setup that WOULD block for the main session.
        seed_spec(project, "sub-spec", AC_FAIL);
        approve(project, "sub-spec");
        bind(project, "s-4", "sub-spec");

        let mut input = stop_input("s-4");
        input.agent_id = Some("explore-99".to_string()); // marks a subagent stop
        let v = StopGate.evaluate(&input, &ctx(project)).expect("no error");
        assert!(!v.is_blocking(), "a subagent stop is never verified, got {v:?}");
    }

    // -- AC-5 -----------------------------------------------------------------

    #[test]
    fn stop_gate_own_counter_caps_the_loop() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_spec(project, "cap-spec", AC_FAIL); // would block, unguarded
        approve(project, "cap-spec");
        bind(project, "s-5", "cap-spec");
        let counter = stop_gate_counter_path(project.to_str().unwrap(), "cap-spec").unwrap();

        // At the ceiling ⇒ release, even though the criterion still fails.
        std::fs::write(&counter, STOP_GATE_MAX_CONSECUTIVE_BLOCKS.to_string()).unwrap();
        let v = StopGate.evaluate(&stop_input("s-5"), &ctx(project)).expect("no error");
        assert!(!v.is_blocking(), "at the block ceiling the gate releases the stop");

        // Counter back to zero, but `stop_hook_active` ⇒ release.
        std::fs::remove_file(&counter).unwrap();
        let mut input = stop_input("s-5");
        input.raw = serde_json::json!({ "stop_hook_active": true });
        let v = StopGate.evaluate(&input, &ctx(project)).expect("no error");
        assert!(!v.is_blocking(), "stop_hook_active releases the stop");

        // Control: with neither guard active, the same failing spec DOES block
        // — proving it was the guards, not the setup, that released above.
        let v = StopGate.evaluate(&stop_input("s-5"), &ctx(project)).expect("no error");
        assert!(v.is_blocking(), "unguarded, the failing spec blocks");
    }

    // -- AC-6 -----------------------------------------------------------------

    #[test]
    fn stop_gate_verdict_matches_qa_run() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        // A mix: one green + one red ⇒ overall fail.
        seed_spec(
            project,
            "parity",
            "- **AC-1** — green.\n  Command: `echo ok`\n- **AC-2** — red.\n  Command: `echo hi`\n  Expect: `NOPE_MISSING_TOKEN`",
        );
        let opts = QaRunOptions { self_invoked: true };
        let gate = run_for_stop_gate(project, "parity", opts);
        let qa = crate::commands::review::qa_run::run_qa_with_options(project, "parity", opts);
        assert_eq!(
            gate.overall, qa.overall,
            "the gate reuses qa-run's parser+executor ⇒ identical verdict"
        );
        assert_eq!(gate.overall, "fail");
        assert_eq!(gate.first_failing_ac.as_deref(), Some("AC-2"), "names the first red AC");
    }

    // -- AC-7 -----------------------------------------------------------------

    #[test]
    fn stop_gate_reason_comes_from_i18n() {
        // Default project (no mustard.json) ⇒ pt-BR catalogue.
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_spec(project, "i18n-pt", AC_FAIL);
        approve(project, "i18n-pt");
        bind(project, "s-7pt", "i18n-pt");
        let v = StopGate.evaluate(&stop_input("s-7pt"), &ctx(project)).expect("no error");
        let Verdict::Deny { reason } = v else {
            panic!("expected Deny for the failing pt-BR spec");
        };
        assert!(reason.contains("Verificação de QA"), "pt-BR reason from catalogue: {reason}");
        assert!(reason.contains("AC-1"), "the failing criterion is named: {reason}");

        // A separate en-US project ⇒ the en-US reason (proves the text is
        // locale-driven from the catalogue, not embedded in the gate).
        let dir2 = tempdir().unwrap();
        let project2 = dir2.path();
        std::fs::write(project2.join("mustard.json"), r#"{"lang":"en-US"}"#).unwrap();
        seed_spec(project2, "i18n-en", AC_FAIL);
        approve(project2, "i18n-en");
        bind(project2, "s-7en", "i18n-en");
        let v = StopGate.evaluate(&stop_input("s-7en"), &ctx(project2)).expect("no error");
        let Verdict::Deny { reason } = v else {
            panic!("expected Deny for the failing en-US spec");
        };
        assert!(
            reason.contains("QA verification did not pass"),
            "en-US reason from catalogue: {reason}"
        );
        assert!(!reason.contains("Verificação"), "no pt-BR text leaks into the en-US project");
    }
}
