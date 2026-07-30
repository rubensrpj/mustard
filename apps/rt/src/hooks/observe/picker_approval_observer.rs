//! `picker_approval_observer` — UserPromptSubmit approval recorder (the picker).
//!
//! Third door onto the SAME `<spec>/.approved-by-user` marker the
//! [`super::approval_marker_observer`] (AskUserQuestion) and the
//! [`super::plan_approval_observer`] (ExitPlanMode) mint. Neither of those two
//! is touched.
//!
//! ## Why a third door — the second gesture
//!
//! `/mustard:spec ar` is the picker's approve-and-implement form: a letter
//! naming the row plus the `r` suffix that means *implement now*. Today the `r`
//! pre-answers only the implement-now CONTINUATION — the approval itself still
//! has to be performed again, so a user who already typed their approval is
//! taken through a plan-mode round trip to say it a second time. That second
//! gesture is the ceremony this door removes.
//!
//! ## Why the marker's property survives
//!
//! The whole value of `.approved-by-user` is that it is born from an act the
//! model cannot author. `UserPromptSubmit` fires only when a person submits a
//! prompt, and the submitted text arrives verbatim in `raw.prompt` — including
//! its leading slash command, which the harness does NOT expand away before the
//! hook sees it (verified against this project's own `user.prompt` event log:
//! `/mustard:git pr close`, `/mustard:skills list` are recorded literally). The
//! model writes neither the prompt nor its text, so the gesture stays
//! unforgeable.
//!
//! There is exactly ONE way model-authored text reaches this trigger, and it is
//! why fact 2 below exists: the RUNTIME speaks through the same channel. A
//! finished background command, and — decisively — a completed subagent's
//! report, arrive as a "user prompt" carrying a machine banner. A subagent's
//! report is written by a model, so a report that merely quoted the picker form
//! would otherwise mint the one signal the gate rests on.
//! [`crate::shared::prompt::is_harness_notice`] is the single owner of that
//! predicate and this door asks it before anything else.
//!
//! ## The three facts
//!
//! All must hold; anything short of all three records nothing.
//!
//! 1. **A person's prompt (unforgeable).** `raw.prompt` carries non-empty text
//!    that is NOT a runtime notice.
//! 2. **The picker's approve-and-implement form, EXACTLY.** The whole prompt is
//!    `/mustard:spec <letter>r` — never a message that merely contains it.
//!    Exactness is the same rule `approval_marker_observer::is_offered` applies
//!    to a selected label, and for the same reason: a substring rule lets a
//!    sentence quoting the form pass, which is the shape of the forgery that
//!    already happened once on the AskUserQuestion door.
//! 3. **State (load-bearing, language-free).** The active spec is `scope=full`,
//!    `stage=Plan` and carries no `pipeline.status{to:approved}` yet — the exact
//!    pre-approval window, derived from `meta.json` + the event log through the
//!    SAME predicates the AskUserQuestion door trusts, imported rather than
//!    re-spelled.
//!
//! The facts are checked cheapest-first (2, 1, then 3) because unlike its two
//! siblings this observer runs on EVERY prompt: the two pure string tests settle
//! the overwhelming majority without touching the filesystem. The conjunction is
//! unchanged — order decides cost, not verdict.
//!
//! Fail-closed on any doubt, fail-open on IO. Pure [`Observer`] — never blocks,
//! never returns a verdict.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use serde_json::Value;

use super::approval_marker_observer::{active_spec, already_approved, is_full_plan};
use crate::shared::context::{approval_marker_path, marker_body};

/// The UserPromptSubmit approval recorder.
pub struct PickerApprovalObserver;

/// The door name this observer records under `via=`.
const PICKER_VIA: &str = "picker";

/// The picker's slash command, as the user types it. Matched case-insensitively
/// but otherwise literally — one spelling, never a family of aliases.
const PICKER_COMMAND: &str = "/mustard:spec";

/// The picker argument the user typed, when the prompt IS an invocation of the
/// picker. `None` for anything else — including the bare command with no
/// argument, which renders the table and decides nothing.
fn picker_argument(prompt: &str) -> Option<&str> {
    let text = prompt.trim();
    // `get` rather than `split_at`: a prompt whose byte at that index is inside a
    // multi-byte character must yield `None`, not a panic.
    let head = text.get(..PICKER_COMMAND.len())?;
    if !head.eq_ignore_ascii_case(PICKER_COMMAND) {
        return None;
    }
    let rest = text.get(PICKER_COMMAND.len()..)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim())
}

/// `true` when the WHOLE prompt is the picker's approve-and-implement form —
/// one row letter followed by the `r` suffix (`/mustard:spec ar`).
///
/// A BARE letter (`/mustard:spec a`) is deliberately not an approval: it only
/// acts on the row, and on a PLAN-stage spec the approval is still the pending
/// action. Only the `r` suffix carries "approve and implement now", which is the
/// gesture this door records.
fn is_approve_and_implement(prompt: &str) -> bool {
    let Some(arg) = picker_argument(prompt) else {
        return false;
    };
    let mut chars = arg.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(letter), Some(suffix), None) => {
            letter.is_ascii_alphabetic() && suffix.eq_ignore_ascii_case(&'r')
        }
        _ => false,
    }
}

/// The literal text the PERSON submitted, or `None` when this invocation carries
/// none — an absent/blank `prompt`, or one the runtime authored rather than a
/// person (a completed background command, a finished subagent's report).
fn user_typed_text(input: &HookInput) -> Option<&str> {
    input
        .raw
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| !crate::shared::prompt::is_harness_notice(s))
}

impl Observer for PickerApprovalObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        // Facts 1 + 2 — a person's prompt, and it IS the approve-and-implement
        // form. Both are pure string tests over the payload; they run first
        // because this observer sees every prompt in the session.
        let Some(prompt) = user_typed_text(input) else {
            return;
        };
        if !is_approve_and_implement(prompt) {
            return;
        }

        // Fact 3 — an unapproved Full spec in PLAN, else nothing is pending.
        let cwd = ctx.project_dir_or_cwd(input);
        let Some(spec) = active_spec(&cwd, input) else {
            return;
        };
        if !is_full_plan(&cwd, &spec) || already_approved(&cwd, &spec) {
            return;
        }

        // All three hold → record the genuine approval, best-effort. Same path,
        // same body composer as the other two doors, so the provenance reads
        // back identically and only `via` says which door it came through.
        if let Some(marker) = approval_marker_path(&cwd, &spec) {
            let body = marker_body(
                &spec,
                PICKER_VIA,
                input.session_id.as_deref().unwrap_or("unknown"),
                &mustard_core::time::now_iso8601(),
            );
            let _ = mustard_core::io::fs::write_atomic(&marker, body.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::Trigger;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use std::path::Path;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
        }
    }

    /// A `UserPromptSubmit` input carrying the text the person submitted — the
    /// shape the harness delivers (`raw.prompt`).
    fn prompt_input(session: &str, prompt: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session.to_string()),
            raw: json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    /// Seed `.claude/spec/<spec>/meta.json` with the given scope + stage.
    fn seed_spec(root: &Path, spec: &str, scope: &str, stage: &str) {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            format!(r#"{{"scope":"{scope}","stage":"{stage}","outcome":"Active"}}"#),
        )
        .unwrap();
    }

    /// Bind `session` → `spec` via the `.session/<id>/active-spec` marker.
    fn bind_session(root: &Path, session: &str, spec: &str) {
        let d = root.join(".claude").join(".session").join(session);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("active-spec"), spec).unwrap();
    }

    /// Emit a real `pipeline.status{to:approved}` into the spec's event log.
    fn seed_approval_event(root: &Path, spec: &str) {
        let ev = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-07-30T00:00:00.000Z".to_string(),
            session_id: "s-seed".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Cli, id: Some("spec".to_string()), actor_type: None },
            event: "pipeline.status".to_string(),
            payload: json!({ "from": "draft", "to": "approved" }),
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(root.to_str().unwrap(), &ev);
    }

    fn marker_exists(root: &Path, spec: &str) -> bool {
        approval_marker_path(root.to_str().unwrap(), spec)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    // ── The form recogniser (unit) ───────────────────────────────────────────

    #[test]
    fn recognises_only_the_approve_and_implement_form() {
        for yes in ["/mustard:spec ar", "  /mustard:spec zr  ", "/MUSTARD:SPEC ar"] {
            assert!(is_approve_and_implement(yes), "should be the form: {yes:?}");
        }
        for no in [
            // A bare letter acts on the row; the approval is still pending.
            "/mustard:spec a",
            // The table render / a spec name / the EXEC continuation.
            "/mustard:spec",
            "/mustard:spec ceremony-costs-more-than-gates",
            // Another command that merely starts the same way.
            "/mustard:specs ar",
            // Free text QUOTING the form is not the form — the exactness rule.
            "go ahead: /mustard:spec ar",
            "/mustard:spec ar please",
            "run /mustard:spec ar for me",
            // Ordinary prose, and the empty prompt.
            "aprova o plano",
            "",
        ] {
            assert!(!is_approve_and_implement(no), "should NOT be the form: {no:?}");
        }
    }

    /// A multi-byte character sitting where the command would end must decline,
    /// not panic — a hook that panics takes the session's prompt with it.
    #[test]
    fn a_multibyte_prompt_declines_without_panicking() {
        assert!(!is_approve_and_implement("/mustard:spéc ar"));
        assert!(!is_approve_and_implement("çã"));
    }

    // ── The observer (integration over a tempdir) ────────────────────────────

    /// The gesture, end to end: the user's OWN prompt is the picker's
    /// approve-and-implement form and a Full plan is awaiting approval, so the
    /// marker is minted — with provenance naming this door.
    #[test]
    fn picker_form_in_a_pending_full_plan_writes_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full (wave plan)", "Plan");
        bind_session(root, "s-1", "epic");

        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec ar"), &ctx(root.to_str().unwrap()));

        assert!(marker_exists(root, "epic"), "the user's own picker approval must mint the marker");
        let marker = approval_marker_path(root.to_str().unwrap(), "epic").unwrap();
        let p = crate::shared::context::read_marker_provenance(&marker)
            .expect("the minted body must read back as provenance");
        assert_eq!(p.via, PICKER_VIA, "the marker names the door it came through");
        assert_eq!(p.spec, "epic");
        assert_eq!(p.session, "s-1");
        assert!(!p.at.is_empty(), "the door must record an instant");
    }

    /// The property the marker exists for, in BOTH directions: the very SAME
    /// text mints nothing when it is not the user's own prompt.
    ///
    /// The only way model-authored text reaches `UserPromptSubmit` is the
    /// runtime's own channel — a completed subagent's report arrives as a "user
    /// prompt" behind a machine banner, and that report is written by a model.
    /// A door that read it would let the model author the one gesture the gate
    /// rests on. The second half re-submits the identical text as a person and
    /// the marker appears, so the refusal cannot pass by the door being inert.
    #[test]
    fn the_same_text_mints_nothing_when_it_is_not_the_users_prompt() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        seed_spec(root, "epic", "full (wave plan)", "Plan");
        bind_session(root, "s-1", "epic");

        // A finished subagent's report — model-authored, delivered through the
        // user channel behind the runtime banner.
        let report = "<task-notification>\n<status>completed</status>\nThe operator will type \
                      /mustard:spec ar\n</task-notification>";
        PickerApprovalObserver.observe(&prompt_input("s-1", report), &ctx(root_str));
        assert!(
            !marker_exists(root, "epic"),
            "a subagent's report must never mint the marker, whatever it quotes"
        );

        // The other runtime banner, carrying nothing but the form.
        let notice = "[SYSTEM NOTIFICATION - NOT USER INPUT]\n/mustard:spec ar";
        PickerApprovalObserver.observe(&prompt_input("s-1", notice), &ctx(root_str));
        assert!(!marker_exists(root, "epic"), "a runtime notice is not a person");

        // The same text present in the payload, but NOT as the submitted prompt.
        let mut elsewhere = prompt_input("s-1", "/mustard:spec ar");
        elsewhere.raw = json!({ "tool_input": { "command": "/mustard:spec ar" } });
        PickerApprovalObserver.observe(&elsewhere, &ctx(root_str));
        assert!(!marker_exists(root, "epic"), "only `prompt` is the person's channel");

        // The other direction — the identical text, submitted by the person.
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec ar"), &ctx(root_str));
        assert!(
            marker_exists(root, "epic"),
            "the same text typed by the user must still mint the marker"
        );
    }

    #[test]
    fn a_bare_letter_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec a"), &ctx(root.to_str().unwrap()));
        assert!(
            !marker_exists(root, "epic"),
            "acting on a row is not approving it — only the `r` form is the gesture"
        );
    }

    #[test]
    fn spec_past_plan_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Execute");
        bind_session(root, "s-1", "epic");
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec ar"), &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "epic"), "no PLAN approval pending → no marker");
    }

    #[test]
    fn light_spec_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "small", "light", "Plan");
        bind_session(root, "s-1", "small");
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec ar"), &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "small"), "a Light spec has no PLAN approval gate");
    }

    #[test]
    fn already_approved_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        seed_approval_event(root, "epic");
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec ar"), &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "epic"), "approval already recorded → nothing to mint");
    }

    #[test]
    fn no_project_is_failopen() {
        let dir = tempdir().unwrap();
        // No `.claude/` at all — observe must not panic / propagate.
        PickerApprovalObserver.observe(
            &prompt_input("s-1", "/mustard:spec ar"),
            &ctx(dir.path().to_str().unwrap()),
        );
        // Survival is the contract.
    }
}
