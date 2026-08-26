//! `mustard-rt run approve-spec` — deterministic spec-approval event sequence.
//!
//! Replaces the hand-assembled `emit-pipeline` sequence the legacy approve
//! flow (now `plugin/refs/spec/resume-loop.md § A`) used to make the LLM run
//! by hand (emit `pipeline.stage Plan` then `pipeline.status
//! from:draft,to:approved`; patch the wave-1 `meta.json` for dispatch). The orchestrator now relays a
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
use std::path::Path;

use crate::commands::review::{ac_negative_check, qa_run};
use crate::shared::context::MarkerProvenance;

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
///
/// `markerVia` / `markerAt` echo the provenance the approval MARKER records —
/// the door the approval came through and when. They are named for the marker,
/// not for this run, because the marker may have been minted in an EARLIER
/// session: the previous `approvedVia` / `approvedAt` spelling read as *how the
/// current approval happened* while carrying *how some other session's approval
/// happened*, which is a silent wrong answer to anyone auditing who approved
/// what. Both are omitted (not null) when the marker's body is unreadable or
/// predates those keys: the marker EXISTENCE is what governed the gate above, so
/// a missing echo degrades the report to silence, never to a failure.
///
/// `approvedThisSession` is the separated half — whether the recorded gesture
/// belongs to the session this run is executing in. It is `false` whenever that
/// cannot be SHOWN (no marker, an unreadable body, a marker with no session),
/// so a run that performed no approval gesture never claims one.
#[derive(Debug, Serialize)]
struct ApproveReport {
    ok: bool,
    spec: String,
    approved: bool,
    resumed: bool,
    #[serde(rename = "markerVia", skip_serializing_if = "Option::is_none")]
    marker_via: Option<String>,
    #[serde(rename = "markerAt", skip_serializing_if = "Option::is_none")]
    marker_at: Option<String>,
    #[serde(rename = "approvedThisSession")]
    approved_this_session: bool,
}

/// `true` only when the approval gesture the marker records happened in `session`
/// — the session this very run is executing in.
///
/// Every uncertainty answers `false`: no marker, a body that does not read back,
/// a marker minted before the session key existed (empty `session`), or a
/// session neither side could resolve. Both doors write the literal `unknown`
/// when the harness gave them no session id, so `unknown == unknown` is NOT a
/// match — it is two absences, and reading it as one would resurrect exactly the
/// false claim this split exists to remove. A report may understate what it can
/// prove; it may never claim a gesture it cannot attribute to this run.
fn gesture_happened_here(prov: Option<&MarkerProvenance>, session: &str) -> bool {
    fn known(s: &str) -> bool {
        !s.trim().is_empty() && s.trim() != "unknown"
    }
    known(session) && prov.is_some_and(|p| known(&p.session) && p.session.trim() == session.trim())
}

/// The approval provenance to echo, read through the single reader in
/// [`crate::shared::context::read_marker_provenance`].
///
/// `None` when the marker is absent OR its body is unreadable — the gate in
/// [`run`] already decided approval from the marker's EXISTENCE, so this read is
/// purely informative and can never itself refuse a spec.
fn approval_provenance(cwd: &str, spec: &str) -> Option<MarkerProvenance> {
    let path = crate::shared::context::approval_marker_path(cwd, spec)?;
    crate::shared::context::read_marker_provenance(&path)
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
// The approval gate — clarify (F6) + user-approval (T5), evaluated together.
//
// `approve-spec` may emit the `draft→approved` signal ONLY once BOTH preconditions
// hold: a Full plan is CLARIFIED (`<spec>/.clarified`, F6) and a real user has
// APPROVED it (`<spec>/.approved-by-user`, T5). Each marker is born from an act the
// model cannot author — the deliberate clarification finalize, and the user's own
// `AskUserQuestion` / `ExitPlanMode` answer echoed in `tool_response`, or the picker
// form (`/mustard:spec {letter}r`, or `/mustard:spec r` inside the unit's own work
// branch) submitted as the whole prompt. A gate the gated could open by running this
// very command is not a gate.
//
// The two preconditions are checked in ONE pass so a single refusal names EVERY
// unmet marker with its minting path — the pre-refactor gate exited on the first
// miss and hid the second, costing the user a second failed run to discover it.
// Mode reads exactly like the `MUSTARD_*_GATE_MODE` close-gate family.
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

/// What the clarify marker says — the three states the gate distinguishes, plus
/// the case where clarify does not apply at all.
///
/// The middle state is the one this wave exists for: a marker that EXISTS but
/// recorded nothing. Minting it costs the orchestrator one command it can run
/// seconds before the approval the marker unlocks, so existence alone proves
/// only that the command ran.
///
/// `pub(crate)` because the DISCOVERY of a hollow marker was moved earlier than
/// the refusal: `active-specs` (the listing) and `resume-bootstrap` (the resume
/// path) read this same classifier to warn, so there is exactly one definition
/// of "hollow" in the crate. Those two callers are advisory — the refusal stays
/// here, in [`run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClarifyState {
    /// Not a Full spec — Light / task specs are never clarify-gated.
    NotGated,
    /// No `<spec>/.clarified` at all.
    Missing,
    /// The marker exists but names no captured term and states no reason.
    Hollow,
    /// The marker records what was settled — terms, or a stated reason.
    Recorded,
}

/// Read the clarify marker for `spec` under `root` and classify what it carries.
///
/// **Fail-CLOSED, deliberately.** Every other check in this crate degrades to
/// "allow" when a read fails — a hook must never block a session over its own
/// I/O error. This one is not a hook: it guards a VERDICT (the `draft→approved`
/// signal), and its whole job is to refuse when the clarification cannot be
/// shown to have happened. An unreadable or unparsable body therefore reads as
/// [`ClarifyState::Hollow`] and REFUSES, for the same reason an absent marker
/// already refused today. A reader who assumes the crate-wide fail-open rule
/// here would get this backwards, which is why it is spelled out.
///
/// Advisory callers (`active-specs`, `resume-bootstrap`) reuse this classifier
/// instead of re-deriving "hollow" — they only REPORT [`ClarifyState::Hollow`];
/// the fail-closed refusal lives in [`run`].
pub(crate) fn clarify_state(root: &str, spec: &str) -> ClarifyState {
    if !spec_is_full(root, spec) {
        return ClarifyState::NotGated;
    }
    let Some(path) = crate::shared::context::clarified_marker_path(root, spec) else {
        return ClarifyState::Missing;
    };
    if !path.exists() {
        return ClarifyState::Missing;
    }
    match crate::shared::context::read_marker_provenance(&path) {
        Some(p) if p.records_substance() => ClarifyState::Recorded,
        _ => ClarifyState::Hollow,
    }
}

/// What the proof ledger says about the spec's own acceptance criteria — the
/// THIRD approval precondition, beside clarify and user-approval.
///
/// The producer is `mustard-rt run ac-negative-check`, which runs each criterion
/// against the tree BEFORE the work exists and records the result in
/// `<spec>/ac-proof.json`. This door only READS that ledger; it never re-runs a
/// command, because the user is waiting at the approval gesture and the proofs
/// take minutes.
///
/// **Fail-CLOSED, deliberately** — the same exception [`clarify_state`] already
/// documents, for the same reason. The crate-wide rule is fail-open (a hook must
/// never block a session over its own IO error) and a reader WILL assume it
/// here. This is not a hook: it guards the `draft→approved` verdict, and an
/// absent, unreadable or unparsable ledger is precisely a proof that cannot be
/// shown to have been taken. It therefore REFUSES, exactly as an absent marker
/// already does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProofState {
    /// The spec declares no `## Acceptance Criteria` at all (or its markdown is
    /// not there to read). Nothing to prove, so nothing to refuse — this gate
    /// must not invent a refusal for a spec that made no claims.
    NotGated,
    /// Every non-exempt criterion carries evidence for the exact command it
    /// carries today — a RED proof, or a GREEN confirmation.
    Proven,
    /// No readable `<spec>/ac-proof.json` — the fail-closed case.
    LedgerUnreadable,
    /// One line per criterion without a proven record, each naming WHAT is
    /// wrong: a proof never taken versus a proof that came back green.
    Unproven(Vec<String>),
}

impl ProofState {
    /// `true` when the gate must refuse. The refusal is UNCONDITIONAL — the
    /// caller never softens it with `MUSTARD_APPROVAL_MODE`.
    fn refuses(&self) -> bool {
        !matches!(self, ProofState::NotGated | ProofState::Proven)
    }
}

/// A proof never taken — the wording for a criterion the ledger records nothing
/// for, including the hand edit this whole gate exists to close: a recorded
/// command that no longer matches the one the criterion carries today.
const WHY_NEVER_TAKEN: &str =
    "the proof was NEVER TAKEN for the command this criterion carries today";

/// A proof that WAS taken and came back the wrong colour. It asks for the
/// OPPOSITE action to [`WHY_NEVER_TAKEN`] — rewrite the command, do not re-run
/// it — so the two never share a wording.
const WHY_GREEN: &str = "the proof was TAKEN and the command came back GREEN, so the criterion \
     cannot tell done from not-done — rewrite the command instead of re-running it";

/// A proof that was taken but produced no verdict.
const WHY_NO_VERDICT: &str = "the proof was TAKEN but the command was killed by its deadline, \
     so no verdict ever arrived";

/// A proof the producer could not attempt at all.
const WHY_NOT_ATTEMPTED: &str =
    "the proof was NEVER TAKEN: the producer could not attempt the command at all";

/// The CONFIRMED column came back red — read, never re-run. It asks for the
/// work to be finished, which is the opposite of [`WHY_GREEN`]'s remedy.
const WHY_CONFIRMATION_RED: &str = "the confirmation was TAKEN and the command still came back \
     RED after its work landed, and no RED proof stands for it either";

/// The CONFIRMED column produced no verdict.
const WHY_CONFIRMATION_NO_VERDICT: &str = "the confirmation was TAKEN but the command was killed \
     by its deadline, so no verdict arrived, and no RED proof stands for it either";

/// The CONFIRMED column found the criterion unrunnable. Naming `ac-amend` is
/// the point: re-running is not the remedy for a command that cannot run.
const WHY_INEXECUTABLE: &str = "the confirmation found the command INEXECUTABLE after its work \
     landed, so the criterion itself is broken — repair it with `mustard-rt run ac-amend`, the \
     one door that accepts a passing replacement for this case";

/// The CONTROL column came back red — read, never re-run. It names the ONE
/// thing a red control settles: the command cannot match anything even where it
/// should, so no colour it produces is about the behaviour.
const WHY_CONTROL_RED: &str = "the CONTROL was TAKEN and came back red against the tree as it is, \
     so this criterion cannot match anything even where it should — repair the command (a broken \
     regex, a shell it cannot run under, a quoting error), then take the proof";

/// The CONTROL column produced no verdict.
const WHY_CONTROL_NO_VERDICT: &str = "the CONTROL was TAKEN but the command was killed by its \
     deadline, so nobody knows whether this criterion can match anything";

/// The CONTROL column could not be attempted at all.
const WHY_CONTROL_NOT_ATTEMPTED: &str = "the CONTROL was NEVER TAKEN: its command could not be \
     attempted at all, so nobody knows whether this criterion can match anything";

/// Why a recorded criterion satisfies neither column — the one action that
/// clears it, in the wording of whichever column last spoke.
///
/// The CONFIRMED column is consulted first because it is the later finding: a
/// criterion whose confirmation says the command is INEXECUTABLE is not fixed
/// by anything the red column suggests. Pure, total — no re-run happens here,
/// exactly as none happens for the red column.
fn why_unsatisfied(p: &ac_negative_check::AcProof) -> &'static str {
    use ac_negative_check::{Confirmation, Control, Proof};
    // The CONTROL is consulted BEFORE either other column, because it decides
    // whether they can be read at all: a criterion whose control is not green
    // cannot match anything even where it should, so its red proof and its
    // confirmation are both answers about the command's spelling. Read off the
    // ledger like everything else here — nothing is ever re-run at the approval
    // gesture, where the user is waiting.
    match p.control {
        Control::Red => return WHY_CONTROL_RED,
        Control::NoVerdict => return WHY_CONTROL_NO_VERDICT,
        Control::NotAttempted => return WHY_CONTROL_NOT_ATTEMPTED,
        // Declared-and-green, or not declared at all: the control has nothing
        // to say, and the columns below answer.
        Control::Green | Control::NotDeclared => {}
    }
    match p.confirmation {
        Confirmation::Red => WHY_CONFIRMATION_RED,
        Confirmation::NoVerdict => WHY_CONFIRMATION_NO_VERDICT,
        Confirmation::Inexecutable => WHY_INEXECUTABLE,
        // A green confirmation is evidence, so `evidenced()` already returned
        // above; a record reaching here has no confirmation to speak of, and
        // the red column answers.
        Confirmation::Green | Confirmation::NotTaken => match p.proof {
            Proof::Green => WHY_GREEN,
            Proof::NoVerdict => WHY_NO_VERDICT,
            Proof::NotAttempted => WHY_NOT_ATTEMPTED,
            // A red proof IS evidence, so this arm is likewise unreachable;
            // treat any such record as no record at all.
            Proof::Red => WHY_NEVER_TAKEN,
        },
    }
}

/// Read `<spec>/ac-proof.json` and classify the spec's acceptance criteria.
///
/// Reads the ledger through the type its producer defined
/// ([`ac_negative_check::AcProofLedger`], via [`ac_negative_check::load_ledger`])
/// and looks each criterion up by the producer's own rule
/// ([`ac_negative_check::recorded_proof`]: id AND command AND expect must all
/// still match). There is no second parser and no second lookup rule for this
/// file, so a hand-edited command silently keeping its old proof is impossible
/// by construction rather than by care.
///
/// BOTH columns of the record are read, through the producer's own
/// [`ac_negative_check::AcProof::evidenced`]: the RED proof (able to fail
/// before its work) or the GREEN confirmation (shown to actually pass after
/// it). Reading only the red column would deadlock the amendment door — a
/// criterion repaired for being INEXECUTABLE is accepted on a replacement that
/// PASSES, so its evidence lives in the confirmed column by construction.
/// Neither column is ever re-run here: the user is waiting at the approval
/// gesture and the proofs take minutes.
///
/// The criteria themselves come from the SHARED `qa_run` parser, so this gate
/// and QA cannot disagree about which criteria a spec declares. The trailing
/// criterion is exempt by [`ac_negative_check::is_exempt`] — the one positional
/// exemption in the crate, reused, not restated.
pub(crate) fn proof_state(root: &str, spec: &str) -> ProofState {
    let root_path = std::path::Path::new(root);
    // No markdown, or no criteria section: the spec claims nothing, so there is
    // nothing to prove. This is the ONE branch that is not fail-closed, and it
    // is not a degradation — it is the absence of an obligation.
    let Some(spec_file) = qa_run::spec_file_for(root_path, spec) else {
        return ProofState::NotGated;
    };
    let Ok(markdown) = mustard_core::io::fs::read_to_string(&spec_file) else {
        return ProofState::NotGated;
    };
    let items = qa_run::extract_ac_section(&markdown)
        .map(|section| qa_run::parse_ac_items(&section))
        .unwrap_or_default();
    if items.is_empty() {
        return ProofState::NotGated;
    }

    let ledger_path = spec_file
        .parent()
        .unwrap_or(root_path)
        .join(ac_negative_check::AC_PROOF_JSON);
    let Some(ledger) = ac_negative_check::load_ledger(&ledger_path) else {
        return ProofState::LedgerUnreadable;
    };

    let total = items.len();
    let mut unproven: Vec<String> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if ac_negative_check::is_exempt(index, total) {
            continue;
        }
        let why = match ac_negative_check::recorded_proof(
            &ledger,
            &item.id,
            &item.command,
            item.expect.as_deref(),
        ) {
            // BOTH readings must hold: the record must carry evidence AND its
            // control must not stand in the way of reading that evidence. They
            // are separate questions — a record can carry a red proof taken
            // when the control was still green (or absent) and a control that
            // has since failed, and answering only the first would approve on
            // evidence nobody can read any more.
            Some(p) if p.evidenced() && p.control_satisfied() => continue,
            Some(p) => why_unsatisfied(p),
            None => WHY_NEVER_TAKEN,
        };
        unproven.push(format!("{} — {why}", item.id));
    }
    if unproven.is_empty() {
        ProofState::Proven
    } else {
        ProofState::Unproven(unproven)
    }
}

/// Narrative sections of `spec.md` still holding the seeded placeholder, byte
/// for byte.
///
/// **The oracle is exact, not a judgement.** It compares each section's body
/// against the very string `spec-draft` seeds for it (`placeholder.fill_*` in
/// the i18n catalogue, in the spec's own language) — so it answers "was this
/// authored?", never "is this good?". A one-word section passes; only untouched
/// scaffold fails. Measuring writing quality is not this gate's business and
/// pretending otherwise would make it unpredictable.
///
/// Why it refuses at all: nothing in the pipeline read the narrative. A Full
/// spec could reach approval with `Por que agora.` under `## Contexto`, and the
/// waves inherit it — `agent-prompt-render` builds each wave's TASK out of this
/// file, so a hollow spec produces hollow prompts for every agent after it.
/// Reported from the field, and then reproduced by this very unit: a
/// `spec-draft --force` re-drafted the body from scratch, restoring the
/// placeholders, and nothing objected.
///
/// Returns the localised HEADINGS, so the refusal names what the reader sees.
/// Fail-open: an unreadable spec yields an empty list — an unreadable file is
/// the read gate's problem, not this one's.
fn scaffold_residue(root: &Path, spec: &str) -> Vec<String> {
    let Ok(body) = mustard_core::io::fs::read_to_string(
        root.join(".claude").join("spec").join(spec).join("spec.md"),
    ) else {
        return Vec::new();
    };
    let lang = mustard_core::ProjectConfig::load(root).i18n().lang;
    // Each section the draft seeds with a placeholder, by the heading key that
    // titles it and the placeholder key that fills it. `context` is the one
    // composite: the draft writes `{intent}.\n\n{fill_why_now}`, so the trailing
    // placeholder line is what marks it unauthored — the title alone is not
    // narrative.
    const SEEDED: &[(&str, &str)] = &[
        ("heading.spec.users", "placeholder.fill_beneficiary"),
        ("heading.spec.metric", "placeholder.fill_metric"),
        ("heading.spec.non_goals", "placeholder.fill_excluded"),
        ("heading.spec.files", "placeholder.fill_files"),
    ];
    let mut residue: Vec<String> = SEEDED
        .iter()
        .filter_map(|(heading_key, placeholder_key)| {
            let heading = mustard_core::translate(heading_key, lang);
            let placeholder = mustard_core::translate(placeholder_key, lang);
            let section = section_body(&body, heading)?;
            // Untouched when the placeholder is the ONLY prose under the
            // heading. A section the author added to still passes.
            (section == placeholder.trim()).then(|| heading.to_string())
        })
        .collect();
    let context_heading = mustard_core::translate("heading.spec.context", lang);
    let why_now = mustard_core::translate("placeholder.fill_why_now", lang);
    if let Some(section) = section_body(&body, context_heading) {
        // The seeded shape is the title echoed back plus the placeholder. An
        // author who wrote the section replaces that line; one who only kept
        // the echo has still explained nothing.
        if section.ends_with(why_now.trim()) {
            residue.push(context_heading.to_string());
        }
    }
    residue.sort();
    residue
}

/// The body between `## <heading>` and the next `##`, or `None` when the
/// heading is absent.
///
/// The heading must be a WHOLE LINE, not a substring. An unanchored search
/// matched `## Contexto` inside `## Contexto e Motivação`, and matched a
/// Decisions bullet that merely quotes `` `## Arquivos` `` before reaching the
/// real section — either way the extracted text is not the section, the
/// placeholder comparison fails, and a spec that is pure scaffold sails through
/// the gate (found in review). The same anchoring the render's section cutter
/// uses, for the same reason.
fn section_body(body: &str, heading: &str) -> Option<String> {
    let wanted = format!("## {heading}");
    let mut lines = body.lines();
    lines.by_ref().position(|l| l.trim_end() == wanted)?;
    let section: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
    Some(section.join("\n").trim().to_string())
}

/// Build the aggregated refusal that names EVERY unmet approval precondition at
/// once — clarify (F6, `<spec>/.clarified`) and/or user-approval (T5,
/// `<spec>/.approved-by-user`) — each with the path that mints it. One message so
/// the user sees everything missing in a single run, instead of the pre-refactor
/// gate's first-miss-only refusal. `spec` is interpolated into the clarify
/// finalize command. Returns `None` when nothing is missing — the caller then
/// proceeds silently. Surfaced as the report `error`; the flow relays
/// `{ok:false,error}` straight to the user.
///
/// A [`ClarifyState::Hollow`] marker earns its OWN wording: the remedy is not
/// "run the finalize" (it already ran) but "run it saying what it settled", so
/// the refusal names the grill to run or tells the caller to state a reason.
fn unmet_gate_message(
    spec: &str,
    clarify: ClarifyState,
    approval_missing: bool,
    proof: &ProofState,
    scaffold_sections: &[String],
) -> Option<String> {
    let mut unmet: Vec<String> = Vec::new();
    if !scaffold_sections.is_empty() {
        unmet.push(format!(
            "narrative — {} section(s) still hold the seeded placeholder, byte for byte: {}. \
             The spec would be approved describing nothing, and the waves inherit that: the \
             per-wave prompt is built from what this file says. Author each one, then \
             re-materialise with `mustard-rt run plan-materialize --spec-dir \
             .claude/spec/{spec} --plan <plan.json>`",
            scaffold_sections.len(),
            scaffold_sections.join(", "),
        ));
    }
    if clarify == ClarifyState::Missing {
        unmet.push(format!(
            "clarify — no `<spec>/.clarified`: run the clarification finalize \
             `mustard-rt run grill-capture --finalize --spec {spec} --term <term>` (once per \
             term the ANALYZE glossary grill confirmed), or state why no grill applied \
             `--reason \"<sentence>\"` — a complete glossary is a legitimate reason, so a \
             clear-glossary spec is never deadlocked"
        ));
    }
    if clarify == ClarifyState::Hollow {
        unmet.push(format!(
            "clarify — `<spec>/.clarified` recorded NOTHING: the marker exists but names no \
             captured term and states no reason, so it proves only that the finalize ran. \
             Re-run it saying what was settled: `mustard-rt run grill-capture --finalize \
             --spec {spec} --term <term>` for each term the ANALYZE glossary grill confirmed \
             (`mustard-rt run glossary-coverage --intent \"<intent>\" --context <CONTEXT.md>` \
             lists the uncovered ones), or `--reason \"<sentence>\"` stating why no grill \
             applied"
        ));
    }
    if approval_missing {
        unmet.push(
            "approval — no `<spec>/.approved-by-user`: THREE gestures mint it, and the model \
             can forge none of them — (1) the user accepts the plan in plan mode \
             (ExitPlanMode); (2) the user SELECTS the approval option of the approval \
             AskUserQuestion (fallback) — a label carrying `approv`/`aprov`, since free text \
             typed instead of selected mints nothing; (3) the user types `/mustard:spec \
             {letter}r` — or `/mustard:spec r` inside the unit's own work branch, where the \
             branch already names the unit — as the WHOLE prompt"
                .to_string(),
        );
    }
    match proof {
        ProofState::NotGated | ProofState::Proven => {}
        ProofState::LedgerUnreadable => unmet.push(format!(
            "proof — no readable `<spec>/{ledger}`: the acceptance criteria were never shown to \
             be ABLE to fail, so passing them would prove nothing. An absent, unreadable or \
             unparsable ledger refuses (fail-closed). Take the proof: \
             `mustard-rt run ac-negative-check --spec {spec}`",
            ledger = ac_negative_check::AC_PROOF_JSON,
        )),
        ProofState::Unproven(entries) => {
            let list = entries
                .iter()
                .map(|e| format!("    - {e}"))
                .collect::<Vec<_>>()
                .join("\n");
            unmet.push(format!(
                "proof — `<spec>/{ledger}` records no PROVEN result for the command these \
                 criteria carry today:\n{list}\n  Take the proof: \
                 `mustard-rt run ac-negative-check --spec {spec}`. A criterion whose proof came \
                 back GREEN is not fixed by re-running it — rewrite its command (or amend it) so \
                 it asserts the new behaviour, then take the proof again.",
                ledger = ac_negative_check::AC_PROOF_JSON,
            ));
        }
    }
    if unmet.is_empty() {
        return None;
    }
    let list = unmet
        .iter()
        .map(|u| format!("- {u}"))
        .collect::<Vec<_>>()
        .join("\n");
    // The relax hint is only honest when every unmet precondition is one the
    // mode actually governs. The proof is unconditional, so when it is the (or
    // a) blocker the message says so instead of pointing at a switch that will
    // not move it.
    let tail = if proof.refuses() {
        "The acceptance-criteria proof is UNCONDITIONAL — MUSTARD_APPROVAL_MODE does not relax it."
    } else {
        "To temporarily relax, set MUSTARD_APPROVAL_MODE=warn or off."
    };
    Some(format!(
        "approve-spec will not self-approve a Full plan (that is what the field incident \
         did). Unmet precondition(s):\n{list}\n{tail}"
    ))
}

/// `true` when `spec`'s `meta.json#scope` declares a Full-scope spec (starts with
/// `full` after a case-insensitive trim — `"full"` or `"full (wave plan)"`). Only
/// a Full plan carries the clarify gate; Light / task specs are never gated.
/// Fail-open: an unreadable `meta.json` / absent scope returns `false` (not
/// gated), the safe direction — the user-approval gate still applies to all.
fn spec_is_full(root: &str, spec: &str) -> bool {
    let Some(sp) = mustard_core::ClaudePaths::for_project(std::path::Path::new(root))
        .and_then(|p| p.for_spec(spec))
        .ok()
    else {
        return false;
    };
    mustard_core::read_meta(&sp.meta_json_path())
        .and_then(|m| m.scope)
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false)
}

/// Why no approval happened — the `error` to print, and whether the process must
/// exit non-zero.
///
/// A gate refusal exits 1 (a caller that ignores stdout still sees the failure);
/// an argument error (empty spec) stays exit 0, as it always has.
#[derive(Debug)]
struct Refused {
    error: String,
    exit_nonzero: bool,
}

/// Decide the approval against `root` and, only if it passes, feed every event
/// of the sequence to `emit`.
///
/// The seam that makes the gate testable: on a refusal `emit` is NEVER called,
/// which is the entire point of a gate, and a test can assert exactly that by
/// passing a recorder. `run` passes the canonical
/// [`crate::commands::event::emit_pipeline::run`].
///
/// Approval gate — refuse (strict) to emit the approval signal until BOTH
/// preconditions hold. Clarify (F6) precedes approval and applies only to Full
/// specs; user-approval (T5) applies to every spec. Both markers are born from
/// acts the model cannot forge (the deliberate `grill-capture --finalize`, and
/// the observer recording the user's real AskUserQuestion / ExitPlanMode answer).
/// A background job (no user, no answer, no marker) halts cleanly here and the
/// spec stays in PLAN instead of auto-approving. A THIRD precondition joins
/// them, unconditionally: every non-exempt acceptance criterion must carry a
/// PROVEN record in `<spec>/ac-proof.json` for the command it carries today (see
/// [`proof_state`] — fail-CLOSED). All three are evaluated TOGETHER so one
/// refusal names every unmet precondition with its remedy.
///
/// `session` is the session this run executes in, resolved by the caller like
/// `mode` is, so the report can say whether the recorded gesture happened HERE
/// without this routine reading process-global state.
fn approve_at(
    root: &str,
    opts: &ApproveSpecOpts,
    mode: ApprovalMode,
    session: &str,
    emit: &mut dyn FnMut(&str, Value),
) -> Result<ApproveReport, Refused> {
    if opts.spec.trim().is_empty() {
        return Err(Refused {
            error: "empty spec name".to_string(),
            exit_nonzero: false,
        });
    }

    // The acceptance-criteria proof is evaluated OUTSIDE the mode branch, on
    // purpose: it is unconditional. `MUSTARD_APPROVAL_MODE` governs the two
    // marker preconditions and nothing else — a gate blocks with no knob fork,
    // as the coverage gate in `plan-materialize` already does.
    let proof = proof_state(root, &opts.spec);

    // Clarify only gates Full specs, and gates them on what the marker CARRIES,
    // not merely that it is there. The spec is known explicitly here via
    // `--spec`, so there is no resolution problem (unlike the write-time
    // `scope_guard`). `off` mutes both marker preconditions; the proof stands.
    let (clarify, approval_missing) = if mode == ApprovalMode::Off {
        (ClarifyState::NotGated, false)
    } else {
        (
            clarify_state(root, &opts.spec),
            !crate::shared::context::approval_marker_path(root, &opts.spec)
                .map(|p| p.exists())
                .unwrap_or(false),
        )
    };

    // A single decision over all three preconditions: any unmet one yields the
    // aggregated message — never a second refusal path. An unmet PROOF always
    // Blocks; when only the marker preconditions are unmet,
    // `approval_gate(mode, false)` maps mode → Block (strict, exit 1) or Warn
    // (stderr nudge, proceed). Nothing unmet → the builder returns `None` and
    // the flow proceeds silently.
    let scaffold = scaffold_residue(Path::new(root), &opts.spec);
    if let Some(message) =
        unmet_gate_message(&opts.spec, clarify, approval_missing, &proof, &scaffold)
    {
        let gate = if proof.refuses() {
            ApprovalGate::Block
        } else {
            approval_gate(mode, false)
        };
        match gate {
            ApprovalGate::Block => {
                return Err(Refused {
                    error: message,
                    exit_nonzero: true,
                })
            }
            ApprovalGate::Warn => eprintln!("{message}"),
            ApprovalGate::Proceed => {}
        }
    }

    for (kind, payload) in approval_sequence(opts.wave_plan, opts.resume) {
        emit(kind, payload);
    }

    // Echo the provenance the approval MARKER recorded — named for the marker,
    // never for this run. Read AFTER the gate, and independent of it: an
    // unreadable body yields `None` here but never changes `approved`, which the
    // gate above already settled.
    let prov = approval_provenance(root, &opts.spec);
    Ok(ApproveReport {
        ok: true,
        spec: opts.spec.clone(),
        approved: true,
        resumed: opts.resume,
        marker_via: prov.as_ref().map(|p| p.via.clone()),
        marker_at: prov
            .as_ref()
            .map(|p| p.at.clone())
            .filter(|s| !s.is_empty()),
        approved_this_session: gesture_happened_here(prov.as_ref(), session),
    })
}

/// CLI entry — `mustard-rt run approve-spec --spec <name> [--wave-plan] [--resume]`.
///
/// Delegates the decision + emission to [`approve_at`] against the process cwd,
/// emitting each step through the canonical
/// [`crate::commands::event::emit_pipeline::run`] (process-global cwd, like
/// `wave_complete_observer`) — no subprocess, no duplicated NDJSON logic, no
/// facade. Prints the JSON report to stdout; a gate refusal exits 1 (a gate the
/// gated cannot open by running this very command), an empty spec name exits 0.
pub fn run(opts: ApproveSpecOpts) {
    let root = crate::shared::context::cwd();
    let spec = opts.spec.clone();
    let mut emit = |kind: &str, payload: Value| {
        crate::commands::event::emit_pipeline::run(
            crate::commands::event::emit_pipeline::EmitPipelineOpts {
                kind: kind.to_string(),
                spec: spec.clone(),
                payload: Some(payload.to_string()),
                allow_no_qa: false,
                intent: None,
                unit_name: None,
                base: None,
                work_kind: None,
            },
        );
    };

    let session = crate::shared::context::session_id();
    match approve_at(&root, &opts, resolve_approval_mode(), &session, &mut emit) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string(&report).unwrap_or_else(|_| "{\"ok\":true}".to_string())
        ),
        Err(refused) => {
            let err = ApproveError {
                ok: false,
                error: refused.error,
            };
            println!(
                "{}",
                serde_json::to_string(&err).unwrap_or_else(|_| "{\"ok\":false}".to_string())
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
            if refused.exit_nonzero {
                std::process::exit(1);
            }
        }
    }
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

    /// AC-11 — a spec whose narrative is still the seeded placeholder is
    /// refused, and the refusal names the sections.
    ///
    /// The oracle is exact: it compares each section against the very string
    /// the draft seeds. Authoring one word clears it — this gate answers "was
    /// this written?", never "is this good?".
    #[test]
    fn approval_refuses_scaffold_residue() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0","specLang":"pt-BR"}"#)
            .unwrap();
        let spec_dir = root.join(".claude/spec/uma-unidade");
        std::fs::create_dir_all(&spec_dir).unwrap();

        // Straight out of the draft: every PRD section is its placeholder.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# t\n\n## Contexto\n\nfazer algo.\n\nPor que agora.\n\n\
             ## Usuários/Stakeholders\n\nQuem se beneficia.\n\n\
             ## Métrica de sucesso\n\nMétrica de sucesso.\n\n\
             ## Não-Objetivos\n\nO que fica de fora.\n\n## Arquivos\n\nListar arquivos afetados.\n",
        )
        .unwrap();

        let residue = scaffold_residue(root, "uma-unidade");
        assert_eq!(residue.len(), 5, "every seeded section is residue: {residue:?}");
        assert!(residue.iter().any(|s| s == "Contexto"), "{residue:?}");

        let msg = unmet_gate_message(
            "uma-unidade",
            ClarifyState::NotGated,
            false,
            &ProofState::NotGated,
            &residue,
        )
        .expect("scaffold residue must refuse");
        assert!(msg.contains("Contexto"), "the refusal must name the sections: {msg}");
        assert!(msg.contains("plan-materialize"), "the refusal must name its remedy: {msg}");

        // Authored: the sections are replaced, and the gate goes quiet.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# t\n\n## Contexto\n\nO roteador nao alcanca a janela.\n\n\
             ## Usuários/Stakeholders\n\nO operador.\n\n\
             ## Métrica de sucesso\n\nNenhum caminho sem roteador.\n\n\
             ## Não-Objetivos\n\nMedir qualidade de texto.\n\n## Arquivos\n\n- `a.rs`\n",
        )
        .unwrap();
        assert!(
            scaffold_residue(root, "uma-unidade").is_empty(),
            "an authored spec must not be refused",
        );

        // Fail-open: an unreadable spec is the read gate's problem, not this one's.
        assert!(scaffold_residue(root, "nao-existe").is_empty());

        // A heading is a WHOLE LINE. `## Contexto e Motivação` is not
        // `## Contexto`, and a bullet quoting a heading is not that heading —
        // an unanchored search matched both and read the wrong text as the
        // section, so a pure-scaffold spec passed the gate.
        std::fs::write(
            spec_dir.join("spec.md"),
            concat!(
                "# t\n",
                "\n",
                "## Contexto e Motivação\n",
                "\n",
                "Por que agora.\n",
                "\n",
                "## Decisions\n",
                "\n",
                "- a secao `## Arquivos` recebe a lista\n",
                "\n",
                "## Arquivos\n",
                "\n",
                "Listar arquivos afetados.\n",
            ),
        )
        .unwrap();
        let residue = scaffold_residue(root, "uma-unidade");
        assert!(
            residue.iter().any(|s| s == "Arquivos"),
            "the real `## Arquivos` section is still placeholder and must be caught, \
             not shadowed by the bullet that quotes its name: {residue:?}",
        );
        assert!(
            !residue.iter().any(|s| s == "Contexto"),
            "`## Contexto e Motivação` is a different heading and has no placeholder \
             to report: {residue:?}",
        );
    }

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

    // -----------------------------------------------------------------------
    // F6 — the clarify gate (clarify precedes approval)
    // -----------------------------------------------------------------------

    #[test]
    fn spec_is_full_reads_meta_scope() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        for (spec, scope, want) in [
            ("epic", "full (wave plan)", true),
            ("epic2", "full", true),
            ("small", "light", false),
        ] {
            let sp = mustard_core::ClaudePaths::for_project(root)
                .unwrap()
                .for_spec(spec)
                .unwrap();
            std::fs::create_dir_all(sp.dir()).unwrap();
            std::fs::write(
                sp.meta_json_path(),
                format!(r#"{{"scope":"{scope}","stage":"Plan","outcome":"Active"}}"#),
            )
            .unwrap();
            assert_eq!(spec_is_full(root_str, spec), want, "scope={scope}");
        }
        // No meta.json → not full (fail-open: the clarify gate does not apply).
        assert!(!spec_is_full(root_str, "ghost"));
    }

    /// Seed a Full spec dir + `meta.json` so `spec_is_full` says yes, and return
    /// the project root string.
    fn seed_full_spec(root: &Path, spec: &str) {
        let sp = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec(spec)
            .unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.meta_json_path(),
            r#"{"scope":"full (wave plan)","stage":"Plan","outcome":"Active"}"#,
        )
        .unwrap();
    }

    #[test]
    fn clarify_gate_blocks_full_without_marker_and_proceeds_with_a_record() {
        // Mirrors the user-approval gate, for the F6 clarify precondition —
        // except the clarify marker must CARRY something: absent → Missing, a
        // body that recorded nothing → Hollow, a recorded term → Recorded.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        seed_full_spec(root, spec);
        let marker = crate::shared::context::clarified_marker_path(root_str, spec)
            .expect("clarified marker path resolves");
        assert!(marker.ends_with(".clarified"));

        assert_eq!(clarify_state(root_str, spec), ClarifyState::Missing);
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);

        // The legacy body — the finalize ran, and said nothing.
        std::fs::write(&marker, b"spec=epic\nvia=grill-finalize\n").unwrap();
        assert_eq!(clarify_state(root_str, spec), ClarifyState::Hollow);

        std::fs::write(
            &marker,
            crate::shared::context::clarify_marker_body(
                spec,
                "s-1",
                "2026-07-25T10:00:00.000Z",
                &["Payable".to_string()],
                "",
            ),
        )
        .unwrap();
        assert_eq!(clarify_state(root_str, spec), ClarifyState::Recorded);
    }

    #[test]
    fn light_spec_is_not_clarify_gated() {
        // A Light spec: `spec_is_full` is false, so `run` skips the clarify gate
        // entirely — approval never requires `.clarified` for a Light / task spec.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let sp = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec("small")
            .unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.meta_json_path(),
            r#"{"scope":"light","stage":"Plan","outcome":"Active"}"#,
        )
        .unwrap();
        assert!(
            !spec_is_full(root_str, "small"),
            "a Light spec is not full → the clarify gate is skipped"
        );
    }

    // -----------------------------------------------------------------------
    // Combined gate — one refusal names EVERY unmet marker (clarify + approval)
    // -----------------------------------------------------------------------

    #[test]
    fn combined_refusal_lists_all_missing_gates() {
        // Both markers absent → the single refusal names BOTH, each with its own
        // minting path, instead of exiting on the first miss and hiding the second.
        let msg = unmet_gate_message("epic", ClarifyState::Missing, true, &ProofState::NotGated, &[])
            .expect("both missing → a refusal");
        assert!(msg.contains(".clarified"), "names the clarify marker: {msg}");
        assert!(
            msg.contains("grill-capture --finalize --spec epic"),
            "names the clarify minting path with the spec: {msg}"
        );
        assert!(msg.contains(".approved-by-user"), "names the approval marker: {msg}");
        assert!(
            msg.contains("ExitPlanMode") && msg.contains("AskUserQuestion"),
            "names the approval minting path: {msg}"
        );
        // Strict refuses (Block ⇒ exit≠0) when a marker is missing.
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    #[test]
    fn clarify_only_missing_refuses_with_single_requirement() {
        // Clarify absent, approval present → refuse, but name ONLY the clarify
        // requirement (no stray approval line).
        let msg = unmet_gate_message("epic", ClarifyState::Missing, false, &ProofState::NotGated, &[])
            .expect("clarify missing → a refusal");
        assert!(msg.contains(".clarified"), "names the clarify marker: {msg}");
        assert!(
            msg.contains("grill-capture --finalize --spec epic"),
            "names the clarify minting path: {msg}"
        );
        assert!(
            !msg.contains(".approved-by-user"),
            "must NOT name the met approval marker: {msg}"
        );
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    #[test]
    fn approval_only_missing_refuses_with_single_requirement() {
        // Approval absent, clarify present (or not a Full spec) → refuse, but name
        // ONLY the approval requirement.
        let msg = unmet_gate_message("epic", ClarifyState::Recorded, true, &ProofState::NotGated, &[])
            .expect("approval missing → a refusal");
        assert!(msg.contains(".approved-by-user"), "names the approval marker: {msg}");
        assert!(
            msg.contains("ExitPlanMode") && msg.contains("AskUserQuestion"),
            "names the approval minting path: {msg}"
        );
        assert!(
            !msg.contains(".clarified"),
            "must NOT name the met clarify marker: {msg}"
        );
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    /// The refusal is read by someone who has just discovered their gesture did
    /// not count, so it is the one place the WORKING gestures have to be written
    /// down — all of them.
    ///
    /// It used to name two doors and there are three: the picker form
    /// (`crate::hooks::observe::picker_approval_observer`) mints the same marker
    /// from a prompt the user typed in full, and a refusal that omits it sends
    /// the operator back through a plan-mode round trip they had a one-line way
    /// out of. The two conditions the sibling doors decline on silently are
    /// named too, because the refusal is where they become actionable: an
    /// approval option whose label carries no `approv`/`aprov` stem, and an
    /// answer typed as free text rather than selected.
    #[test]
    fn the_refusal_names_the_gestures_that_actually_mint() {
        let msg = unmet_gate_message("epic", ClarifyState::Recorded, true, &ProofState::NotGated, &[])
            .expect("approval missing → a refusal");

        // 1. Plan mode.
        assert!(msg.contains("ExitPlanMode"), "the plan-mode gesture is unnamed: {msg}");
        // 2. The modal — and the two conditions its recorder declines on.
        assert!(msg.contains("AskUserQuestion"), "the modal gesture is unnamed: {msg}");
        assert!(
            msg.contains("SELECT") && msg.contains("free text"),
            "the modal gesture must say that only a SELECTED option counts, or the \
             operator retypes the same free text: {msg}"
        );
        assert!(
            msg.contains("approv") && msg.contains("aprov"),
            "the modal gesture must name the label stems its recorder matches: {msg}"
        );
        // 3. The picker — both spellings, under the whole-prompt rule that is the
        //    only thing keeping the form unforgeable.
        assert!(
            msg.contains("/mustard:spec {letter}r") && msg.contains("/mustard:spec r"),
            "the picker gestures are unnamed — the operator's one-line way out is \
             missing from the message that explains what to do instead: {msg}"
        );
        assert!(
            msg.contains("WHOLE prompt"),
            "naming the picker form without its whole-prompt rule teaches a gesture \
             that mints nothing when quoted inside a sentence: {msg}"
        );
        assert!(
            msg.contains("work branch"),
            "the bare `r` must carry the position it is valid in — outside a unit's \
             branch it names no spec at all: {msg}"
        );
    }

    #[test]
    fn both_present_approves() {
        // Neither precondition unmet → no refusal message, and strict proceeds.
        assert_eq!(
            unmet_gate_message("epic", ClarifyState::Recorded, false, &ProofState::Proven, &[]),
            None
        );
        // A Light spec skips clarify entirely — same silence.
        assert_eq!(
            unmet_gate_message("small", ClarifyState::NotGated, false, &ProofState::NotGated, &[]),
            None
        );
        assert_eq!(approval_gate(ApprovalMode::Strict, true), ApprovalGate::Proceed);
    }

    // -----------------------------------------------------------------------
    // The marker must CARRY something — a hollow `.clarified` refuses
    // -----------------------------------------------------------------------

    /// The input for which the gate now says no: a marker holding only the spec
    /// name. Approval reports `ok:false` with a reason naming the remedy, and —
    /// the load-bearing half — NOT ONE approval event is emitted.
    #[test]
    fn approve_refuses_a_marker_that_recorded_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        seed_full_spec(root, spec);

        // The user really approved — only the clarification is hollow, so the
        // refusal can come from nothing else.
        std::fs::write(
            crate::shared::context::approval_marker_path(root_str, spec).unwrap(),
            crate::shared::context::marker_body(
                spec,
                "ExitPlanMode",
                "s-1",
                "2026-07-25T10:00:00.000Z",
            ),
        )
        .unwrap();
        let clarified = crate::shared::context::clarified_marker_path(root_str, spec).unwrap();
        std::fs::write(&clarified, b"spec=epic\n").unwrap();

        let opts = ApproveSpecOpts {
            spec: spec.to_string(),
            wave_plan: true,
            resume: false,
        };
        // A recorder in place of the real emitter — `RefCell` so the log can be
        // inspected between the two `approve_at` calls below.
        let emitted: std::cell::RefCell<Vec<(String, Value)>> = std::cell::RefCell::new(Vec::new());
        let mut record =
            |kind: &str, payload: Value| emitted.borrow_mut().push((kind.to_string(), payload));

        let refused = approve_at(root_str, &opts, ApprovalMode::Strict, "s-1", &mut record)
            .err()
            .expect("a marker that recorded nothing must refuse the approval");
        assert!(refused.exit_nonzero, "a gate refusal exits non-zero");
        assert!(
            refused.error.contains(".clarified") && refused.error.contains("recorded NOTHING"),
            "the refusal names the hollow marker: {}",
            refused.error
        );
        assert!(
            refused.error.contains("--term") && refused.error.contains("--reason"),
            "the refusal names the remedy — run the grill, or state a reason: {}",
            refused.error
        );
        assert!(
            emitted.borrow().is_empty(),
            "the approval events must NOT be emitted: {:?}",
            emitted.borrow()
        );

        // The report the caller sees.
        let err = ApproveError {
            ok: false,
            error: refused.error,
        };
        let json: Value = serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(json["ok"], false);

        // Recording what was settled is what unblocks it — same spec, same
        // markers, one honest line added.
        std::fs::write(
            &clarified,
            crate::shared::context::clarify_marker_body(
                spec,
                "s-1",
                "2026-07-25T10:00:00.000Z",
                &[],
                "the glossary already defines every matched term",
            ),
        )
        .unwrap();
        let report = approve_at(root_str, &opts, ApprovalMode::Strict, "s-1", &mut record)
            .expect("a recorded clarification approves");
        assert!(report.approved);
        assert_eq!(
            emitted.borrow().len(),
            2,
            "plan + approved: {:?}",
            emitted.borrow()
        );
    }

    // -----------------------------------------------------------------------
    // Provenance echo — the report surfaces the door + instant the marker holds
    // -----------------------------------------------------------------------

    #[test]
    fn approve_spec_echoes_provenance() {
        // A marker written by the shared body writer reads back as the door and
        // the instant, and the report serialises both under stable keys.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "epic";
        std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
        let marker =
            crate::shared::context::approval_marker_path(root.to_str().unwrap(), spec).unwrap();
        std::fs::write(
            &marker,
            crate::shared::context::marker_body(
                spec,
                "AskUserQuestion",
                "s-1",
                "2026-07-24T10:00:00.000Z",
            ),
        )
        .unwrap();

        let prov = approval_provenance(root.to_str().unwrap(), spec).expect("provenance reads back");
        assert_eq!(prov.via, "AskUserQuestion");
        assert_eq!(prov.at, "2026-07-24T10:00:00.000Z");

        let report = ApproveReport {
            ok: true,
            spec: spec.to_string(),
            approved: true,
            resumed: false,
            marker_via: Some(prov.via.clone()),
            marker_at: Some(prov.at.clone()),
            approved_this_session: gesture_happened_here(Some(&prov), "s-1"),
        };
        let json: Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        // The keys name the MARKER, which is what they hold.
        assert_eq!(json["markerVia"], "AskUserQuestion");
        assert_eq!(json["markerAt"], "2026-07-24T10:00:00.000Z");
        assert_eq!(json["approvedThisSession"], true, "the marker names s-1: {json}");
    }

    /// AC-8 — the report must not answer "how did the CURRENT approval happen?"
    /// with a gesture some EARLIER session performed. The marker's provenance is
    /// published under keys that name the marker, and the separated
    /// `approvedThisSession` says plainly that this run performed nothing.
    ///
    /// Two-sided: a marker minted by THIS session still reports `true`, so the
    /// fix cannot pass by making the field permanently false.
    #[test]
    fn report_does_not_claim_a_gesture_that_did_not_happen() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "epic";
        std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
        let marker =
            crate::shared::context::approval_marker_path(root.to_str().unwrap(), spec).unwrap();
        // The user approved in session `s-earlier`; this run is `s-now`.
        std::fs::write(
            &marker,
            crate::shared::context::marker_body(
                spec,
                "ExitPlanMode",
                "s-earlier",
                "2026-07-20T09:00:00.000Z",
            ),
        )
        .unwrap();
        let prov = approval_provenance(root.to_str().unwrap(), spec).expect("provenance reads back");

        assert!(
            !gesture_happened_here(Some(&prov), "s-now"),
            "an earlier session's gesture is not this run's"
        );
        let report = ApproveReport {
            ok: true,
            spec: spec.to_string(),
            approved: true,
            resumed: false,
            marker_via: Some(prov.via.clone()),
            marker_at: Some(prov.at.clone()),
            approved_this_session: gesture_happened_here(Some(&prov), "s-now"),
        };
        let json: Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(json["approvedThisSession"], false, "{json}");
        // The provenance is still published — under a key that names the marker,
        // not one that reads as how this run approved.
        assert_eq!(json["markerVia"], "ExitPlanMode", "{json}");
        assert!(
            json.get("approvedVia").is_none(),
            "no field claims the current run performed the gesture: {json}"
        );

        // The other direction — a gesture that DID happen here reports so.
        assert!(gesture_happened_here(Some(&prov), "s-earlier"));
        // And every absence answers false, never true by coincidence: two
        // unresolved sessions are two absences, not a match.
        assert!(!gesture_happened_here(None, "s-earlier"));
        let anonymous = crate::shared::context::MarkerProvenance {
            session: "unknown".to_string(),
            ..prov.clone()
        };
        assert!(
            !gesture_happened_here(Some(&anonymous), "unknown"),
            "`unknown` == `unknown` is two absences, not an attribution"
        );
    }

    #[test]
    fn unreadable_marker_body_still_approves() {
        // The marker EXISTS (so the gate proceeds) but its body has no `via=`
        // line: the provenance read degrades to `None` and the echoed keys are
        // omitted — never a rejection, and never a null key.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "epic";
        std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
        let marker =
            crate::shared::context::approval_marker_path(root.to_str().unwrap(), spec).unwrap();
        std::fs::write(&marker, b"garbage with no via line\n").unwrap();

        // Existence still governs the gate.
        assert!(marker.exists());
        assert_eq!(
            approval_gate(ApprovalMode::Strict, marker.exists()),
            ApprovalGate::Proceed
        );
        // But the provenance read degrades to nothing.
        assert!(approval_provenance(root.to_str().unwrap(), spec).is_none());

        // The report a degraded read produces: approved, with the echo keys
        // simply absent (skip_serializing_if), not present-and-null.
        let report = ApproveReport {
            ok: true,
            spec: spec.to_string(),
            approved: true,
            resumed: false,
            marker_via: None,
            marker_at: None,
            approved_this_session: gesture_happened_here(None, "s-1"),
        };
        let json: Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(json["approved"], true);
        assert!(json.get("markerVia").is_none(), "no null key: {json}");
        assert!(json.get("markerAt").is_none(), "no null key: {json}");
        assert_eq!(
            json["approvedThisSession"], false,
            "a body that does not read back cannot attribute the gesture: {json}"
        );
    }

    // -----------------------------------------------------------------------
    // The THIRD precondition — every criterion must carry a PROVEN record
    // -----------------------------------------------------------------------

    /// Seed a spec's `spec.md` with two criteria: `AC-1` carrying `command`, and
    /// a trailing build-green criterion (exempt by position).
    fn seed_criteria(root: &Path, spec: &str, command: &str) {
        let sp = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec(spec)
            .unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.spec_md_path(),
            format!(
                "# S\n\n## Acceptance Criteria\n\
                 - **AC-1** — when the work lands, then the behaviour holds.\n  Command: `{command}`\n\
                 - **AC-2** — build green.\n  Command: `cargo build --workspace`\n"
            ),
        )
        .unwrap();
    }

    /// Write a proof ledger recording `AC-1` as PROVEN for `command`.
    fn seed_proof_ledger(root: &Path, spec: &str, command: &str) {
        let sp = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec(spec)
            .unwrap();
        let body = serde_json::json!({
            "spec": spec,
            "criteria": [{
                "id": "AC-1",
                "command": command,
                "expect": null,
                "verdict": "proven",
                "proof": "red",
                "exit": 1,
                "reason": null,
                "stderr_excerpt": ""
            }],
            "amendments": []
        });
        std::fs::write(
            sp.dir().join("ac-proof.json"),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
    }

    /// AC-4 — a criterion whose current command has no PROVEN record refuses,
    /// and the refusal names that criterion inside the SAME aggregated message
    /// (never a second refusal path). Driven through the hand edit this gate
    /// exists to close: the ledger proves the OLD command, the spec now carries
    /// a different one.
    #[test]
    fn approval_refuses_a_criterion_with_no_recorded_proof() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        seed_full_spec(root, spec);
        seed_criteria(root, spec, "cargo test -p mustard-rt --lib the_new_unit");

        // Both marker preconditions are genuinely met, so the refusal can come
        // from nothing but the proof.
        std::fs::write(
            crate::shared::context::approval_marker_path(root_str, spec).unwrap(),
            crate::shared::context::marker_body(spec, "ExitPlanMode", "s-1", "2026-07-25T10:00:00.000Z"),
        )
        .unwrap();
        std::fs::write(
            crate::shared::context::clarified_marker_path(root_str, spec).unwrap(),
            crate::shared::context::clarify_marker_body(
                spec,
                "s-1",
                "2026-07-25T10:00:00.000Z",
                &[],
                "the glossary already defines every matched term",
            ),
        )
        .unwrap();

        // No ledger at all → fail-CLOSED, and the message says so.
        assert_eq!(proof_state(root_str, spec), ProofState::LedgerUnreadable);

        let opts = ApproveSpecOpts { spec: spec.to_string(), wave_plan: true, resume: false };
        let emitted: std::cell::RefCell<Vec<(String, Value)>> = std::cell::RefCell::new(Vec::new());
        let mut record =
            |kind: &str, payload: Value| emitted.borrow_mut().push((kind.to_string(), payload));

        let refused = approve_at(root_str, &opts, ApprovalMode::Strict, "s-1", &mut record)
            .expect_err("no proof → the approval refuses");
        assert!(refused.exit_nonzero, "a gate refusal exits non-zero");
        assert!(
            refused.error.contains("ac-proof.json") && refused.error.contains("ac-negative-check"),
            "the refusal names the ledger and the command that mints it: {}",
            refused.error
        );
        assert!(
            emitted.borrow().is_empty(),
            "no approval event may be emitted: {:?}",
            emitted.borrow()
        );

        // A ledger that proves the criterion's OLD command is NO proof for the
        // one it carries today — precisely the hand edit this gate closes.
        seed_proof_ledger(root, spec, "cargo test -p mustard-rt --lib the_old_unit");
        let stale = proof_state(root_str, spec);
        let ProofState::Unproven(entries) = &stale else {
            panic!("a superseded command must read as unproven: {stale:?}");
        };
        assert_eq!(entries.len(), 1, "only AC-1 is unproven: {entries:?}");
        assert!(entries[0].starts_with("AC-1 —"), "names the criterion: {entries:?}");
        assert!(entries[0].contains("NEVER TAKEN"), "and what is wrong: {entries:?}");

        let refused = approve_at(root_str, &opts, ApprovalMode::Strict, "s-1", &mut record)
            .expect_err("a stale record is not a proof");
        assert!(
            refused.error.contains("AC-1"),
            "the criterion is named inside the aggregated refusal: {}",
            refused.error
        );
        // ONE refusal, carrying the standing preamble — not a second path.
        assert!(
            refused.error.contains("Unmet precondition(s):"),
            "the proof joins the existing aggregated message: {}",
            refused.error
        );
        assert!(
            refused.error.contains("UNCONDITIONAL"),
            "and says the mode does not relax it: {}",
            refused.error
        );

        // The other direction — proving the command it carries TODAY approves.
        seed_proof_ledger(root, spec, "cargo test -p mustard-rt --lib the_new_unit");
        assert_eq!(proof_state(root_str, spec), ProofState::Proven);
        let report = approve_at(root_str, &opts, ApprovalMode::Strict, "s-1", &mut record)
            .expect("a proven criterion approves");
        assert!(report.approved);
        assert_eq!(emitted.borrow().len(), 2, "plan + approved: {:?}", emitted.borrow());
    }

    /// The gate reads the CONFIRMED column too, and re-runs nothing to do it.
    ///
    /// The record under test is the one the amendment door writes when it
    /// repairs an INEXECUTABLE criterion: the red column says GREEN (the
    /// replacement passes, which is the point) and the evidence lives in the
    /// confirmation. Reading only the red column would refuse it forever.
    ///
    /// Two-sided: the same record with the confirmation NOT taken is refused,
    /// and the refusal names the confirmed column's own finding — so the gate
    /// cannot pass by accepting any record that merely mentions a confirmation.
    #[test]
    fn approval_reads_the_confirmation_column() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        let command = "cargo test -p mustard-rt --lib the_repaired_unit";
        seed_full_spec(root, spec);
        seed_criteria(root, spec, command);
        let ledger_path = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec(spec)
            .unwrap()
            .dir()
            .join("ac-proof.json");

        // A record whose RED column is green and whose CONFIRMED column is not
        // taken: no evidence at all, refused — and the wording comes from the
        // red column, because that is the only one that spoke.
        let record = |confirmation: &str| {
            serde_json::json!({
                "spec": spec,
                "criteria": [{
                    "id": "AC-1", "command": command, "expect": null,
                    "verdict": "unproven", "proof": "green", "confirmation": confirmation,
                    "exit": 0, "confirmation_exit": 0, "reason": null, "stderr_excerpt": ""
                }],
                "amendments": []
            })
        };
        std::fs::write(&ledger_path, record("not-taken").to_string()).unwrap();
        let ProofState::Unproven(entries) = proof_state(root_str, spec) else {
            panic!("a record with neither column is not evidence");
        };
        assert!(entries[0].contains("came back GREEN"), "{entries:?}");

        // The confirmed column found the criterion INEXECUTABLE: still refused,
        // and now the refusal names the amendment door instead of a re-run.
        std::fs::write(&ledger_path, record("inexecutable").to_string()).unwrap();
        let ProofState::Unproven(entries) = proof_state(root_str, spec) else {
            panic!("an inexecutable criterion is not evidence");
        };
        assert!(entries[0].contains("INEXECUTABLE"), "{entries:?}");
        assert!(entries[0].contains("ac-amend"), "names the repair door: {entries:?}");

        // And the accepted direction: a GREEN confirmation IS evidence.
        std::fs::write(&ledger_path, record("green").to_string()).unwrap();
        assert_eq!(
            proof_state(root_str, spec),
            ProofState::Proven,
            "a criterion shown to actually pass after its work satisfies the gate"
        );

        // A ledger written before the confirmed column existed still reads as
        // the truth about it — nobody asked — so the red column governs alone.
        let legacy = serde_json::json!({
            "spec": spec,
            "criteria": [{
                "id": "AC-1", "command": command, "expect": null,
                "verdict": "proven", "proof": "red", "exit": 1,
                "reason": null, "stderr_excerpt": ""
            }],
            "amendments": []
        });
        std::fs::write(&ledger_path, legacy.to_string()).unwrap();
        assert_eq!(proof_state(root_str, spec), ProofState::Proven);
    }

    /// The proof precondition is UNCONDITIONAL, and it invents no refusal for a
    /// spec that declares no criteria.
    #[test]
    fn the_proof_precondition_ignores_the_mode_and_spares_a_spec_with_no_criteria() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        seed_full_spec(root, spec);
        seed_criteria(root, spec, "cargo test -p mustard-rt --lib the_new_unit");

        let opts = ApproveSpecOpts { spec: spec.to_string(), wave_plan: false, resume: false };
        let emitted: std::cell::RefCell<Vec<(String, Value)>> = std::cell::RefCell::new(Vec::new());
        let mut record =
            |kind: &str, payload: Value| emitted.borrow_mut().push((kind.to_string(), payload));

        // `off` mutes the two MARKER preconditions; the proof still refuses.
        for mode in [ApprovalMode::Off, ApprovalMode::Warn, ApprovalMode::Strict] {
            let refused = approve_at(root_str, &opts, mode, "s-1", &mut record)
                .err()
                .unwrap_or_else(|| panic!("{mode:?} must not relax the proof"));
            assert!(refused.exit_nonzero, "{mode:?}");
        }
        assert!(emitted.borrow().is_empty(), "nothing emitted under any mode");

        // A spec that declares NO criteria has nothing to prove — unchanged.
        let bare = "bare";
        seed_full_spec(root, bare);
        std::fs::write(
            mustard_core::ClaudePaths::for_project(root)
                .unwrap()
                .for_spec(bare)
                .unwrap()
                .spec_md_path(),
            "# S\n\nNo criteria here.\n",
        )
        .unwrap();
        assert_eq!(proof_state(root_str, bare), ProofState::NotGated);
        let bare_opts = ApproveSpecOpts { spec: bare.to_string(), wave_plan: false, resume: false };
        assert_eq!(
            unmet_gate_message(bare, ClarifyState::NotGated, false, &proof_state(root_str, bare), &[]),
            None,
            "a spec with no acceptance criteria must not be refused by this gate"
        );
        // And it approves under `off`, where the two marker preconditions are muted.
        assert!(approve_at(root_str, &bare_opts, ApprovalMode::Off, "s-1", &mut record).is_ok());
    }
}
