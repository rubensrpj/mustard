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
//! naming the row plus the `r` suffix that means *implement now*. The `r` used
//! to pre-answer only the implement-now CONTINUATION — the approval itself had
//! to be performed again, so a user who had already typed their approval was
//! taken through a plan-mode round trip to say it a second time. That second
//! gesture is the ceremony this door removes.
//!
//! The bare `/mustard:spec r` is the same gesture with the letter left out,
//! and it is accepted for exactly one position: a checkout standing INSIDE a
//! unit's own work branch. There the branch already names the unit, so the
//! table the letter indexes has nothing left to disambiguate — asking for a row
//! letter would be asking the operator to re-state what the tree already says.
//! Outside a unit's branch (an integration base, a detached HEAD, a directory
//! that is not a repository) the bare form names no unit and decides nothing.
//! One consequence is stated rather than hidden: the bare `r` is no longer read
//! as "act on row `r`" — the two spellings collide on that one letter, and this
//! door resolves the collision toward the checkout because a bare letter never
//! minted anything anyway.
//!
//! ## Why the marker's property survives
//!
//! The whole value of `.approved-by-user` is that it is born from an act the
//! model cannot author. `UserPromptSubmit` fires only when a person submits a
//! prompt, and the submitted text arrives verbatim in `raw.prompt` — including
//! its leading slash command, which the harness does NOT expand away before the
//! hook sees it (verified against this project's own `user.prompt` event log:
//! `/mustard:git pr close` is recorded literally). The
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
//!    `/mustard:spec` on its own, or that command plus a single row letter, with
//!    an optional trailing `r` (`a`, `ar`, `r`) — never a
//!    message that merely contains either. Exactness is the same rule
//!    `approval_marker_observer::is_offered` applies to a selected label, and
//!    for the same reason: a substring rule lets a sentence quoting the form
//!    pass, which is the shape of the forgery that already happened once on the
//!    AskUserQuestion door. Widening this to a form that matched inside a longer
//!    message would hand every subagent report that merely SPELLS the gesture
//!    the power to mint the marker.
//! 3. **The spec THE GESTURE names, in the pre-approval window.** A letter is
//!    resolved through [`crate::commands::spec::active_specs::spec_for_letter`]
//!    — the SAME enumerator that rendered the table the user read; the bare
//!    command and the bare `r` are resolved through the checkout's own branch
//!    ([`crate::commands::event::work_branch::slug_of_work_branch`], the same
//!    reading `spec-draft` consumes to name the spec directory). Either way that
//!    spec must be `stage=Plan` and carry no `pipeline.status{to:approved}` yet,
//!    through the SAME predicates the AskUserQuestion door trusts, imported
//!    rather than re-spelled. **Scope is not asked** — see
//!    [`crate::hooks::observe::approval_marker_observer::is_awaiting_approval`]
//!    for the deadlock that requiring `scope=full` here produced.
//!
//! ## The gesture decides WHICH spec — never the session
//!
//! Fact 3 deliberately does NOT ask `active_spec()` (session binding →
//! current-spec → unique-pending), the way the two sibling doors do. Those two
//! read a gesture that carries no spec of its own, so the session is the only
//! thing that can name one. This door's gesture DOES name one — by row letter,
//! or by the branch the tree is standing on — and the picker exists precisely to
//! act on a unit that is not the current spec: honouring the session there would
//! mint a real, unforgeable gesture against the WRONG spec, which for that spec
//! is indistinguishable from a forged one. A letter no row carries, and a
//! checkout inside no unit, both resolve to nothing and mint nothing; the user
//! then approves the ordinary way, and `approve-spec` still refuses without the
//! marker.
//!
//! The facts are checked cheapest-first (2, 1, then 3) because unlike its two
//! siblings this observer runs on EVERY prompt: the two pure string tests settle
//! the overwhelming majority without touching the filesystem — the enumeration
//! in fact 3 (which reads the spec area and the work branches) is reached only
//! by a prompt that already IS the form. The conjunction is unchanged — order
//! decides cost, not verdict.
//!
//! Fail-closed on any doubt, fail-open on IO. Pure [`Observer`] — never blocks,
//! never returns a verdict.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use serde_json::Value;
use std::path::Path;

use super::approval_marker_observer::{already_approved, is_awaiting_approval};
use crate::commands::event::work_branch::{current_branch, slug_of_work_branch};
use crate::commands::spec::active_specs::spec_for_letter;
use crate::shared::context::{approval_marker_path, marker_body};

/// The UserPromptSubmit approval recorder.
pub struct PickerApprovalObserver;

/// The door name this observer records under `via=`.
const PICKER_VIA: &str = "picker";

/// The picker's slash command, as the user types it. Matched case-insensitively
/// but otherwise literally — one spelling, never a family of aliases.
const PICKER_COMMAND: &str = "/mustard:spec";

/// The picker argument the user typed, when the prompt IS an invocation of the
/// picker — the EMPTY string when the command was typed on its own. `None` for
/// anything else.
///
/// The bare command used to return `None` here, on the reading that it "renders
/// the table and decides nothing". Inside the unit's own work branch it decides
/// plenty: the branch names the unit, so there is no table left to render and
/// nothing left to pick. Returning the empty argument lets
/// [`approve_and_implement_target`] read that case; the whitespace rule below is
/// what still keeps `/mustard:specs ar` out.
fn picker_argument(prompt: &str) -> Option<&str> {
    let text = prompt.trim();
    // `get` rather than `split_at`: a prompt whose byte at that index is inside a
    // multi-byte character must yield `None`, not a panic.
    let head = text.get(..PICKER_COMMAND.len())?;
    if !head.eq_ignore_ascii_case(PICKER_COMMAND) {
        return None;
    }
    let rest = text.get(PICKER_COMMAND.len()..)?;
    // Empty is the bare command. Anything else must be separated by whitespace,
    // or `/mustard:specs ar` would read as the picker with argument `s ar`.
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim())
}

/// WHICH plan an approve-and-implement gesture names — the closed set of two
/// spellings, and the only thing that differs between them.
#[derive(Debug, PartialEq, Eq)]
enum ApprovalTarget {
    /// `/mustard:spec a` (or `ar`) — the row the LETTER names in the table the
    /// user read.
    Row(char),
    /// `/mustard:spec` on its own, or `/mustard:spec r` — the unit the CHECKOUT
    /// is standing in. No letter is needed, because the branch already names the
    /// unit.
    ///
    /// `fallback_letter` is `Some('r')` for the spelled-out `r` and `None` for
    /// the bare command. Off a work branch the checkout names nothing, and there
    /// the two spellings must part ways: a typed `r` reads as the row letter it
    /// looks like, while a bare command named no row to begin with.
    Checkout { fallback_letter: Option<char> },
}

/// The plan the gesture names, when the WHOLE prompt is the picker's
/// approve-and-implement form — a row letter followed by the `r` suffix
/// (`/mustard:spec ar`), or that suffix alone (`/mustard:spec r`). `None` for
/// anything else.
///
/// Returns the TARGET rather than a `bool` because the gesture is the only
/// thing that says WHICH spec is being approved; discarding it and asking the
/// session instead lands the marker on whatever spec the session was bound to,
/// which is the plan the user did NOT name.
///
/// **A bare letter counts, and so does the bare command.** They used to be
/// refused, on the reading that a letter "only acts on the row" and that the
/// approval was still a separate pending action. That reading is what made the
/// operator answer the same question twice: they pick row `a`, and the flow then
/// presents the plan and asks whether they approve it. But the picker's own
/// table says a letter means *act on row — PLAN approve*, so the second question
/// re-asks what the first one already answered. The letter arrives through the
/// same `UserPromptSubmit` channel as `ar` and is exactly as impossible for the
/// model to author; the only thing missing was recognising it.
///
/// So the four accepted spellings, and what tells them apart:
///
/// ```text
/// /mustard:spec        the unit the checkout is standing in — nothing else to name
/// /mustard:spec r      the same, spelled out; off a work branch, row `r`
/// /mustard:spec a      row `a`
/// /mustard:spec ar     row `a`, the older spelling, kept as an alias
/// ```
///
/// The `r` arm is matched BEFORE the general letter arm, because `r` is itself a
/// letter and the checkout has to win that collision — which is the rule the
/// picker's prose already stated, back when a bare letter minted nothing and the
/// collision therefore cost nothing.
fn approve_and_implement_target(prompt: &str) -> Option<ApprovalTarget> {
    let arg = picker_argument(prompt)?;
    let mut chars = arg.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (None, _, _) => Some(ApprovalTarget::Checkout { fallback_letter: None }),
        (Some(suffix), None, None) if suffix.eq_ignore_ascii_case(&'r') => {
            Some(ApprovalTarget::Checkout { fallback_letter: Some('r') })
        }
        (Some(letter), None, None) if letter.is_ascii_alphabetic() => {
            Some(ApprovalTarget::Row(letter.to_ascii_lowercase()))
        }
        (Some(letter), Some(suffix), None)
            if letter.is_ascii_alphabetic() && suffix.eq_ignore_ascii_case(&'r') =>
        {
            Some(ApprovalTarget::Row(letter.to_ascii_lowercase()))
        }
        _ => None,
    }
}

/// The unit the CHECKOUT is standing in: the slug half of the work branch the
/// tree is on (`feature/my-unit` → `my-unit`), which IS the name its spec is
/// filed under.
///
/// The branch is the DURABLE record of a unit's name — the pending-work-branch
/// marker that carried it from the gate is consumed by the first checkout — so
/// [`slug_of_work_branch`] is the same reading `spec-draft` consumes to name the
/// spec directory, and the two therefore agree by construction rather than by
/// both deriving the same string. Re-spelling the branch parse here would mint
/// exactly the second name that reading exists to prevent.
///
/// `None` everywhere the tree is not inside a unit: an integration base (a
/// declared base parses to no slug), a detached HEAD, a VCS opt-out, a directory
/// that is not a repository. The bare `r` then names no plan, and a name nobody
/// can resolve mints nothing — the fail-closed direction, exactly as for a
/// letter no row carries.
fn spec_of_checkout(project: &Path) -> Option<String> {
    let config = mustard_core::ProjectConfig::load(project);
    let vcs = config.vcs()?;
    let current = current_branch(&vcs, &project.to_string_lossy())?;
    slug_of_work_branch(&current, &config)
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
        let Some(target) = approve_and_implement_target(prompt) else {
            return;
        };

        // Fact 3 — the plan THE GESTURE names, and it must be an unapproved
        // Full spec in PLAN. A letter resolves through the picker's own
        // enumerator, the bare `r` through the checkout's own branch; never
        // through the session: see the module docs.
        let cwd = ctx.project_dir_or_cwd(input);
        let project = Path::new(&cwd);
        let named = match target {
            ApprovalTarget::Row(letter) => spec_for_letter(project, letter),
            ApprovalTarget::Checkout { fallback_letter } => spec_of_checkout(project)
                .or_else(|| fallback_letter.and_then(|l| spec_for_letter(project, l))),
        };
        let Some(spec) = named else {
            return;
        };
        if !is_awaiting_approval(&cwd, &spec) || already_approved(&cwd, &spec) {
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
            inject_only: None,
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

    /// Seed a spec the PICKER can enumerate: `spec.md` (what discovery globs)
    /// plus the `meta.json` sidecar carrying scope + stage.
    ///
    /// Both files are required — a directory with only `meta.json` is invisible
    /// to `active-specs`, so it would carry no row letter and this door would
    /// (correctly) decide nothing about it.
    fn seed_spec(root: &Path, spec: &str, scope: &str, stage: &str) {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), format!("# {spec}\n\n## Resumo\n\nlinha.\n")).unwrap();
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
        // The letter comes back, lower-cased — it is what names the row.
        for (yes, target) in [
            ("/mustard:spec ar", ApprovalTarget::Row('a')),
            ("  /mustard:spec zr  ", ApprovalTarget::Row('z')),
            ("/MUSTARD:SPEC ar", ApprovalTarget::Row('a')),
            ("/mustard:spec BR", ApprovalTarget::Row('b')),
            // `rr` is still a row: the FIRST char is the letter, as always.
            ("/mustard:spec rr", ApprovalTarget::Row('r')),
            // A BARE letter is the row too — the gesture the picker's own table
            // has always called "act on row, PLAN approve".
            ("/mustard:spec a", ApprovalTarget::Row('a')),
            ("  /mustard:spec Z  ", ApprovalTarget::Row('z')),
            // The suffix alone leaves the row half to the checkout, keeping the
            // letter as the fallback for a tree standing on no unit.
            ("/mustard:spec r", ApprovalTarget::Checkout { fallback_letter: Some('r') }),
            (
                "  /mustard:spec R  ",
                ApprovalTarget::Checkout { fallback_letter: Some('r') },
            ),
            // The bare command names the checkout and nothing else: off a work
            // branch it has no row to fall back to.
            ("/mustard:spec", ApprovalTarget::Checkout { fallback_letter: None }),
            ("  /mustard:spec  ", ApprovalTarget::Checkout { fallback_letter: None }),
        ] {
            assert_eq!(
                approve_and_implement_target(yes),
                Some(target),
                "should be the form: {yes:?}"
            );
        }
        for no in [
            // A spec named in full is not a row letter — the picker routes it
            // straight to that spec, and the approval happens the ordinary way.
            "/mustard:spec ceremony-costs-more-than-gates",
            // Two letters that do not end in the `r` suffix name no row.
            "/mustard:spec ab",
            // A digit is not a row letter.
            "/mustard:spec 1",
            // Another command that merely starts the same way.
            "/mustard:specs ar",
            // Free text QUOTING either form is not the form — the exactness
            // rule, which the bare `r` does not relax by one character.
            "go ahead: /mustard:spec ar",
            "/mustard:spec ar please",
            "run /mustard:spec ar for me",
            "then type /mustard:spec r",
            "/mustard:spec r agora",
            // Ordinary prose, and the empty prompt.
            "aprova o plano",
            "",
        ] {
            assert!(
                approve_and_implement_target(no).is_none(),
                "should NOT be the form: {no:?}"
            );
        }
    }

    /// A multi-byte character sitting where the command would end must decline,
    /// not panic — a hook that panics takes the session's prompt with it.
    #[test]
    fn a_multibyte_prompt_declines_without_panicking() {
        assert!(approve_and_implement_target("/mustard:spéc ar").is_none());
        assert!(approve_and_implement_target("çã").is_none());
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

    /// **The letter decides WHICH spec; the session does not.**
    ///
    /// The picker exists to act on a row that is NOT the spec the session is
    /// working on, so a door that validated only the SHAPE of the letter and
    /// then asked the session for the spec would mint a real, unforgeable
    /// gesture against the wrong plan — and for that plan a marker it never
    /// earned is indistinguishable from a forged one.
    ///
    /// Two pending Full plans, the session bound to the FIRST, and the user
    /// types the SECOND's letter. Both halves are asserted: the row named is
    /// approved, and the bound spec is left untouched.
    #[test]
    fn the_letter_names_the_row_even_when_the_session_is_bound_elsewhere() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // No date prefix ⇒ rows sort by name, so `a` = aardvark, `b` = beluga.
        seed_spec(root, "aardvark-plan", "full", "Plan");
        seed_spec(root, "beluga-plan", "full", "Plan");
        bind_session(root, "s-1", "aardvark-plan");

        PickerApprovalObserver
            .observe(&prompt_input("s-1", "/mustard:spec br"), &ctx(root.to_str().unwrap()));

        assert!(
            marker_exists(root, "beluga-plan"),
            "the row the user's letter named must be the one approved"
        );
        assert!(
            !marker_exists(root, "aardvark-plan"),
            "the session's spec must NOT collect an approval the user gave to another row"
        );
        let marker = approval_marker_path(root.to_str().unwrap(), "beluga-plan").unwrap();
        let p = crate::shared::context::read_marker_provenance(&marker)
            .expect("the minted body must read back as provenance");
        assert_eq!(p.spec, "beluga-plan", "the body must name the row, not the session's spec");
        assert_eq!(p.via, PICKER_VIA);
    }

    /// A letter no row carries names nothing, so nothing is minted — the
    /// fail-closed half of resolving through the enumerator. The user simply
    /// approves the ordinary way and `approve-spec` still refuses without a
    /// marker; the unsafe direction would be minting on the wrong row.
    #[test]
    fn a_letter_no_row_carries_mints_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");

        PickerApprovalObserver
            .observe(&prompt_input("s-1", "/mustard:spec zr"), &ctx(root.to_str().unwrap()));

        assert!(
            !marker_exists(root, "epic"),
            "row `z` does not exist — an unresolvable letter must decide nothing"
        );
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

    /// Run a git command in `root`, asserting success — test scaffolding only.
    fn git(root: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    /// A repo whose single commit lives on `base`, with `git.flow` declared so
    /// the base set is DERIVED (nothing here assumes `dev`/`main`).
    fn repo_on(root: &Path, base: &str) {
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", base]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);
    }

    /// **The bare `r`: the BRANCH names the unit, and the session does not.**
    ///
    /// Inside a unit's own work branch the table has nothing left to
    /// disambiguate — the checkout already says which unit this is — so the
    /// gesture needs no letter. What it must NOT do is fall back to the session,
    /// for the same reason the letter form does not: the session is bound
    /// elsewhere here, and honouring it would mint a real, unforgeable gesture
    /// against a plan the operator never named.
    ///
    /// Both halves are asserted, plus the inverse that always travels with this
    /// door: the identical text arriving as a subagent's report mints nothing.
    #[test]
    fn a_bare_r_inside_the_units_branch_mints_the_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        repo_on(root, "dev");
        seed_spec(root, "epic", "full (wave plan)", "Plan");
        seed_spec(root, "other-plan", "full", "Plan");
        // The session is working on the OTHER pending plan on purpose.
        bind_session(root, "s-1", "other-plan");

        // On the integration base the gesture names no unit at all.
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec r"), &ctx(root_str));
        assert!(!marker_exists(root, "epic"), "an integration base is not a unit's branch");
        assert!(!marker_exists(root, "other-plan"), "and the session must not answer for it");

        git(root, &["checkout", "-b", "feature/epic"]);

        // Model-authored text carrying the very same gesture still mints nothing.
        let report = "<task-notification>\n<status>completed</status>\nThe operator will type \
                      /mustard:spec r\n</task-notification>";
        PickerApprovalObserver.observe(&prompt_input("s-1", report), &ctx(root_str));
        assert!(
            !marker_exists(root, "epic"),
            "a subagent's report must never mint the marker, whatever it quotes"
        );

        // The person's own prompt, inside the unit's branch → the branch decides.
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec r"), &ctx(root_str));
        assert!(marker_exists(root, "epic"), "the unit the checkout stands in is the one approved");
        assert!(
            !marker_exists(root, "other-plan"),
            "the session's spec must NOT collect an approval the branch did not name"
        );
        let marker = approval_marker_path(root_str, "epic").unwrap();
        let p = crate::shared::context::read_marker_provenance(&marker)
            .expect("the minted body must read back as provenance");
        assert_eq!(p.spec, "epic", "the body names the unit, not the session's spec");
        assert_eq!(p.via, PICKER_VIA);
    }

    /// Outside a unit, the bare `r` decides nothing — every position that cannot
    /// SHOW which unit the tree is in resolves to no spec.
    #[test]
    fn the_bare_r_decides_nothing_outside_a_unit_branch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        repo_on(root, "dev");
        seed_spec(root, "epic", "full", "Plan");

        // A neighbouring unit's branch names a slug this project has no spec for.
        git(root, &["checkout", "-b", "feature/another-unit"]);
        PickerApprovalObserver.observe(&prompt_input("s-1", "/mustard:spec r"), &ctx(root_str));
        assert!(!marker_exists(root, "epic"), "another unit's branch never answers for this spec");
        assert!(
            spec_of_checkout(root).as_deref() == Some("another-unit"),
            "the reading is the branch's own slug, not a guess"
        );

        // A detached HEAD is standing on no branch at all.
        git(root, &["checkout", "--detach", "HEAD"]);
        assert_eq!(spec_of_checkout(root), None, "a detached HEAD names no unit");

        // …and a directory that is not a repository names none either.
        let bare = tempdir().unwrap();
        assert_eq!(spec_of_checkout(bare.path()), None);
        PickerApprovalObserver.observe(
            &prompt_input("s-1", "/mustard:spec r"),
            &ctx(bare.path().to_str().unwrap()),
        );
    }

    /// The letter alone IS the approval. It used to mint nothing, which is what
    /// made the operator answer the same question twice: pick the row, then
    /// approve the plan the row named. The picker's table has always said a
    /// letter means *act on row — PLAN approve*.
    #[test]
    fn bare_letter_mints_the_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        PickerApprovalObserver
            .observe(&prompt_input("s-1", "/mustard:spec a"), &ctx(root.to_str().unwrap()));
        assert!(
            marker_exists(root, "epic"),
            "picking the row IS approving it — the second question was the ceremony"
        );
    }

    /// The bare command, inside the unit's own work branch. Standing in the unit
    /// already answers *which one*, so there is no row left to pick and no
    /// letter left to type.
    #[test]
    fn bare_letter_mints_from_the_command_alone_inside_the_units_branch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        repo_on(root, "dev");
        git(root, &["checkout", "-b", "feature/epic"]);
        PickerApprovalObserver
            .observe(&prompt_input("s-1", "/mustard:spec"), &ctx(root.to_str().unwrap()));
        assert!(
            marker_exists(root, "epic"),
            "inside the unit's branch the bare command names it and approves it"
        );
    }

    /// …and off any unit's branch the bare command names nothing, so it is the
    /// table render it always was.
    #[test]
    fn the_bare_command_mints_nothing_outside_a_unit_branch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        repo_on(root, "dev");
        PickerApprovalObserver
            .observe(&prompt_input("s-1", "/mustard:spec"), &ctx(root.to_str().unwrap()));
        assert!(
            !marker_exists(root, "epic"),
            "no unit branch and no letter → the gesture names nothing, and the session must not answer for it"
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

    /// A `light` spec is approvable. Requiring `scope=full` here made every
    /// light plan impossible to approve: the three doors that mint the marker
    /// all demanded `full`, while `approve-spec` demands the marker from specs
    /// of EVERY scope. The operator made the gesture the refusal text names,
    /// nothing was written, and the only way through was the
    /// `MUSTARD_APPROVAL_MODE=warn` escape hatch. Measured twice on this
    /// repository — the second time on the unit that removed it.
    #[test]
    fn light_spec_can_be_approved() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "small", "light", "Plan");
        bind_session(root, "s-1", "small");
        PickerApprovalObserver
            .observe(&prompt_input("s-1", "/mustard:spec ar"), &ctx(root.to_str().unwrap()));
        assert!(
            marker_exists(root, "small"),
            "scope decides how much ceremony a plan carries, never whether a person's approval of it counts"
        );
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
