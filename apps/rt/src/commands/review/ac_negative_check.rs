//! `mustard-rt run ac-negative-check` — the NEGATIVE TEST that proves an
//! acceptance criterion is ABLE to fail.
//!
//! # The rule, in one sentence
//!
//! **A criterion clears the proof ONLY by failing.** Its own command is run
//! against the repository AS IT IS NOW — before the work it describes exists —
//! and must come back red. Everything else is UNPROVEN: a command that already
//! exits green, one killed by its deadline, one that could not be attempted at
//! all, and one still carrying an unfilled `<…>` placeholder.
//!
//! NOTHING else in this module produces a refusal. A gate that refuses for
//! reasons the reader cannot act on teaches the caller to route around it, so
//! every unproven entry carries a short human reason naming the ONE action that
//! clears it.
//!
//! # The second half: the confirmation
//!
//! Failing before the work is only half an answer. A command that is BROKEN and
//! a command whose behaviour is merely absent are both red, so the red pass
//! alone cannot tell them apart. The CONFIRMATION pass ([`confirm`], reached by
//! `--confirm`) closes that: once the work has landed, each criterion that
//! cleared the red pass is run AGAIN and must now come back GREEN. One still
//! red there is reported UNPROVEN — it does not get to clear on its earlier
//! failure alone.
//!
//! The two passes are separate commands because they answer at opposite
//! moments. Taking the confirmation automatically would re-run every criterion
//! at PLAN time, before its work exists, where red is the CORRECT answer — the
//! approval gate would then refuse every honest spec.
//!
//! [`Confirmation`] is the second column in the record, beside [`Proof`]; the
//! two are never collapsed, for the same reason `NotAttempted` and `Green` are
//! never collapsed. Its `Inexecutable` value is the one finding no red pass can
//! produce: a command that could not be attempted at all AFTER its work landed
//! is broken whatever the work does, and that is the single state
//! [`crate::commands::spec::ac_amend`] accepts a PASSING replacement for.
//!
//! # Why a static linter cannot answer this
//!
//! Whether a command CAN fail is a fact about the repository it runs against,
//! not about how the command is spelled. `analyze_validation`'s tautology linter
//! reads command shapes and is right to stay a WARN; this command runs them.
//!
//! # NEVER TAKEN is not TAKEN AND GREEN
//!
//! [`Proof`] keeps the two apart in the record and in every message. A proof
//! that was never taken and a proof that came back green ask for OPPOSITE
//! actions — run it, versus rewrite the criterion — so collapsing them into one
//! wording is how a caller learns to read a missing artefact as a failure.
//!
//! # One parser, one executor
//!
//! The criteria are read through the SHARED `qa_run` parser
//! ([`qa_run::extract_ac_section`] + [`qa_run::parse_ac_items`]) and each
//! command runs through the SHARED `qa_run` executor ([`qa_run::execute_ac`]),
//! so the per-criterion deadline, the pipe drain and the `Expect:` regex grading
//! are the ones QA itself uses. There is no second parser and no second executor
//! to drift.
//!
//! The executor's self-invocation handling is left exactly as it is: like
//! `mustard-rt run qa-run`, this command does NOT set
//! `QaRunOptions::self_invoked`, so a criterion runs verbatim.
//!
//! # The proof ledger
//!
//! The record lands in `<spec-dir>/ac-proof.json` beside the spec markdown, with
//! entries sorted by id so the file is byte-stable, plus an `amendments` array
//! (written back untouched — the amendment operation owns it). A criterion whose
//! recorded command AND expect regex still match is NOT re-run on a later pass:
//! the ledger is precisely the reason the gate stays stable once the command
//! starts passing for the honest reason, after the work exists.
//!
//! The project root is a PARAMETER of [`check`], never re-derived from the
//! process working directory — this tool cuts a worktree per work unit, so the
//! engine runs off-root as a matter of course (the same reason
//! `analyze_validation::validate` takes its `root`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::commands::review::qa_run;
use mustard_core::io::fs;

/// The proof ledger's file name, written beside the spec markdown.
///
/// `pub(crate)` so the approval gate reads the ledger by the same name the
/// producer writes — one spelling, no drift.
pub(crate) const AC_PROOF_JSON: &str = "ac-proof.json";

/// What actually happened when a criterion's command was run.
///
/// The distinction this enum exists for: [`Proof::Green`] is a proof that WAS
/// taken and came back the wrong colour, while [`Proof::NotAttempted`] is a
/// proof that was NEVER TAKEN. They are opposite situations asking for opposite
/// actions, so they are opposite values here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Proof {
    /// TAKEN — the command ran and came back RED. This is the only outcome that
    /// proves the criterion knows how to fail.
    Red,
    /// TAKEN — the command ran and came back GREEN against the tree as it is.
    Green,
    /// TAKEN — the command ran but was killed by its deadline, so no verdict
    /// ever arrived.
    NoVerdict,
    /// NEVER TAKEN — the command was not run at all (an unfilled placeholder, or
    /// a command the executor could not attempt).
    NotAttempted,
}

/// What happened when a criterion's command was run AGAIN, after the work it
/// describes had landed — the second column of the record.
///
/// The distinction this enum exists for is the same one [`Proof`] draws, one
/// pass later: [`Confirmation::NotTaken`] is a confirmation that was NEVER
/// TAKEN, while [`Confirmation::Red`] is one that WAS taken and came back the
/// wrong colour. They ask for opposite actions — take the confirmation, versus
/// finish (or repair) the work — so they are opposite values here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Confirmation {
    /// NEVER TAKEN — the confirmation pass has not reached this criterion. The
    /// default, so every ledger written before this column existed reads as the
    /// truth about it: nobody asked.
    #[default]
    NotTaken,
    /// TAKEN — the command ran after the work landed and came back GREEN. This
    /// is the only outcome that confirms the criterion.
    Green,
    /// TAKEN — the command ran after the work landed and STILL came back red.
    /// Either the work is not there, or the command never asserted it.
    Red,
    /// TAKEN — the command was killed by its deadline, so no verdict arrived.
    NoVerdict,
    /// TAKEN — the command could not be attempted AT ALL after its work landed.
    /// Nothing the work does will change that, so this is the one state that
    /// says the criterion itself is broken rather than unmet — and the only one
    /// [`crate::commands::spec::ac_amend`] repairs with a PASSING replacement.
    Inexecutable,
}

/// Whether a criterion cleared the negative test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Verdict {
    /// The criterion cleared the pass that ran: RED before its work in the
    /// proof pass, GREEN after it in the confirmation pass.
    Proven,
    /// Anything else. The entry's `reason` names the one action that clears it.
    Unproven,
    /// The trailing criterion, exempt by position — see [`is_exempt`].
    Exempt,
}

/// One criterion's record in the proof ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AcProof {
    /// The criterion id (`AC-1`, `AC-W4-2`, …).
    pub(crate) id: String,
    /// The EXACT command string that was run (or would be run).
    pub(crate) command: String,
    /// The criterion's declared `Expect:` evidence regex, when it has one.
    #[serde(default)]
    pub(crate) expect: Option<String>,
    /// Whether the criterion cleared the proof.
    pub(crate) verdict: Verdict,
    /// The RED column: what happened when the command ran BEFORE its work
    /// existed — including NOT having run at all.
    pub(crate) proof: Proof,
    /// The CONFIRMED column: what happened when the command ran AGAIN, after
    /// its work landed. Defaults to [`Confirmation::NotTaken`], so a ledger
    /// written before this column existed says only that nobody asked.
    #[serde(default)]
    pub(crate) confirmation: Confirmation,
    /// The command's own exit code in the RED pass, when one arrived.
    #[serde(default)]
    pub(crate) exit: Option<i64>,
    /// The command's own exit code in the CONFIRMATION pass. Kept apart from
    /// [`AcProof::exit`] so confirming a criterion never overwrites the record
    /// of what it did before its work existed.
    #[serde(default)]
    pub(crate) confirmation_exit: Option<i64>,
    /// A short human reason: what happened and the one action that clears it.
    /// Absent only for a criterion that is already proven. Whichever pass spoke
    /// last owns it — a criterion has ONE next action, not one per column.
    #[serde(default)]
    pub(crate) reason: Option<String>,
    /// Bounded excerpt of the command's own output, as the executor captured it
    /// on the most recent run.
    #[serde(default)]
    pub(crate) stderr_excerpt: String,
}

impl AcProof {
    /// `true` when this record carries evidence an approval gate can act on:
    /// the criterion was shown ABLE to fail before its work (the red column),
    /// or it has since been shown to actually PASS after it (the confirmed
    /// column).
    ///
    /// The second half is not a softening — it is the only way an amendment
    /// accepted for an INEXECUTABLE predecessor can ever satisfy the gate, and
    /// that amendment's own door already refused everything else.
    ///
    /// Reading the RED column rather than [`AcProof::verdict`] is deliberate: a
    /// confirmation that came back red turns the verdict Unproven, and that is
    /// a CLOSE-time finding about the work — it must not retroactively unmake
    /// an approval the red proof legitimately earned.
    pub(crate) fn evidenced(&self) -> bool {
        self.proof == Proof::Red || self.confirmation == Confirmation::Green
    }
}

/// The on-disk proof ledger: one record per criterion plus the amendment
/// history.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AcProofLedger {
    /// The spec the criteria belong to.
    #[serde(default)]
    pub(crate) spec: String,
    /// One entry per criterion, sorted by id so the file is byte-stable.
    #[serde(default)]
    pub(crate) criteria: Vec<AcProof>,
    /// The amendment history. Read back and written out UNTOUCHED: the
    /// amendment operation owns this array, the negative test only preserves it.
    #[serde(default)]
    pub(crate) amendments: Vec<serde_json::Value>,
}

/// Which of the two passes a run takes.
///
/// A parameter rather than two engines: the spec resolution, the shared parser,
/// the ledger read/write and the report are identical, and only the per-criterion
/// question changes. Two engines is how the two halves would drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pass {
    /// Run each criterion against the tree BEFORE its work exists; red clears.
    Proof,
    /// Run each already-proven criterion AGAIN, after its work landed; green
    /// clears.
    Confirm,
}

impl Pass {
    /// The pass name published in the report, so a reader knows which question
    /// the numbers below it answer.
    fn name(self) -> &'static str {
        match self {
            Pass::Proof => "proof",
            Pass::Confirm => "confirm",
        }
    }
}

/// The JSON document printed on stdout.
#[derive(Debug, Serialize)]
pub(crate) struct NegativeCheckReport {
    /// `true` only when every non-exempt criterion cleared the pass that ran.
    pub(crate) ok: bool,
    /// Which pass ran: `proof` (red before the work) or `confirm` (green after).
    pub(crate) pass: &'static str,
    /// The spec whose criteria were tested (`None` when none resolved).
    pub(crate) spec: Option<String>,
    /// Where the ledger was written, relative to the project root and with
    /// forward slashes — a repo path, never a machine path.
    pub(crate) ledger: Option<String>,
    /// How many criteria came back red.
    pub(crate) proven: usize,
    /// How many did not — each named in `criteria` with its reason.
    pub(crate) unproven: usize,
    /// How many are exempt by position (the trailing safety criterion).
    pub(crate) exempt: usize,
    /// How many carry a GREEN confirmation — the second half of the proof.
    pub(crate) confirmed: usize,
    /// How many non-exempt criteria do NOT. In the `proof` pass this is every
    /// one of them by construction: the confirmation is not due yet.
    pub(crate) unconfirmed: usize,
    /// Every criterion's record, sorted by id.
    pub(crate) criteria: Vec<AcProof>,
    /// Why the engine could not run at all: `spec-required`, `spec-not-found`,
    /// `spec-unreadable`, `ledger-write-failed`. NOT a criterion verdict.
    pub(crate) error: Option<String>,
}

impl NegativeCheckReport {
    /// A report for a run that never got as far as testing a criterion.
    fn aborted(pass: Pass, spec: Option<String>, error: &str) -> Self {
        Self {
            ok: false,
            pass: pass.name(),
            spec,
            ledger: None,
            proven: 0,
            unproven: 0,
            exempt: 0,
            confirmed: 0,
            unconfirmed: 0,
            criteria: Vec::new(),
            error: Some(error.to_string()),
        }
    }
}

/// The reason a GREEN command is unproven — a proof that WAS taken.
const REASON_GREEN: &str = "the proof was TAKEN and the command came back green \
     against the tree as it is, so this criterion cannot tell done from \
     not-done — rewrite the command so it asserts the new behaviour";

/// The reason a timed-out command is unproven — taken, but no verdict arrived.
const REASON_NO_VERDICT: &str = "the proof was TAKEN but the command was killed by its \
     deadline, so no verdict ever arrived — narrow the command and take the proof again";

/// The reason an unattemptable command is unproven — a proof NEVER TAKEN.
const REASON_NOT_ATTEMPTED: &str = "the proof was NEVER TAKEN: the command could not be \
     attempted at all — make the command runnable, then take the proof";

/// The reason a skeleton command is unproven — a proof NEVER TAKEN.
const REASON_PLACEHOLDER: &str = "the proof was NEVER TAKEN: the command still carries an \
     unfilled `<…>` placeholder — fill it in, then take the proof";

/// Why the trailing criterion is not tested.
const REASON_EXEMPT: &str = "exempt by position: the trailing criterion is the build-green \
     safety net, green before the work by design";

/// The reason a criterion that is STILL red after its work landed is unproven.
/// It names the opposite action to [`REASON_GREEN`]: finish the work, do not
/// rewrite the command.
const REASON_STILL_RED: &str = "the confirmation was TAKEN and the command still came back red \
     AFTER its work landed, so this criterion does not clear on its earlier failure alone — \
     finish the work the criterion describes, then take the confirmation again";

/// The reason a confirmation that timed out proves nothing.
const REASON_CONFIRM_NO_VERDICT: &str = "the confirmation was TAKEN but the command was killed by \
     its deadline, so no verdict ever arrived — narrow the command and take the confirmation again";

/// The reason an INEXECUTABLE criterion is unproven, and the ONE door that
/// repairs it. A command that cannot be attempted at all after its work landed
/// is broken whatever the work does, so re-running it is not the remedy.
const REASON_INEXECUTABLE: &str = "the confirmation was TAKEN and the command could not be \
     attempted AT ALL after its work landed, so the criterion itself is inexecutable — repair it \
     through `mustard-rt run ac-amend`, which accepts a passing replacement for exactly this case";

/// The reason a criterion has no confirmation to take. Naming the missing RED
/// proof rather than the missing confirmation is deliberate: taking the
/// confirmation is not what clears it.
const REASON_NOTHING_TO_CONFIRM: &str = "there is no RED proof to confirm for the command this \
     criterion carries today — take the proof first with `mustard-rt run ac-negative-check \
     --spec <slug>`";

/// `true` when the criterion at `index` of a `total`-item list is the trailing
/// safety criterion, exempt from the negative test.
///
/// This is the positional rule the tautology linter already applies (it never
/// reports the last criterion weak, and reports nothing at all for a lone one) —
/// ONE exemption in the codebase, not two. The trailing criterion is the
/// build-green safety net: it is green before the work by design, so requiring
/// it to fail would block every spec. Pure, total.
///
/// `pub(crate)` so the amendment door
/// ([`crate::commands::spec::ac_amend`]) applies the SAME positional rule when
/// it decides whether a replacement command owes a proof at all — a second
/// spelling of "which criterion is exempt" is how the two would disagree about
/// the trailing one.
pub(crate) fn is_exempt(index: usize, total: usize) -> bool {
    total > 0 && index + 1 == total
}

/// `true` when a command is still a SKELETON — it carries an unfilled `<…>`
/// placeholder, so there is nothing to run yet. Same rule the tautology linter
/// uses to leave skeleton commands alone. Pure, total.
fn is_placeholder(command: &str) -> bool {
    command.contains('<')
}

/// Classify ONE executed criterion from the executor's status.
///
/// The whole rule lives here: `fail` — the command came back red — is the ONLY
/// status that proves anything. Pure and total, so the classification is
/// unit-testable without spawning a command.
fn classify(status: &str) -> (Verdict, Proof, Option<String>) {
    match status {
        "fail" => (Verdict::Proven, Proof::Red, None),
        "pass" => (
            Verdict::Unproven,
            Proof::Green,
            Some(REASON_GREEN.to_string()),
        ),
        "timeout" => (
            Verdict::Unproven,
            Proof::NoVerdict,
            Some(REASON_NO_VERDICT.to_string()),
        ),
        // `skip` and any future status: the criterion was never actually run.
        _ => (
            Verdict::Unproven,
            Proof::NotAttempted,
            Some(REASON_NOT_ATTEMPTED.to_string()),
        ),
    }
}

/// Classify ONE confirmation run from the executor's status.
///
/// The mirror image of [`classify`], one pass later: `pass` — the command came
/// back green once its work existed — is the ONLY status that confirms
/// anything. Pure and total, so the classification is unit-testable without
/// spawning a command.
fn classify_confirmation(status: &str) -> (Verdict, Confirmation, Option<String>) {
    match status {
        "pass" => (Verdict::Proven, Confirmation::Green, None),
        "fail" => (
            Verdict::Unproven,
            Confirmation::Red,
            Some(REASON_STILL_RED.to_string()),
        ),
        "timeout" => (
            Verdict::Unproven,
            Confirmation::NoVerdict,
            Some(REASON_CONFIRM_NO_VERDICT.to_string()),
        ),
        // `skip` and any future status: the executor could not attempt the
        // command at all, which after the work has landed means the criterion
        // is broken rather than unmet.
        _ => (
            Verdict::Unproven,
            Confirmation::Inexecutable,
            Some(REASON_INEXECUTABLE.to_string()),
        ),
    }
}

/// Locate the spec markdown for `spec`, which may be a PATH (to the markdown
/// itself or to the spec directory) or a SLUG under `.claude/spec/`.
///
/// The ledger lives beside the markdown, so the spec directory is always the
/// file's parent — one rule for all three spellings.
fn resolve_spec_file(root: &Path, spec: &str) -> Option<PathBuf> {
    let as_path = Path::new(spec);
    if as_path.is_file() {
        return Some(as_path.to_path_buf());
    }
    if as_path.is_dir() {
        return ["spec.md", "wave-plan.md"]
            .into_iter()
            .map(|name| as_path.join(name))
            .find(|p| fs::exists(p));
    }
    // A slug — resolved through the SAME locator qa-run uses, so the negative
    // test and QA can never disagree about which file a spec name names.
    qa_run::spec_file_for(root, spec)
}

/// Parse the ledger at `path`. `None` when the file is absent, unreadable or
/// unparsable — the ONE reader of `ac-proof.json` in the crate, so the producer
/// and the approval gate can never disagree about what the file says.
///
/// The two callers read the same `None` in opposite directions, deliberately:
/// [`read_ledger`] treats it as "nothing recorded yet, test the criteria again"
/// (fail-open — re-taking a proof is cheap and safe), while the approval gate
/// treats it as a refusal (fail-closed — an approval must be PROVEN, never
/// assumed). The direction is the caller's decision; the parse is not.
pub(crate) fn load_ledger(path: &Path) -> Option<AcProofLedger> {
    fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str::<AcProofLedger>(&body).ok())
}

/// Read the ledger already on disk, if any. An absent or unreadable file yields
/// an empty ledger: a proof that cannot be read is a proof that was never taken,
/// and the criteria are simply tested again.
fn read_ledger(path: &Path) -> AcProofLedger {
    load_ledger(path).unwrap_or_default()
}

/// The recorded proof for `id` whose command AND expect regex still match.
///
/// Matching on BOTH is deliberate: the expect regex is half of how the executor
/// grades a command, so a criterion whose regex changed is a different question
/// and must be asked again. The direction of the extra strictness is the safe
/// one — it can only cause a re-run, never a stale reuse.
///
/// `pub(crate)` so the approval gate looks a proof up by exactly this rule: a
/// recorded command that no longer matches the criterion's current command is
/// NO proof, which is the hand edit the gate exists to catch.
pub(crate) fn recorded_proof<'a>(
    ledger: &'a AcProofLedger,
    id: &str,
    command: &str,
    expect: Option<&str>,
) -> Option<&'a AcProof> {
    ledger.criteria.iter().find(|p| {
        p.id == id && p.command == command && p.expect.as_deref() == expect
    })
}

/// Take the proof for ONE criterion and build its ledger record.
///
/// The whole per-criterion rule, in one place: an `exempt` criterion is recorded
/// without being run; a SKELETON command is recorded as NEVER TAKEN; anything
/// else is executed through the shared `qa_run` executor and classified by
/// [`classify`].
///
/// `pub(crate)` because the amendment door
/// ([`crate::commands::spec::ac_amend`]) must ask THIS engine whether a
/// replacement command is proven, and must record the answer in the very shape
/// the ledger already carries. Re-deriving either there would let the gate that
/// produces the proof and the door that demands one drift apart — which is
/// exactly the drift this whole path exists to remove.
pub(crate) fn prove_one(
    root: &Path,
    id: &str,
    command: &str,
    expect: Option<&str>,
    exempt: bool,
) -> AcProof {
    let base = |verdict: Verdict, proof: Proof, reason: &str| AcProof {
        id: id.to_string(),
        command: command.to_string(),
        expect: expect.map(str::to_string),
        verdict,
        proof,
        confirmation: Confirmation::NotTaken,
        exit: None,
        confirmation_exit: None,
        reason: Some(reason.to_string()),
        stderr_excerpt: String::new(),
    };
    if exempt {
        return base(Verdict::Exempt, Proof::NotAttempted, REASON_EXEMPT);
    }
    if is_placeholder(command) {
        return base(Verdict::Unproven, Proof::NotAttempted, REASON_PLACEHOLDER);
    }
    let result = qa_run::execute_ac(command, expect, root);
    let (verdict, proof, reason) = classify(result.status());
    AcProof {
        id: id.to_string(),
        command: command.to_string(),
        expect: expect.map(str::to_string),
        verdict,
        proof,
        confirmation: Confirmation::NotTaken,
        exit: result.exit(),
        confirmation_exit: None,
        reason,
        stderr_excerpt: result.stderr_excerpt().to_string(),
    }
}

/// Take the CONFIRMATION for ONE criterion and build its updated record.
///
/// `previous` is the criterion's record from the ledger, already matched on id
/// AND command AND expect by [`recorded_proof`]. Only a record that cleared the
/// RED pass has anything to confirm: a criterion that never came back red is
/// returned untouched but for the reason naming what is actually missing, and
/// NOTHING is run for it — taking a confirmation is not how a missing proof is
/// obtained.
///
/// The red columns ([`AcProof::proof`], [`AcProof::exit`]) are carried over
/// verbatim. Confirming a criterion must never overwrite the record of what it
/// did before its work existed; that record is the expensive half.
pub(crate) fn confirm_one(
    root: &Path,
    id: &str,
    command: &str,
    expect: Option<&str>,
    previous: Option<&AcProof>,
) -> AcProof {
    let record = previous.cloned().unwrap_or(AcProof {
        id: id.to_string(),
        command: command.to_string(),
        expect: expect.map(str::to_string),
        verdict: Verdict::Unproven,
        proof: Proof::NotAttempted,
        confirmation: Confirmation::NotTaken,
        exit: None,
        confirmation_exit: None,
        reason: None,
        stderr_excerpt: String::new(),
    });
    if record.proof != Proof::Red {
        return AcProof {
            verdict: Verdict::Unproven,
            reason: Some(REASON_NOTHING_TO_CONFIRM.to_string()),
            ..record
        };
    }
    let result = qa_run::execute_ac(&record.command, record.expect.as_deref(), root);
    let (verdict, confirmation, reason) = classify_confirmation(result.status());
    AcProof {
        verdict,
        confirmation,
        confirmation_exit: result.exit(),
        reason,
        stderr_excerpt: result.stderr_excerpt().to_string(),
        ..record
    }
}

/// Run the negative test for `spec` against an explicit project `root`, write
/// the ledger, and return the report.
///
/// `root` is UNCONDITIONAL on purpose — an internal `current_dir()` fallback
/// would keep the hidden dependency and make the off-root defect conditional on
/// the caller remembering to pass it. Every criterion's command runs WITH `root`
/// as its working directory, which is where AC commands are written to run.
pub(crate) fn check(root: &Path, spec: &str) -> NegativeCheckReport {
    run_pass(root, spec, Pass::Proof)
}

/// Take the CONFIRMATION for `spec` against an explicit project `root`: run each
/// criterion that cleared the red pass AGAIN, now that its work has landed, and
/// require it to come back green.
///
/// A criterion still red here is reported UNPROVEN — the whole point of the
/// second half. Its earlier failure stays in the record ([`AcProof::proof`]);
/// what it no longer does is clear the criterion on its own.
pub(crate) fn confirm(root: &Path, spec: &str) -> NegativeCheckReport {
    run_pass(root, spec, Pass::Confirm)
}

/// Both passes, in one engine — see [`Pass`] for why it is a parameter.
fn run_pass(root: &Path, spec: &str, pass: Pass) -> NegativeCheckReport {
    let Some(spec_file) = resolve_spec_file(root, spec) else {
        return NegativeCheckReport::aborted(pass, Some(spec.to_string()), "spec-not-found");
    };
    let Ok(markdown) = fs::read_to_string(&spec_file) else {
        return NegativeCheckReport::aborted(pass, Some(spec.to_string()), "spec-unreadable");
    };
    let spec_dir = spec_file.parent().unwrap_or(root).to_path_buf();
    let slug = spec_dir
        .file_name()
        .map_or_else(|| spec.to_string(), |n| n.to_string_lossy().into_owned());

    let items = qa_run::extract_ac_section(&markdown)
        .map(|section| qa_run::parse_ac_items(&section))
        .unwrap_or_default();

    let ledger_path = spec_dir.join(AC_PROOF_JSON);
    let previous = read_ledger(&ledger_path);

    let total = items.len();
    let mut criteria: Vec<AcProof> = Vec::with_capacity(total);
    for (index, item) in items.iter().enumerate() {
        let expect = item.expect.as_deref();
        // Exempt in BOTH passes, for the one reason: the trailing criterion is
        // the build-green safety net, so neither colour tells anyone anything.
        if is_exempt(index, total) {
            criteria.push(prove_one(root, &item.id, &item.command, expect, true));
            continue;
        }
        let recorded = recorded_proof(&previous, &item.id, &item.command, expect);
        if pass == Pass::Confirm {
            // The confirmation only ever speaks about a criterion the ledger
            // already carries. One it does not is missing its RED proof, and a
            // green run here would answer a question nobody asked.
            criteria.push(confirm_one(root, &item.id, &item.command, expect, recorded));
            continue;
        }
        // A proof already recorded for this exact command is kept as it is: the
        // command WILL start passing once the work exists, and re-running it
        // then would turn every recorded red into a green nobody can act on.
        if let Some(kept) = recorded {
            criteria.push(kept.clone());
            continue;
        }
        criteria.push(prove_one(root, &item.id, &item.command, expect, false));
    }
    // Byte-stability: one deterministic order for the file AND the report.
    criteria.sort_by(|a, b| a.id.cmp(&b.id));

    let proven = criteria.iter().filter(|c| c.verdict == Verdict::Proven).count();
    let unproven = criteria.iter().filter(|c| c.verdict == Verdict::Unproven).count();
    let exempt = criteria.iter().filter(|c| c.verdict == Verdict::Exempt).count();
    let confirmed = criteria
        .iter()
        .filter(|c| c.confirmation == Confirmation::Green)
        .count();
    let unconfirmed = criteria
        .iter()
        .filter(|c| c.verdict != Verdict::Exempt && c.confirmation != Confirmation::Green)
        .count();

    // The ledger is written whichever way the verdicts fell: the proofs already
    // obtained are the expensive part of this run and must not be lost because a
    // sibling criterion is still unproven.
    let ledger = AcProofLedger {
        spec: slug.clone(),
        criteria: criteria.clone(),
        // Preserved verbatim — the amendment operation is this array's author.
        amendments: previous.amendments,
    };
    let written = write_ledger(&ledger_path, &ledger);

    // Each pass is judged by its OWN question: red before the work, green
    // after. `unproven` already carries both — the confirm pass writes an
    // Unproven verdict for every criterion it did not confirm.
    NegativeCheckReport {
        ok: written && unproven == 0,
        pass: pass.name(),
        spec: Some(slug),
        ledger: written.then(|| repo_relative(root, &ledger_path)),
        proven,
        unproven,
        exempt,
        confirmed,
        unconfirmed,
        criteria,
        error: (!written).then(|| "ledger-write-failed".to_string()),
    }
}

/// Serialize and write the ledger. `false` when nothing landed on disk — a lost
/// record is reported, never assumed away.
fn write_ledger(path: &Path, ledger: &AcProofLedger) -> bool {
    let Ok(mut body) = serde_json::to_string_pretty(ledger) else {
        return false;
    };
    body.push('\n');
    fs::write_atomic(path, body.as_bytes()).is_ok()
}

/// `path` relative to the project root, with forward slashes — a repo path the
/// output can carry without leaking the machine it ran on (the `run` surface is
/// snapshot-compared). Falls back to the file name when it is outside the root.
///
/// `pub(crate)` so the amendment door reports the artefacts it rewrote in the
/// same repo-path spelling the ledger path already uses.
pub(crate) fn repo_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .or_else(|| path.file_name().map(Path::new))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The process exit code for a finished report.
///
/// `0` — every non-exempt criterion is proven. `2` — at least one is unproven
/// (the blocking verdict). `1` — the engine could not run at all, which is an
/// input error and not a verdict about any criterion.
fn exit_code(report: &NegativeCheckReport) -> i32 {
    if report.error.is_some() {
        1
    } else if report.unproven > 0 {
        2
    } else {
        0
    }
}

/// Dispatch `mustard-rt run ac-negative-check`.
///
/// `confirm` selects the SECOND pass (`--confirm`): the criteria that cleared
/// the red proof are run again, after their work has landed, and must now come
/// back green. Two invocations rather than one automatic pair, because red is
/// the correct answer at PLAN time and the wrong one after EXECUTE.
pub fn run(spec: Option<&str>, confirm: bool) {
    let root = PathBuf::from(crate::shared::context::project_dir());
    let take = if confirm { self::confirm } else { check };
    let report = match spec.map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => take(&root, spec),
        None => NegativeCheckReport::aborted(
            if confirm { Pass::Confirm } else { Pass::Proof },
            None,
            "spec-required",
        ),
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    std::process::exit(exit_code(&report));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A command that comes back RED on both shells (`cmd.exe` and `sh`): the
    /// directory does not exist, so `cd` exits non-zero.
    const RED_COMMAND: &str = "cd no-such-directory-abc";
    /// A command that comes back GREEN on both shells — `cd .` is a builtin
    /// everywhere and always succeeds.
    const GREEN_COMMAND: &str = "cd .";

    /// Seed `<root>/.claude/spec/<spec>/spec.md` with `body`; returns the dir.
    fn seed(root: &Path, spec: &str, body: &str) -> PathBuf {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), body).unwrap();
        dir
    }

    /// One criterion of the report, by id.
    fn entry<'a>(report: &'a NegativeCheckReport, id: &str) -> &'a AcProof {
        report
            .criteria
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("{id} missing from {:?}", report.criteria))
    }

    /// A spec whose second criterion already exits green BEFORE its work exists.
    /// AC-1 fails now (proven), AC-2 passes now (vacuous), AC-3 is the trailing
    /// safety criterion.
    fn vacuous_spec_body() -> String {
        format!(
            "# S\n\n## Acceptance Criteria\n\
             - **AC-1** — when the work lands, then the new behaviour holds.\n  Command: `{RED_COMMAND}`\n\
             - **AC-2** — when the work lands, then the other thing holds.\n  Command: `{GREEN_COMMAND}`\n\
             - **AC-3** — build green.\n  Command: `{GREEN_COMMAND}`\n"
        )
    }

    /// AC-2 — a criterion whose command already exits green against the tree as
    /// it is is VACUOUS, not proven: it cannot tell done from not-done. The
    /// wording says the proof was TAKEN and came back green — never the
    /// never-taken wording, which asks for the opposite action.
    #[test]
    fn a_command_that_passes_now_is_vacuous() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "vacuous", &vacuous_spec_body());
        let report = check(dir.path(), "vacuous");

        let vacuous = entry(&report, "AC-2");
        assert_eq!(vacuous.verdict, Verdict::Unproven, "a green command proves nothing");
        assert_eq!(vacuous.proof, Proof::Green);
        assert_eq!(vacuous.exit, Some(0), "the command genuinely exited 0");
        let reason = vacuous.reason.clone().unwrap_or_default();
        assert!(reason.contains("TAKEN"), "the proof WAS taken: {reason}");
        assert!(reason.contains("green"), "and it came back green: {reason}");
        assert!(
            !reason.contains("NEVER TAKEN"),
            "a green proof is the opposite of a missing one: {reason}"
        );

        // Two-sided: the criterion that DOES fail now is proven, so the
        // assertion above cannot pass by the engine calling everything vacuous.
        let proven = entry(&report, "AC-1");
        assert_eq!(proven.verdict, Verdict::Proven, "a red command clears the proof");
        assert_eq!(proven.proof, Proof::Red);
        assert!(proven.reason.is_none(), "a proven criterion needs no remedy");
        // And the trailing safety criterion is exempt, never tested.
        assert_eq!(entry(&report, "AC-3").verdict, Verdict::Exempt);
    }

    /// AC-3 — one unproven criterion blocks (exit 2) and the ledger STILL
    /// records the proofs the run did obtain: those reds are the expensive part
    /// of the run and must survive a sibling's failure.
    #[test]
    fn unproven_criterion_blocks_and_records_the_proofs() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "blocked", &vacuous_spec_body());
        let report = check(dir.path(), "blocked");

        assert!(!report.ok, "an unproven criterion must not report ok");
        assert_eq!(exit_code(&report), 2, "the blocking exit code");
        assert_eq!((report.proven, report.unproven, report.exempt), (1, 1, 1));

        // The ledger landed, and it carries the proof that WAS obtained.
        let ledger_path = spec_dir.join(AC_PROOF_JSON);
        let body = std::fs::read_to_string(&ledger_path).unwrap();
        let ledger: AcProofLedger = serde_json::from_str(&body).unwrap();
        assert_eq!(ledger.spec, "blocked");
        assert!(ledger.amendments.is_empty(), "the amendment history starts empty");
        let ids: Vec<&str> = ledger.criteria.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["AC-1", "AC-2", "AC-3"], "entries sorted by id");
        let recorded = ledger
            .criteria
            .iter()
            .find(|c| c.id == "AC-1")
            .unwrap();
        assert_eq!(recorded.verdict, Verdict::Proven);
        assert_eq!(recorded.command, RED_COMMAND, "the EXACT command that ran");
        // Byte-stable: the same inputs produce the same file, byte for byte.
        let again = check(dir.path(), "blocked");
        assert_eq!(std::fs::read_to_string(&ledger_path).unwrap(), body);
        assert_eq!(exit_code(&again), 2);
    }

    /// A recorded proof is NOT taken again while its command and expect regex
    /// still match — that is what keeps the gate stable once the work exists and
    /// the command starts passing for the honest reason.
    #[test]
    fn a_recorded_proof_is_not_taken_again() {
        let dir = tempdir().unwrap();
        // The spec's first criterion would come back GREEN if it were run now.
        let body = format!(
            "# S\n\n## Acceptance Criteria\n\
             - **AC-1** — the behaviour holds.\n  Command: `{GREEN_COMMAND}`\n\
             - **AC-2** — build green.\n  Command: `{GREEN_COMMAND}`\n"
        );
        let spec_dir = seed(dir.path(), "kept", &body);
        // A ledger already recording AC-1 as proven, for that exact command.
        let seeded = AcProofLedger {
            spec: "kept".to_string(),
            criteria: vec![AcProof {
                id: "AC-1".to_string(),
                command: GREEN_COMMAND.to_string(),
                expect: None,
                verdict: Verdict::Proven,
                proof: Proof::Red,
                confirmation: Confirmation::NotTaken,
                exit: Some(1),
                confirmation_exit: None,
                reason: None,
                stderr_excerpt: String::new(),
            }],
            amendments: vec![serde_json::json!({ "id": "AC-1", "reason": "from wave 3" })],
        };
        std::fs::write(
            spec_dir.join(AC_PROOF_JSON),
            serde_json::to_string_pretty(&seeded).unwrap(),
        )
        .unwrap();

        let report = check(dir.path(), "kept");
        assert_eq!(
            entry(&report, "AC-1").verdict,
            Verdict::Proven,
            "the recorded proof stands; re-running would have read green"
        );
        assert!(report.ok);
        // The amendment history the ledger already carried is preserved.
        let ledger: AcProofLedger =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join(AC_PROOF_JSON)).unwrap())
                .unwrap();
        assert_eq!(ledger.amendments.len(), 1, "amendments are never dropped");

        // Two-sided: change the command and the proof no longer applies — the
        // criterion is asked again, and now reads green.
        let changed = format!(
            "# S\n\n## Acceptance Criteria\n\
             - **AC-1** — the behaviour holds.\n  Command: `{GREEN_COMMAND} .`\n\
             - **AC-2** — build green.\n  Command: `{GREEN_COMMAND}`\n"
        );
        std::fs::write(spec_dir.join("spec.md"), changed).unwrap();
        let rechecked = check(dir.path(), "kept");
        assert_eq!(entry(&rechecked, "AC-1").verdict, Verdict::Unproven);
        assert_eq!(entry(&rechecked, "AC-1").proof, Proof::Green);
    }

    /// A criterion still carrying an unfilled `<…>` placeholder was NEVER
    /// TAKEN — the command was not run at all. Its wording must not read like a
    /// proof that came back green, because the action it asks for is the
    /// opposite one.
    #[test]
    fn an_unfilled_placeholder_is_never_taken_not_a_green_proof() {
        let dir = tempdir().unwrap();
        let body = format!(
            "# S\n\n## Acceptance Criteria\n\
             - **AC-1** — the new unit passes.\n  Command: `cargo test -p mustard-rt <test-name>`\n\
             - **AC-2** — build green.\n  Command: `{GREEN_COMMAND}`\n"
        );
        seed(dir.path(), "skeleton", &body);
        let report = check(dir.path(), "skeleton");

        let skeleton = entry(&report, "AC-1");
        assert_eq!(skeleton.verdict, Verdict::Unproven);
        assert_eq!(skeleton.proof, Proof::NotAttempted, "nothing was ever run");
        assert_eq!(skeleton.exit, None, "a command never run has no exit code");
        let reason = skeleton.reason.clone().unwrap_or_default();
        assert!(reason.contains("NEVER TAKEN"), "absence names itself: {reason}");
        assert!(reason.contains("placeholder"), "and names the remedy: {reason}");
        assert!(
            !reason.contains("came back green"),
            "a proof never taken is not a proof that came back green: {reason}"
        );
    }

    /// AC-1 — the second half of the proof. A criterion that cleared the red
    /// pass must come back GREEN once its work has landed; one that is STILL
    /// red there is reported unproven instead of clearing on its earlier
    /// failure alone.
    ///
    /// The "work" is materialised literally: [`RED_COMMAND`] is `cd` into a
    /// directory that does not exist, so CREATING that directory is exactly the
    /// event the criterion asserts. The same command, the same ledger — only
    /// the tree changes, which is the whole claim under test.
    ///
    /// Two-sided by construction: the first confirmation refuses, the second
    /// (after the "work") clears, so the assertion cannot pass by the pass
    /// being permanently red or permanently green.
    #[test]
    fn ac_proof_requires_green_after() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "second-half", &vacuous_spec_body());

        // The RED pass, unchanged: AC-1 fails now, so it is proven able to fail.
        // (AC-2 is vacuous and AC-3 exempt — neither is what this test judges.)
        let proof = check(dir.path(), "second-half");
        assert_eq!(entry(&proof, "AC-1").verdict, Verdict::Proven);
        assert_eq!(entry(&proof, "AC-1").proof, Proof::Red);
        assert_eq!(
            entry(&proof, "AC-1").confirmation,
            Confirmation::NotTaken,
            "the red pass never claims a confirmation it did not take"
        );
        assert_eq!(proof.pass, "proof");
        assert_eq!(proof.confirmed, 0, "nothing is confirmed before the work");

        // The CONFIRMATION pass, with the work still absent: STILL red.
        let still_red = confirm(dir.path(), "second-half");
        let ac1 = entry(&still_red, "AC-1");
        assert_eq!(
            ac1.verdict,
            Verdict::Unproven,
            "a criterion still red after its work must not clear on its earlier failure"
        );
        assert_eq!(ac1.confirmation, Confirmation::Red);
        assert_eq!(ac1.proof, Proof::Red, "the earlier failure stays in the record");
        assert!(ac1.exit.is_some(), "and so does the exit code it produced then");
        let reason = ac1.reason.clone().unwrap_or_default();
        assert!(reason.contains("still came back red"), "{reason}");
        assert!(
            reason.contains("finish the work"),
            "the remedy is the OPPOSITE of the green one — finish, not rewrite: {reason}"
        );
        assert_eq!(still_red.pass, "confirm");
        assert!(!still_red.ok, "an unconfirmed criterion must not report ok");
        assert_eq!(exit_code(&still_red), 2, "the blocking exit code");

        // THE WORK LANDS: the directory the criterion asserts now exists, so the
        // very same command comes back green.
        std::fs::create_dir(dir.path().join("no-such-directory-abc")).unwrap();
        let green = confirm(dir.path(), "second-half");
        let ac1 = entry(&green, "AC-1");
        assert_eq!(ac1.verdict, Verdict::Proven, "green after the work clears it");
        assert_eq!(ac1.confirmation, Confirmation::Green);
        assert_eq!(ac1.confirmation_exit, Some(0));
        assert!(ac1.reason.is_none(), "a confirmed criterion needs no remedy");
        assert_eq!(green.confirmed, 1, "AC-1 alone: AC-2 never cleared the red pass");
        assert!(ac1.evidenced(), "a green confirmation is evidence a gate can act on");

        // The ledger carries BOTH columns, so a later reader never has to re-run
        // anything to know what happened on each side of the work.
        let ledger: AcProofLedger = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join(AC_PROOF_JSON)).unwrap(),
        )
        .unwrap();
        let recorded = ledger.criteria.iter().find(|c| c.id == "AC-1").unwrap();
        assert_eq!((recorded.proof, recorded.confirmation), (Proof::Red, Confirmation::Green));

        // A criterion that never cleared the RED pass has nothing to confirm,
        // and the confirmation does not become a back door to clearing it.
        let ac2 = entry(&green, "AC-2");
        assert_eq!(ac2.verdict, Verdict::Unproven);
        assert_eq!(ac2.confirmation, Confirmation::NotTaken);
        assert!(
            ac2.reason.clone().unwrap_or_default().contains("no RED proof to confirm"),
            "{:?}",
            ac2.reason
        );
    }

    /// The confirmation classification, as a table — the mirror of
    /// [`only_a_failing_command_clears_the_proof`]. `pass` is the ONLY status
    /// that confirms anything, and `skip` earns its own INEXECUTABLE value:
    /// a command that cannot be attempted after its work landed is broken
    /// whatever the work does, so its remedy is the amendment door, not a
    /// re-run.
    #[test]
    fn only_a_passing_command_clears_the_confirmation() {
        assert_eq!(classify_confirmation("pass").0, Verdict::Proven);
        assert_eq!(classify_confirmation("pass").1, Confirmation::Green);
        assert!(classify_confirmation("pass").2.is_none());
        assert_eq!(classify_confirmation("fail").1, Confirmation::Red);
        assert_eq!(classify_confirmation("timeout").1, Confirmation::NoVerdict);
        assert_eq!(classify_confirmation("skip").1, Confirmation::Inexecutable);
        for status in ["fail", "timeout", "skip"] {
            let (verdict, confirmation, reason) = classify_confirmation(status);
            assert_eq!(verdict, Verdict::Unproven, "{status}");
            assert_ne!(confirmation, Confirmation::NotTaken, "{status} WAS taken");
            assert!(reason.is_some(), "{status} must name its remedy");
        }
        // NEVER TAKEN is the default, and no executed status can produce it —
        // the same separation `Proof` draws between absence and a wrong colour.
        assert_eq!(Confirmation::default(), Confirmation::NotTaken);
        assert!(
            classify_confirmation("skip")
                .2
                .unwrap_or_default()
                .contains("ac-amend"),
            "the inexecutable remedy names the one door that repairs it"
        );
    }

    /// The classification rule, stated as a table: `fail` is the ONLY status
    /// that proves anything, and each of the other three keeps its own [`Proof`]
    /// so the report never collapses a missing proof into a green one.
    #[test]
    fn only_a_failing_command_clears_the_proof() {
        assert_eq!(classify("fail").0, Verdict::Proven);
        assert_eq!(classify("fail").1, Proof::Red);
        assert_eq!(classify("pass"), (Verdict::Unproven, Proof::Green, Some(REASON_GREEN.to_string())));
        assert_eq!(classify("timeout").1, Proof::NoVerdict);
        assert_eq!(classify("skip").1, Proof::NotAttempted);
        // Every unproven classification carries a reason the caller can act on.
        for status in ["pass", "timeout", "skip"] {
            let (verdict, _, reason) = classify(status);
            assert_eq!(verdict, Verdict::Unproven, "{status}");
            assert!(reason.is_some(), "{status} must name its remedy");
        }
    }

    /// The positional exemption: the trailing criterion, and only it.
    #[test]
    fn only_the_trailing_criterion_is_exempt() {
        assert!(!is_exempt(0, 3));
        assert!(!is_exempt(1, 3));
        assert!(is_exempt(2, 3));
        assert!(is_exempt(0, 1), "a lone criterion is the trailing one");
        assert!(!is_exempt(0, 0));
    }

    /// A spec that cannot be located is an INPUT error (exit 1), never a verdict
    /// about a criterion — the two must not be read as the same refusal.
    #[test]
    fn an_unresolvable_spec_is_an_input_error_not_a_verdict() {
        let dir = tempdir().unwrap();
        let report = check(dir.path(), "no-such-spec");
        assert_eq!(report.error.as_deref(), Some("spec-not-found"));
        assert_eq!(report.unproven, 0, "no criterion was judged");
        assert_eq!(exit_code(&report), 1, "an input error is not the blocking code");
    }
}
