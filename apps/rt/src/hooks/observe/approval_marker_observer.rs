//! `approval_marker_observer` — PostToolUse(AskUserQuestion) approval recorder (T5).
//!
//! ## Why this exists — the forgeable approval gate
//!
//! The Full pipeline's PLAN→EXECUTE gate keys on a `pipeline.status{to:approved}`
//! event that `approve-spec` emits (`scope_guard`, `block_unapproved_execute`).
//! In the field an orchestrator ran `approve-spec` itself and walked past the
//! gate with NO human in the loop — a gate the gated can open alone is not a
//! gate. This observer supplies the ONE signal the model cannot author: the
//! user's real answer to the approval `AskUserQuestion`. The harness echoes that
//! answer in `tool_response` (`{answers:{<question>: <selected label>}}`); the
//! model writes the *question*, never the *answer*. When the answer is an
//! affirmative approval of the active Full-scope spec still awaiting approval in
//! PLAN, we drop `<spec>/.approved-by-user`. `approve-spec` then REQUIRES that
//! marker (strict), so the approval must originate from the user.
//!
//! ## What counts as approval — the hard point, resolved fail-closed
//!
//! Recognition rests on THREE facts that must ALL hold; on any doubt we write
//! nothing:
//!
//! 1. **State (load-bearing, unforgeable, language-free).** The active spec is
//!    `scope=full`, `stage=Plan`, and carries NO `pipeline.status{to:approved}`
//!    yet — the exact "unapproved Full spec in PLAN" window `scope_guard` treats
//!    as pre-approval. This both identifies WHICH spec and proves an approval is
//!    the pending action. It is derived from the deterministic `meta.json` + the
//!    event log, so the model cannot fabricate it.
//! 2. **A real user answer (unforgeable).** `tool_response.answers` holds ≥1
//!    non-empty selection. An empty `{}` (cancel / dismiss) records nothing.
//! 3. **Affirmative selection.** A selected option label is an *approval* rather
//!    than a reject / adjust / stop. We do NOT hardcode a multilingual approval
//!    dictionary (fragile, and the corpus-over-hand-curated rule forbids it):
//!    the label is split into word tokens, lowercased, and a token must START
//!    WITH the canonical approval stem for the project's UI languages
//!    (`approv` / `aprov`). Word-boundary (not substring) matching is what makes
//!    this robust to i18n negation — `desaprovar`, `reprovar`, `disapprove`,
//!    `reject`, `parar`, `ajustar`, `stop` all fail, while `Aprovar…` /
//!    `Approve…` (both "implement now" and "approve only") pass. A label in a
//!    language outside that set never matches (fail-closed) — the operator
//!    widens the stems or relaxes `MUSTARD_APPROVAL_MODE`. The stem only
//!    separates approve from reject *within an already-genuine answer*; facts
//!    1+2 carry the security weight.
//!
//! Why not a pre-declared "awaiting approval of X" marker (the arming variant)?
//! The state window in fact 1 already declares "spec X awaits approval"
//! deterministically, so a separate arming step (a command the flow must call
//! before every question) would add surface without adding a signal the model
//! could not equally influence.
//!
//! ## Fail-closed, but never silent
//!
//! Every branch that cannot PROVE all three facts records nothing, and every IO
//! step degrades to a no-op — the observer never blocks (it is a pure
//! [`Observer`]) and never mints a marker on uncertainty.
//!
//! Fact 3 is the one condition the *author of the question* controls and could
//! not previously discover. A plan awaiting approval, answered "Sim, pode ir",
//! recorded nothing and said nothing; the run then died at `approve-spec`'s
//! refusal, which names the missing marker but not the reason it is missing.
//! The stem requirement was documented only here, in the source. So when facts
//! 1 and 2 hold and only fact 3 fails, the observer now NAMES that condition on
//! stderr ([`unrecognised_answer_notice`]). This changes nothing about what the
//! gate accepts — the stems and the fail-closed default are untouched — only
//! about what it explains when it declines.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::path::Path;

use crate::shared::context::{approval_marker_path, current_spec, spec_for_session};

/// The PostToolUse(AskUserQuestion) approval recorder.
pub struct ApprovalMarkerObserver;

/// Canonical approval stems for the project's UI languages. A selected option
/// whose FIRST word starts with one of these (case-folded) is an approval. See
/// the module docs for why this is word-boundary, not substring.
const APPROVAL_STEMS: &[&str] = &["approv", "aprov"];

/// Resolve the spec the current session is deciding on; `None` on any
/// uncertainty (which the caller treats as "record nothing"). Prefers the
/// session→spec binding (precise), then the newest-pipeline-state hint, then —
/// only when both are silent — the UNIQUE pending Full plan (see
/// [`unique_pending_full_plan`]). Shared with [`super::plan_approval_observer`]
/// (the plan-mode recorder).
pub(crate) fn active_spec(cwd: &str, input: &HookInput) -> Option<String> {
    let sid = input.session_id.as_deref().unwrap_or("");
    spec_for_session(cwd, sid)
        .or_else(|| current_spec(cwd))
        .or_else(|| unique_pending_full_plan(cwd))
}

/// Last-resort spec resolution for [`active_spec`] when neither the session→spec
/// binding nor the legacy `.pipeline-states/` hint names a spec: the UNIQUE spec
/// whose `meta.json` sits in the exact fact-1 window — `scope=full`, `stage=Plan`,
/// and NOT yet approved. Exactly one such spec is unambiguous and IS the plan
/// being approved; zero or MORE THAN ONE returns `None` (fail-closed), so a real
/// approval is never attributed to the wrong spec.
///
/// Field evidence (2026-07-18): the emitter-side session bind raced to a dead
/// session, so both approval observers went blind and a genuine user approval
/// minted nothing. Reusing [`is_full_plan`] + [`already_approved`] — the same
/// predicates the observer's fact 1 already trusts — keeps this fallback aligned
/// with the gate and free of a second, driftable definition of "pending Full plan".
fn unique_pending_full_plan(cwd: &str) -> Option<String> {
    let spec_dir = ClaudePaths::for_project(Path::new(cwd)).ok()?.spec_dir();
    let mut pending = fs::read_dir(&spec_dir)
        .ok()?
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.file_name)
        .filter(|name| is_full_plan(cwd, name) && !already_approved(cwd, name));
    let first = pending.next()?;
    // A second candidate makes attribution ambiguous → record nothing.
    if pending.next().is_some() {
        return None;
    }
    Some(first)
}

/// `true` when `spec` is a Full-scope spec still in stage `Plan` (from its
/// `meta.json`) — the only lifecycle state where a PLAN approval is pending.
pub(crate) fn is_full_plan(cwd: &str, spec: &str) -> bool {
    let Some(sp) = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec))
        .ok()
    else {
        return false;
    };
    let Some(meta) = mustard_core::read_meta(&sp.meta_json_path()) else {
        return false;
    };
    let is_full = meta
        .scope
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false);
    let is_plan = meta
        .stage
        .as_deref()
        .map(|s| s.trim().eq_ignore_ascii_case("Plan"))
        .unwrap_or(false);
    is_full && is_plan
}

/// `true` when the spec already carries a `pipeline.status{to:approved}` event —
/// approval has already happened, so there is nothing to record.
pub(crate) fn already_approved(cwd: &str, spec: &str) -> bool {
    let Some(events_dir) = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
    else {
        return false;
    };
    read_harness_events_from_ndjson_dir(&events_dir).iter().any(|ev| {
        ev.event == "pipeline.status"
            && ev.payload.get("to").and_then(Value::as_str) == Some("approved")
    })
}

/// Collect every option label the user actually selected from
/// `tool_response.answers` — a map `{<question>: <label> | [<label>, …]}`. An
/// empty map (cancel / dismiss) yields nothing.
fn selected_labels(input: &HookInput) -> Vec<String> {
    let Some(answers) = input
        .raw
        .get("tool_response")
        .and_then(|r| r.get("answers"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for v in answers.values() {
        match v {
            Value::String(s) if !s.trim().is_empty() => out.push(s.clone()),
            Value::Array(items) => {
                for s in items.iter().filter_map(Value::as_str) {
                    if !s.trim().is_empty() {
                        out.push(s.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// `true` when a selected label is an affirmative approval — some word token
/// (lowercased, split on non-alphanumeric runs) starts with a canonical approval
/// stem. Word-boundary, so `desaprovar` / `reprovar` / `disapprove` do NOT match
/// while `Aprovar…` / `Approve…` do.
fn is_affirmative(label: &str) -> bool {
    label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .any(|w| APPROVAL_STEMS.iter().any(|&stem| w.starts_with(stem)))
}

/// Explain a decline the author of the question could not otherwise discover.
///
/// Returns the advisory text for the ONE case worth explaining: a real answer
/// arrived (`labels` is non-empty) while an approval was genuinely pending, yet
/// no selected label carries an approval stem — so nothing was recorded. It
/// names the condition, the labels that failed it, and the stems that satisfy
/// it, because the requirement is invisible from outside this module.
///
/// `None` when there is nothing to explain: an empty `labels` is a cancelled or
/// dismissed dialog, which answers no question and therefore fails no condition.
///
/// A deliberate rejection also lands here and is told the same thing. That is
/// the honest trade: distinguishing "the user said no" from "the user said yes
/// in words we do not recognise" would take a hand-curated multilingual
/// negation dictionary — exactly what the corpus-over-curation rule forbids —
/// so the notice states the condition and leaves the reading to the human.
fn unrecognised_answer_notice(spec: &str, labels: &[String]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let selected = labels
        .iter()
        .map(|l| format!("{:?}", l.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    let stems = APPROVAL_STEMS
        .iter()
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(" / ");
    Some(format!(
        "[approval] `{spec}` awaits approval, but NOTHING was recorded: no word in the \
         selected option ({selected}) begins with {stems} — the stem this recorder requires \
         to tell an approval from a rejection. If that answer WAS an approval, phrase the \
         option label with that stem (\"Approve …\" / \"Aprovar …\") and ask again; \
         `approve-spec` will keep refusing until `.approved-by-user` exists. If it was a \
         rejection, nothing is wrong."
    ))
}

impl Observer for ApprovalMarkerObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = ctx.project_dir_or_cwd(input);

        // Fact 1 — an unapproved Full spec in PLAN, else nothing is pending.
        let Some(spec) = active_spec(&cwd, input) else {
            return;
        };
        if !is_full_plan(&cwd, &spec) || already_approved(&cwd, &spec) {
            return;
        }

        // Facts 2 + 3 — a real user answer that is affirmative. A decline is
        // still fail-closed (nothing is written), but it no longer happens in
        // silence: the unmet condition is named on stderr. Advisory only — an
        // `eprintln!` is a pure side-effect and can never turn this Observer
        // into a verdict.
        let labels = selected_labels(input);
        if !labels.iter().any(|l| is_affirmative(l)) {
            if let Some(notice) = unrecognised_answer_notice(&spec, &labels) {
                eprintln!("{notice}");
            }
            return;
        }

        // All three facts hold → record the genuine approval, best-effort.
        if let Some(marker) = approval_marker_path(&cwd, &spec) {
            let body = format!(
                "spec={spec}\nvia=AskUserQuestion\nsession={}\n",
                input.session_id.as_deref().unwrap_or("unknown")
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
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        }
    }

    /// A PostToolUse(AskUserQuestion) input carrying `tool_response.answers`.
    fn ask_input(session: &str, answers: Value) -> HookInput {
        HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(session.to_string()),
            raw: json!({ "tool_response": { "questions": [], "answers": answers } }),
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

    /// Bind `session` → `spec` via the `.session/<id>/active-spec` marker so
    /// `active_spec` resolves deterministically (no process-env dependency).
    fn bind_session(root: &Path, session: &str, spec: &str) {
        let d = root.join(".claude").join(".session").join(session);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("active-spec"), spec).unwrap();
    }

    /// Emit a real `pipeline.status{to:approved}` into the spec's event log.
    fn seed_approval_event(root: &Path, spec: &str) {
        let ev = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-07-09T00:00:00.000Z".to_string(),
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

    // ── The affirmative recognizer (unit) ────────────────────────────────────

    #[test]
    fn affirmative_matches_approve_words_across_languages() {
        for yes in [
            "Aprovar e implementar agora — wave 1",
            "Approve and implement now — wave 1",
            "Approve only — new session",
            "Aprovar apenas — nova sessão",
            "APROVAR",
        ] {
            assert!(is_affirmative(yes), "should be affirmative: {yes}");
        }
    }

    #[test]
    fn affirmative_rejects_negations_and_stops() {
        // Word-boundary is the point: negation-prefixed forms must NOT match.
        for no in [
            "Rejeitar decomposição",
            "Reject decomposition",
            "Stop — re-plan",
            "Adjust-stop",
            "Ajustar-parar",
            "Desaprovar",
            "Reprovar",
            "Disapprove",
        ] {
            assert!(!is_affirmative(no), "should NOT be affirmative: {no}");
        }
    }

    // ── The decline notice (unit) ────────────────────────────────────────────

    #[test]
    fn notice_names_the_condition_the_label_failed() {
        let labels = vec!["Sim, pode ir".to_string()];
        let msg = unrecognised_answer_notice("epic", &labels).expect("a real answer is explained");
        // The spec, the label that failed, and BOTH stems that would satisfy it.
        assert!(msg.contains("epic"), "names the spec: {msg}");
        assert!(msg.contains("Sim, pode ir"), "quotes the selected label: {msg}");
        assert!(msg.contains("approv") && msg.contains("aprov"), "names the stems: {msg}");
        // And that the consequence is nothing recorded, not something rejected.
        assert!(msg.contains(".approved-by-user"), "names the marker: {msg}");
    }

    #[test]
    fn notice_stays_silent_on_a_dismissed_dialog() {
        // No answer was given, so no condition was failed — nothing to explain.
        assert_eq!(unrecognised_answer_notice("epic", &[]), None);
    }

    // ── The observer (integration over a tempdir) ────────────────────────────

    #[test]
    fn approval_in_full_plan_writes_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full (wave plan)", "Plan");
        bind_session(root, "s-1", "epic");
        let input = ask_input("s-1", json!({ "Approve the plan?": "Aprovar e implementar agora" }));
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(marker_exists(root, "epic"), "a genuine approval must mint the marker");
    }

    #[test]
    fn english_approve_only_also_writes_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        // Multi-select array form + the "approve only" option are both approvals.
        let input = ask_input("s-1", json!({ "Decision": ["Approve only — new session"] }));
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(marker_exists(root, "epic"));
    }

    #[test]
    fn rejection_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        let input = ask_input("s-1", json!({ "Decision": "Rejeitar decomposição" }));
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "epic"), "a rejection must NOT mint the marker");
    }

    #[test]
    fn cancelled_empty_answers_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        let input = ask_input("s-1", json!({})); // dismissed dialog
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "epic"));
    }

    #[test]
    fn no_spec_in_plan_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Full spec, but already past PLAN (Execute) — no approval is pending.
        seed_spec(root, "epic", "full", "Execute");
        bind_session(root, "s-1", "epic");
        let input = ask_input("s-1", json!({ "Decision": "Approve and implement now" }));
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "epic"), "no PLAN approval pending → no marker");
    }

    #[test]
    fn light_spec_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A Light spec has no PLAN approval gate at all.
        seed_spec(root, "small", "light", "Plan");
        bind_session(root, "s-1", "small");
        let input = ask_input("s-1", json!({ "Decision": "Aprovar" }));
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "small"));
    }

    #[test]
    fn already_approved_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        seed_approval_event(root, "epic"); // approval already recorded
        let input = ask_input("s-1", json!({ "Decision": "Aprovar e implementar agora" }));
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        // No re-mint: the state gate short-circuits once approval exists. The
        // marker was never written by this observe (the approval predates it).
        assert!(!marker_exists(root, "epic"));
    }

    #[test]
    fn no_project_is_failopen() {
        let dir = tempdir().unwrap();
        // No `.claude/` at all — observe must not panic / propagate.
        let input = ask_input("s-1", json!({ "Decision": "Aprovar" }));
        ApprovalMarkerObserver.observe(&input, &ctx(dir.path().to_str().unwrap()));
        // Survival is the contract.
    }

    // ── The unbound fallback: the UNIQUE pending Full plan ────────────────────

    #[test]
    fn unique_pending_full_plan_resolves_without_binding() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        // One Full spec in PLAN, unapproved, and NO session binding at all — the
        // field-incident shape (the emitter bound to a dead session).
        seed_spec(root, "epic", "full (wave plan)", "Plan");

        // Deterministic guarantee: the resolver finds the single pending Full plan
        // with no binding, no env override, no pipeline-states hint.
        assert_eq!(
            unique_pending_full_plan(root_str).as_deref(),
            Some("epic"),
            "the unique full/Plan/unapproved spec resolves as the pending plan",
        );

        // End-to-end: an unbound session's genuine approval now mints the marker
        // via the fallback. `active_spec` still consults `current_spec` first,
        // which honours `MUSTARD_ACTIVE_SPEC`; skip the mint assertion when that
        // override is inherited so the test never flakes on an ambient env.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_none() {
            let input = ask_input("s-unbound", json!({ "Approve?": "Aprovar e implementar agora" }));
            ApprovalMarkerObserver.observe(&input, &ctx(root_str));
            assert!(
                marker_exists(root, "epic"),
                "an unbound session's real approval mints the marker via the fallback",
            );
        }
    }

    #[test]
    fn two_pending_full_plans_stay_none() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        // TWO Full specs in PLAN, both unapproved → ambiguous.
        seed_spec(root, "epic-a", "full", "Plan");
        seed_spec(root, "epic-b", "full", "Plan");

        // Deterministic guarantee: ambiguity resolves to nothing (fail-closed) —
        // an approval is never attributed by guessing between candidates.
        assert_eq!(
            unique_pending_full_plan(root_str),
            None,
            "two pending Full plans are ambiguous → the fallback declines",
        );

        // End-to-end: an unbound approval mints NOTHING under ambiguity. Guarded
        // against an ambient `MUSTARD_ACTIVE_SPEC` for the same reason as above.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_none() {
            let input = ask_input("s-unbound", json!({ "Decision": "Aprovar" }));
            ApprovalMarkerObserver.observe(&input, &ctx(root_str));
            assert!(!marker_exists(root, "epic-a"), "no marker on ambiguity");
            assert!(!marker_exists(root, "epic-b"), "no marker on ambiguity");
        }
    }
}
