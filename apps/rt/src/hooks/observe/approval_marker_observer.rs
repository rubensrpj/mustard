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
//! ## Fail-closed by construction
//!
//! Every branch that cannot PROVE all three facts records nothing, and every IO
//! step degrades to a no-op — the observer never blocks (it is a pure
//! [`Observer`]) and never mints a marker on uncertainty.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
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
/// session→spec binding (precise), then the newest-pipeline-state hint — the
/// same two-tier resolution the other spec-scoped hooks use. Shared with
/// [`super::plan_approval_observer`] (the plan-mode recorder).
pub(crate) fn active_spec(cwd: &str, input: &HookInput) -> Option<String> {
    let sid = input.session_id.as_deref().unwrap_or("");
    spec_for_session(cwd, sid).or_else(|| current_spec(cwd))
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

        // Facts 2 + 3 — a real user answer that is affirmative.
        if !selected_labels(input).iter().any(|l| is_affirmative(l)) {
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
}
