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
//! 2. **A real SELECTION (unforgeable).** `tool_response.answers` holds ≥1
//!    non-empty answer that is EXACTLY one of the option labels the question
//!    offered (`tool_input`, authored by the model and echoed by the harness).
//!    An empty `{}` (cancel / dismiss) records nothing — and so does free text.
//!
//!    Free text is the hole this requirement closes, REPRODUCED in the field:
//!    the harness lets the user answer by typing their own words through the
//!    `Other` row or the notes field, and that answer lands in the SAME
//!    `answers` map as a selected label. Fact 3 below only asks whether SOME
//!    word carries the approval stem, so a long message that merely *mentions*
//!    approval — a field report discussing it, for instance — minted the one
//!    signal the whole gate rests on being unforgeable, and the marker then
//!    froze the wave layout and silently discarded a plan revision. An answer
//!    the model did not offer as an option is therefore never an approval,
//!    whatever words it contains. When the offered options cannot be read at
//!    all, nothing is offered and nothing is minted: fail-closed.
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
//!
//! Fact 1 declines the same way, and for the same reason: an approval that was
//! really SELECTED while nothing was awaiting one used to return in silence, so
//! the operator spent an unforgeable gesture, it counted for nothing, and the
//! run only said so much later at `approve-spec`'s refusal.
//! [`missing_plan_notice`] names which half of fact 1 failed. It fires ONLY
//! behind facts 2 and 3 — this module observes every `AskUserQuestion` in the
//! session, and a wider condition would warn on every ordinary question.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::path::Path;

use crate::shared::context::{approval_marker_path, current_spec, marker_body, spec_for_session};

/// The PostToolUse(AskUserQuestion) approval recorder.
pub struct ApprovalMarkerObserver;

/// Canonical approval stems for the project's UI languages. A selected option
/// whose FIRST word starts with one of these (case-folded) is an approval. See
/// the module docs for why this is word-boundary, not substring.
const APPROVAL_STEMS: &[&str] = &["approv", "aprov"];

/// Resolve the spec the current session is deciding on; `None` on any
/// uncertainty (which the caller treats as "record nothing"). A thin reading of
/// [`resolve_pending_plan`], so the spec it names ALWAYS satisfies fact 1.
/// Shared with [`super::plan_approval_observer`] (the plan-mode recorder).
pub(crate) fn active_spec(cwd: &str, input: &HookInput) -> Option<String> {
    match resolve_pending_plan(cwd, input) {
        PendingPlan::Awaiting(spec) => Some(spec),
        PendingPlan::Outside { .. } | PendingPlan::Unresolved => None,
    }
}

/// Where the resolution ladder landed relative to fact 1.
///
/// The two failure shapes are kept apart because the caller must NAME which one
/// happened: a ladder that could not resolve a spec at all is a different thing
/// from one that resolved a spec sitting outside the pre-approval window, and
/// the operator's next move differs accordingly.
enum PendingPlan {
    /// A spec in the pre-approval window — `scope=full`, `stage=Plan`, no
    /// `pipeline.status{to:approved}` yet. Fact 1 holds, and this is the spec
    /// it holds for.
    Awaiting(String),
    /// Every rung that named a spec named one outside that window. The FIRST
    /// such candidate is carried, with the reason it fell outside.
    Outside { spec: String, already_approved: bool },
    /// No rung named a spec at all.
    Unresolved,
}

/// Walk the resolution ladder and report where it landed.
///
/// The rungs are unchanged and in the same order — the session→spec binding
/// (precise), then the legacy `.pipeline-states/` hint, then the UNIQUE pending
/// Full plan (see [`unique_pending_full_plan`]) — but a rung now ENDS the walk
/// only when what it named satisfies fact 1.
///
/// That is the whole point. The ladder used to stop at the first rung that
/// answered ANYTHING, so a stale `.pipeline-states/` file naming a spec long
/// past PLAN shadowed the third rung — the rung written precisely for the case
/// where the first two cannot answer — and a genuine user approval then minted
/// nothing. A rung's answer is a HINT about which spec is in play, never proof
/// that an approval is pending for it; only fact 1 is that proof, so fact 1 is
/// what decides when the walk is over.
fn resolve_pending_plan(cwd: &str, input: &HookInput) -> PendingPlan {
    let sid = input.session_id.as_deref().unwrap_or("");
    // Bound as closures so the ladder stays LAZY: the directory scan behind the
    // third rung is only paid for when the two cheap ones failed.
    let session_rung = || spec_for_session(cwd, sid);
    let current_rung = || current_spec(cwd);
    let unique_rung = || unique_pending_full_plan(cwd);
    let rungs: [&dyn Fn() -> Option<String>; 3] = [&session_rung, &current_rung, &unique_rung];

    let mut outside: Option<PendingPlan> = None;
    for rung in rungs {
        let Some(spec) = rung() else {
            continue;
        };
        if !is_full_plan(cwd, &spec) {
            outside.get_or_insert(PendingPlan::Outside { spec, already_approved: false });
            continue;
        }
        if already_approved(cwd, &spec) {
            outside.get_or_insert(PendingPlan::Outside { spec, already_approved: true });
            continue;
        }
        return PendingPlan::Awaiting(spec);
    }
    outside.unwrap_or(PendingPlan::Unresolved)
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

/// Every option label the QUESTION OFFERED, read from `tool_input`.
///
/// The harness echoes the tool's own input back to PostToolUse, so this is the
/// menu the model authored — not anything the answer can influence. Collected by
/// walking the document for any `options` array and taking each entry's `label`
/// (or the entry itself, when the option is a bare string), which keeps this
/// robust to where the array is nested (`{questions:[{options:[…]}]}` today).
///
/// An empty result means nothing can be shown to have been offered, and the
/// caller then mints nothing — the fail-closed direction.
fn offered_labels(input: &HookInput) -> Vec<String> {
    fn walk(node: &Value, out: &mut Vec<String>) {
        match node {
            Value::Object(map) => {
                for (key, value) in map {
                    if key == "options" {
                        if let Some(items) = value.as_array() {
                            for item in items {
                                let label = match item {
                                    Value::String(s) => Some(s.as_str()),
                                    other => other.get("label").and_then(Value::as_str),
                                };
                                if let Some(l) = label.filter(|l| !l.trim().is_empty()) {
                                    out.push(l.to_string());
                                }
                            }
                        }
                    }
                    walk(value, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&input.tool_input, &mut out);
    out
}

/// `true` when `answer` is EXACTLY one of the `offered` option labels (trimmed).
///
/// Exactness is the whole point: a substring or prefix rule would let free text
/// that quotes an option back in a longer sentence pass, which is the shape of
/// the incident. Nothing offered ⇒ nothing selected.
fn is_offered(answer: &str, offered: &[String]) -> bool {
    offered.iter().any(|o| o.trim() == answer.trim())
}

/// `true` when a selected label is an affirmative approval — some word token
/// (lowercased, split on non-alphanumeric runs) starts with a canonical approval
/// stem. Word-boundary, so `desaprovar` / `reprovar` / `disapprove` do NOT match
/// while `Aprovar…` / `Approve…` do.
///
/// Only ever asked about an answer that already passed [`is_offered`]: the stem
/// separates approve from reject *within a genuine selection*, and was never
/// meant to judge arbitrary prose.
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
/// TWO conditions can now fail, and they are told apart because their remedies
/// differ. When no answer is one of the OFFERED options, the answer was typed
/// as free text (the `Other` row / the notes field) — the operator's genuine
/// approval was real but unselectable, and the remedy is to answer again by
/// picking the option. When an offered option WAS selected but carries no
/// approval stem, the original condition applies.
///
/// A deliberate rejection also lands in the second case and is told the same
/// thing. That is the honest trade: distinguishing "the user said no" from "the
/// user said yes in words we do not recognise" would take a hand-curated
/// multilingual negation dictionary — exactly what the corpus-over-curation rule
/// forbids — so the notice states the condition and leaves the reading to the
/// human.
fn unrecognised_answer_notice(spec: &str, labels: &[String], offered: &[String]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let quote = |values: &[String]| -> String {
        values
            .iter()
            .map(|v| format!("{:?}", truncate(v.trim())))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let selected = quote(labels);
    if !labels.iter().any(|l| is_offered(l, offered)) {
        let menu = if offered.is_empty() {
            "the question offered no options this recorder could read".to_string()
        } else {
            format!("the options offered were: {}", quote(offered))
        };
        return Some(format!(
            "[approval] `{spec}` awaits approval, but NOTHING was recorded: the answer \
             ({selected}) is not one of the options the question offered, so it is free text — \
             and free text never mints the approval marker, whatever words it contains (that is \
             how a message merely MENTIONING approval once forged one). {menu}. If you did mean \
             to approve, answer the question again by SELECTING the approval option instead of \
             typing; `approve-spec` will keep refusing until `.approved-by-user` exists."
        ));
    }
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

/// Explain a genuine approval that found NO plan to land on — the fact-1 half
/// of the same debt [`unrecognised_answer_notice`] pays for facts 2 and 3.
///
/// Reached only when facts 2 and 3 ALREADY hold: an offered option was really
/// selected and it carries the approval stem. That narrowness is the design,
/// not an oversight — this observer sees EVERY `AskUserQuestion` in the session,
/// and almost none of them is about approving a plan, so a notice on any weaker
/// condition would turn ordinary questions into a stream of warnings.
///
/// The three failures are named apart because their remedies are: nothing
/// resolved (approve from the session that owns the plan, or name the row),
/// something resolved but outside the `full`+`Plan` window (the wrong spec is in
/// play), and already approved (there is simply nothing left to record). The
/// silent version of this cost the operator the gesture itself: it was spent,
/// counted for nothing, and the run only said so later at `approve-spec`'s
/// refusal, which names the missing marker but not the reason it is missing.
///
/// `None` for [`PendingPlan::Awaiting`], which is not a failure at all.
fn missing_plan_notice(pending: &PendingPlan) -> Option<String> {
    let tail = "`approve-spec` will keep refusing until `<spec>/.approved-by-user` exists.";
    match pending {
        PendingPlan::Awaiting(_) => None,
        PendingPlan::Unresolved => Some(format!(
            "[approval] an approval was SELECTED, but NOTHING was recorded: no spec could be \
             resolved for this session — no session→spec binding, no current-spec hint, and no \
             single Full spec awaiting approval in PLAN. Approve from the session that owns the \
             plan, or name the row yourself with `/mustard:spec <letter>r`. {tail}"
        )),
        PendingPlan::Outside { spec, already_approved: false } => Some(format!(
            "[approval] an approval was SELECTED, but NOTHING was recorded: the spec this \
             session resolves to (`{spec}`) is not a Full spec in stage PLAN, so no plan \
             approval is pending for it. If another plan is the one you meant to approve, name \
             its row with `/mustard:spec <letter>r`. {tail}"
        )),
        PendingPlan::Outside { spec, already_approved: true } => Some(format!(
            "[approval] an approval was SELECTED, but NOTHING was recorded: `{spec}` is ALREADY \
             approved — its `pipeline.status{{to:approved}}` event is on the log — so there is \
             nothing left to record."
        )),
    }
}

/// Bound one quoted answer in the notice. A free-text answer can be an entire
/// message — the incident's was — and a hook's stderr is a diagnostic, not a
/// transcript.
fn truncate(s: &str) -> String {
    const MAX: usize = 80;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX).collect();
    format!("{head}…")
}

impl Observer for ApprovalMarkerObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = ctx.project_dir_or_cwd(input);

        // Fact 1 — an unapproved Full spec in PLAN. A miss no longer returns
        // right here: the answer is read first, because an answer that IS a
        // genuine approval turns this miss into the one thing worth saying.
        let pending = resolve_pending_plan(&cwd, input);

        // Facts 2 + 3 — a real SELECTION (one of the offered option labels,
        // exactly) that is affirmative. The `is_offered` filter runs BEFORE the
        // stem test, so free text is discarded on its shape and never reaches a
        // recogniser that only inspects its words. A decline is still fail-closed
        // (nothing is written), but it no longer happens in silence: the unmet
        // condition is named on stderr. Advisory only — an `eprintln!` is a pure
        // side-effect and can never turn this Observer into a verdict.
        let labels = selected_labels(input);
        let offered = offered_labels(input);
        let approved = labels
            .iter()
            .any(|l| is_offered(l, &offered) && is_affirmative(l));

        let PendingPlan::Awaiting(spec) = &pending else {
            // Nothing is awaiting approval. Only a genuine approval — offered,
            // selected, affirmative — is worth a word here; on anything else
            // this is one of the session's ordinary questions and silence is
            // the correct answer.
            if approved {
                if let Some(notice) = missing_plan_notice(&pending) {
                    eprintln!("{notice}");
                }
            }
            return;
        };

        if !approved {
            if let Some(notice) = unrecognised_answer_notice(spec, &labels, &offered) {
                eprintln!("{notice}");
            }
            return;
        }

        // All three facts hold → record the genuine approval, best-effort.
        if let Some(marker) = approval_marker_path(&cwd, spec) {
            let body = marker_body(
                spec,
                "AskUserQuestion",
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
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        }
    }

    /// A PostToolUse(AskUserQuestion) input whose `tool_input` offers `options`
    /// and whose `tool_response.answers` carries the user's answer — the shape
    /// the harness delivers, with the offered menu and the answer separated.
    fn ask_input_offering(session: &str, options: Value, answers: Value) -> HookInput {
        HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(session.to_string()),
            tool_input: json!({
                "questions": [{ "question": "Decision", "header": "Plan", "options": options }]
            }),
            raw: json!({ "tool_response": { "questions": [], "answers": answers } }),
            ..HookInput::default()
        }
    }

    /// The common case: every answer given was ALSO one of the offered options,
    /// i.e. the user picked from the menu.
    fn ask_input(session: &str, answers: Value) -> HookInput {
        let offered: Vec<Value> = match &answers {
            Value::Object(map) => map
                .values()
                .flat_map(|v| match v {
                    Value::String(s) => vec![json!({ "label": s })],
                    Value::Array(items) => {
                        items.iter().map(|i| json!({ "label": i })).collect()
                    }
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        ask_input_offering(session, json!(offered), answers)
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
        // The option WAS offered — so the stem is the condition that failed.
        let offered = labels.clone();
        let msg = unrecognised_answer_notice("epic", &labels, &offered)
            .expect("a real answer is explained");
        // The spec, the label that failed, and BOTH stems that would satisfy it.
        assert!(msg.contains("epic"), "names the spec: {msg}");
        assert!(msg.contains("Sim, pode ir"), "quotes the selected label: {msg}");
        assert!(msg.contains("approv") && msg.contains("aprov"), "names the stems: {msg}");
        // And that the consequence is nothing recorded, not something rejected.
        assert!(msg.contains(".approved-by-user"), "names the marker: {msg}");
    }

    /// The fact-1 decline SPEAKS, and says which half of fact 1 failed.
    ///
    /// Each of the three shapes has a different remedy, so each is named: the
    /// ladder resolved nothing, it resolved a spec outside the `full`+`Plan`
    /// window, or it resolved one whose approval already landed. The window that
    /// holds — an `Awaiting` — is not a failure and says nothing.
    #[test]
    fn a_fact_one_decline_names_its_reason() {
        let unresolved = missing_plan_notice(&PendingPlan::Unresolved)
            .expect("an approval with no plan to land on is explained");
        assert!(unresolved.contains("no spec could be resolved"), "{unresolved}");
        assert!(unresolved.contains("/mustard:spec <letter>r"), "names a remedy: {unresolved}");

        let outside = missing_plan_notice(&PendingPlan::Outside {
            spec: "shipped-already".to_string(),
            already_approved: false,
        })
        .expect("a spec outside the window is explained");
        assert!(outside.contains("shipped-already"), "names the spec: {outside}");
        assert!(outside.contains("stage PLAN"), "names the window: {outside}");

        let done = missing_plan_notice(&PendingPlan::Outside {
            spec: "epic".to_string(),
            already_approved: true,
        })
        .expect("an already-approved spec is explained");
        assert!(done.contains("ALREADY"), "names the condition: {done}");
        assert!(done.contains("epic"), "names the spec: {done}");

        assert_eq!(
            missing_plan_notice(&PendingPlan::Awaiting("epic".to_string())),
            None,
            "a plan that IS awaiting approval failed nothing",
        );
    }

    #[test]
    fn notice_stays_silent_on_a_dismissed_dialog() {
        // No answer was given, so no condition was failed — nothing to explain.
        assert_eq!(unrecognised_answer_notice("epic", &[], &[]), None);
    }

    #[test]
    fn notice_tells_free_text_apart_from_an_unrecognised_option() {
        // An answer nobody offered is free text, and the remedy is different:
        // select the option instead of typing it.
        let typed = vec!["Yes, approve it, go ahead".to_string()];
        let offered = vec!["Approve and implement now".to_string()];
        let msg = unrecognised_answer_notice("epic", &typed, &offered)
            .expect("free text is explained");
        assert!(msg.contains("free text"), "names the condition: {msg}");
        assert!(msg.contains("SELECTING"), "names the remedy: {msg}");
        assert!(msg.contains("Approve and implement now"), "shows the menu: {msg}");
        // A very long answer is bounded — stderr is a diagnostic, not a transcript.
        let essay = vec!["approve ".repeat(200)];
        let long = unrecognised_answer_notice("epic", &essay, &offered).unwrap();
        assert!(long.len() < 1200, "the quoted answer is bounded: {} chars", long.len());
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

    /// This door's half of the shared-body claim: what it minted reads back as
    /// typed provenance carrying an instant — a field only `marker_body` emits,
    /// so a door that went back to composing its own text fails here.
    #[test]
    fn marker_body_is_the_single_writer_for_ask_user_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-7", "epic");
        let input = ask_input("s-7", json!({ "Decision": "Aprovar e implementar agora" }));
        ApprovalMarkerObserver.observe(&input, &ctx(root.to_str().unwrap()));
        let marker = approval_marker_path(root.to_str().unwrap(), "epic").unwrap();
        let p = crate::shared::context::read_marker_provenance(&marker)
            .expect("the minted body must read back as provenance");
        assert_eq!(p.via, "AskUserQuestion");
        assert_eq!(p.spec, "epic");
        assert_eq!(p.session, "s-7");
        assert!(!p.at.is_empty(), "the door must record an instant");
    }

    /// AC-9 — the forged approval, closed in BOTH directions.
    ///
    /// Free text carrying approval words mints NOTHING, however emphatic; a
    /// genuine selection of an offered approval option still mints the marker
    /// exactly as before. The second half is what stops the fix from passing by
    /// making the recogniser inert.
    #[test]
    fn free_text_answer_never_mints_the_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        seed_spec(root, "epic", "full (wave plan)", "Plan");
        bind_session(root, "s-1", "epic");

        // The menu the model authored. The user typed instead of picking.
        let offered = json!([
            { "label": "Approve and implement now" },
            { "label": "Reject — re-plan" }
        ]);
        // The field shape: a long message that merely CONTAINS an approval word.
        let essay = "Here is the field report. The run stalled at approve-spec because the \
                     approval marker was missing, so nobody could approve the plan.";
        assert!(
            is_affirmative(essay),
            "the stem recogniser DOES fire on this text — the shape check is what must stop it"
        );
        ApprovalMarkerObserver.observe(
            &ask_input_offering("s-1", offered.clone(), json!({ "Decision": essay })),
            &ctx(root_str),
        );
        assert!(
            !marker_exists(root, "epic"),
            "free text must never mint the marker, whatever words it carries"
        );

        // Not even the exact option quoted back inside a longer sentence.
        ApprovalMarkerObserver.observe(
            &ask_input_offering(
                "s-1",
                offered.clone(),
                json!({ "Decision": "yes: Approve and implement now, please" }),
            ),
            &ctx(root_str),
        );
        assert!(!marker_exists(root, "epic"), "a quoted option inside prose is still free text");

        // Nor when the offered menu cannot be read at all (fail-closed).
        let mut blind = ask_input_offering("s-1", offered.clone(), json!({ "Decision": "Approve and implement now" }));
        blind.tool_input = json!({});
        ApprovalMarkerObserver.observe(&blind, &ctx(root_str));
        assert!(!marker_exists(root, "epic"), "no readable menu → nothing is proven offered");

        // The other direction — a genuine SELECTION of the offered approval
        // option still mints the marker, exactly as today.
        ApprovalMarkerObserver.observe(
            &ask_input_offering(
                "s-1",
                offered,
                json!({ "Decision": "Approve and implement now" }),
            ),
            &ctx(root_str),
        );
        assert!(
            marker_exists(root, "epic"),
            "a real selection of an offered approval label must still mint the marker"
        );
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

    /// **A stale hint no longer shadows the plan that IS pending.**
    ///
    /// The ladder used to stop at the first rung that answered ANYTHING, and the
    /// legacy `.pipeline-states/` sink keeps answering long after the spec it
    /// names has left PLAN. That obsolete guess reached rung two, ended the
    /// walk, and the third rung — written exactly for the case where the first
    /// two cannot answer — was never consulted, so a genuine approval minted
    /// nothing at all.
    ///
    /// Both halves are asserted: the pending plan collects the marker, and the
    /// stale spec collects nothing.
    #[test]
    fn a_stale_hint_never_shadows_the_pending_full_plan() {
        // The hint is read from `.pipeline-states/` only when no env override
        // is set; skip rather than flake on an ambient one.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();

        // The spec the stale hint names is a Full spec long past PLAN…
        seed_spec(root, "shipped-already", "full", "Execute");
        let states = root.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("shipped-already.json"), "{}").unwrap();
        // …while the plan actually awaiting approval is another one entirely,
        // with NO session binding — the shape the third rung exists for.
        seed_spec(root, "epic", "full (wave plan)", "Plan");

        assert_eq!(
            current_spec(root_str).as_deref(),
            Some("shipped-already"),
            "the fixture only proves something if the stale hint really answers",
        );
        assert_eq!(
            active_spec(root_str, &ask_input("s-unbound", json!({}))).as_deref(),
            Some("epic"),
            "the ladder walks past a hint that satisfies no fact-1 window",
        );

        let input = ask_input("s-unbound", json!({ "Approve?": "Aprovar e implementar agora" }));
        ApprovalMarkerObserver.observe(&input, &ctx(root_str));
        assert!(marker_exists(root, "epic"), "the pending plan is the one approved");
        assert!(
            !marker_exists(root, "shipped-already"),
            "and the stale hint's spec collects nothing",
        );
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
