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
//! What is NOT left to memory is WHO takes the confirmation once the work has
//! landed: [`crate::commands::pipeline::close_pipeline`] takes it itself, on
//! every close, through [`confirm_in_process`]. A flag nobody is told about is
//! a mechanism that ships inert — the close is the moment the second half is
//! due, so the close is what asks for it.
//!
//! [`Confirmation`] is the second column in the record, beside [`Proof`]; the
//! two are never collapsed, for the same reason `NotAttempted` and `Green` are
//! never collapsed. Its `Inexecutable` value is the one finding no red pass can
//! produce: a command that could not be attempted at all AFTER its work landed
//! is broken whatever the work does, and that is the single state
//! [`crate::commands::spec::ac_amend`] accepts a PASSING replacement for.
//!
//! # The third transition: the removal
//!
//! Red before and green after still leave a gap. A criterion can be red before
//! its work (the thing it names did not exist yet) and green after it (it does
//! now) while asserting nothing the work actually DOES — the classic shape is a
//! command pointing at a subsystem the waves never touched, which both earlier
//! passes wave through.
//!
//! The REMOVAL pass ([`removal`], reached by `--removal`) runs each CONFIRMED
//! criterion against a checkout with the work taken away again. One that is
//! STILL GREEN there SURVIVED the removal and is reported as verifying nothing
//! — [`Removal::Survived`], the third column, and the finding this pass exists
//! to produce.
//!
//! What "the work" is comes off the record, not a guess: each wave already
//! caches the digest of the files it changed, and [`super::work_removed`] reads
//! that set and strips it in a scratch worktree. The live checkout is never
//! written to.
//!
//! ## What a RED here does and does not mean — the limit, stated
//!
//! This pass is a FALSIFIER, not a certificate, and saying otherwise would be
//! the habit the rest of this module exists to remove. The strip is
//! file-grained, because file paths are all the cached digest carries, so it
//! takes away the criterion's own evidence whenever that evidence shares a file
//! with the behaviour — for a project whose tests live beside the code they
//! test, that is every test criterion there is. A red from such a run was
//! guaranteed before the command was even spawned.
//!
//! So the pass does NOT report those as proven. [`super::work_removed`]
//! publishes the words the strip took out of the tree, and a criterion whose
//! OWN EVIDENCE names one of them is recorded as [`Removal::EvidenceRemoved`]:
//! the removal was NOT TAKEN for it, because taking it could only have produced
//! an answer about the strip. Its remedy is the honest one — assert the
//! behaviour through evidence the work does not carry away with it, or accept
//! that this transition has nothing to say about this criterion.
//!
//! "Own evidence" is BOTH halves the executor grades with — the command AND the
//! `Expect:` regex. A criterion reading a file the work writes into
//! (``Command: `type lib.txt`  Expect: `beta_marker` ``) carries its marker
//! only in the expectation, and consulting the command alone would hand that
//! whole class a strip-manufactured red booked as proof.
//!
//! What survives, then, is exactly one sound reading each way: GREEN with the
//! work gone is a finding about the criterion; RED with the criterion's own
//! evidence — command and expectation alike — still intact is a red the
//! behaviour earned.
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
use std::collections::BTreeSet;
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

/// What happened when a criterion's command was run against a tree with the
/// work it describes TAKEN AWAY — the third column of the record.
///
/// The transition nobody was testing. The red proof says the criterion failed
/// BEFORE its work existed and the confirmation says it passed AFTER, and a
/// criterion pointing at something the work never did clears both. Removing the
/// work again is what catches it — see the module's own statement of what this
/// column can and cannot say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Removal {
    /// NEVER TAKEN — nobody removed the work and asked. The default, so every
    /// ledger written before this column existed reads as the truth about it.
    #[default]
    NotTaken,
    /// TAKEN — the command came back RED with the work removed, and the strip
    /// left the criterion's own evidence in place, so the red is one the
    /// missing behaviour earned rather than one the strip manufactured.
    Red,
    /// TAKEN — the command came back GREEN with the work removed, so it
    /// SURVIVED the removal: it verifies something the work did not do.
    Survived,
    /// TAKEN — the command was killed by its deadline, so no verdict arrived.
    NoVerdict,
    /// TAKEN — the command could not be attempted against the stripped tree.
    NotAttempted,
    /// NOT TAKEN — the strip took away the criterion's OWN evidence along with
    /// the behaviour (its command names a word the removal deleted from the
    /// tree), so the command was guaranteed to come back red before it was even
    /// spawned. Running it would have produced a fact about the strip dressed
    /// as a fact about the criterion, which is the one answer this whole module
    /// refuses — so the command is not run, and this says so.
    EvidenceRemoved,
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
    /// The REMOVED column: what happened when the command ran against a tree
    /// with the work taken away again. Defaults to [`Removal::NotTaken`].
    #[serde(default)]
    pub(crate) removal: Removal,
    /// The command's own exit code in the REMOVAL pass. Kept apart from the
    /// other two for the same reason they are apart from each other: a later
    /// pass must never overwrite the record of an earlier one.
    #[serde(default)]
    pub(crate) removal_exit: Option<i64>,
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
    /// The ADDITION history — criteria introduced after the artefacts were
    /// frozen ([`crate::commands::spec::ac_add`]). Kept apart from
    /// [`AcProofLedger::amendments`] for the reason the two doors are apart: an
    /// amendment SUPERSEDES a predecessor and an addition has none, so folding
    /// them would make the audit trail unable to say which happened. Read back
    /// and written out untouched by every other pass.
    #[serde(default)]
    pub(crate) additions: Vec<serde_json::Value>,
}

/// Which of the three passes a run takes.
///
/// A parameter rather than three engines: the spec resolution, the shared
/// parser, the ledger read/write and the report are identical, and only the
/// per-criterion question changes. Three engines is how the halves would drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pass {
    /// Run each criterion against the tree BEFORE its work exists; red clears.
    Proof,
    /// Run each already-proven criterion AGAIN, after its work landed; green
    /// clears.
    Confirm,
    /// Run each already-confirmed criterion against a tree with the work TAKEN
    /// AWAY; red clears. The third transition.
    Removal,
}

impl Pass {
    /// The pass name published in the report, so a reader knows which question
    /// the numbers below it answer.
    fn name(self) -> &'static str {
        match self {
            Pass::Proof => "proof",
            Pass::Confirm => "confirm",
            Pass::Removal => "removal",
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
    /// How many came back RED with their work taken away AND with their own
    /// evidence intact — the reds the missing behaviour earned.
    pub(crate) removed_red: usize,
    /// How many SURVIVED the removal: still green with the work gone, so they
    /// verify nothing. Each is named in `criteria` with its reason.
    pub(crate) survived: usize,
    /// How many the removal declined to judge because the strip took away the
    /// criterion's own evidence. Counted APART from `removed_red` on purpose:
    /// folding a guaranteed red into the proven column is precisely how this
    /// pass would report a coverage it never had.
    pub(crate) evidence_removed: usize,
    /// The files whose work the removal pass actually took away, as repo paths.
    /// Reported rather than assumed: a removal is only as good as the strip it
    /// ran against, and an operator reading `red` owes a look at WHAT was
    /// removed. Empty for the other two passes, which remove nothing.
    pub(crate) taken_away: Vec<String>,
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
            removed_red: 0,
            survived: 0,
            evidence_removed: 0,
            taken_away: Vec::new(),
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

/// The reason a confirmation was NOT taken by a pass whose own executable is
/// the file the criterion's command would overwrite. It is deliberately NOT
/// [`REASON_INEXECUTABLE`]: the command was never attempted here, so calling it
/// broken would order the reader to rewrite a criterion nothing is wrong with —
/// the exact "answer that reads like the fact" this whole path exists to refuse.
///
/// It names the FILE, relative to `root`, rather than a crate: the crate name
/// was never the fact — two files with the same crate behind them are not in
/// conflict, and the ledger this reason lands in is committed.
fn reason_confirm_not_here(root: &Path) -> String {
    let running = qa_run::running_binary_label(root);
    format!(
        "the confirmation was NOT TAKEN here: this command overwrites `{running}`, the file this \
         process is executing from, so the close could not attempt it — take it from a shell with \
         `mustard-rt run ac-negative-check --confirm --spec <slug>`"
    )
}

/// The reason a criterion SURVIVED the removal of its own work — the finding
/// the third transition exists to produce. It names the shape the field keeps
/// producing: the command is satisfied by something the waves never touched, so
/// the work could not have been what made it go green.
const REASON_SURVIVED: &str = "the REMOVAL was TAKEN and the command still came back green with \
     the work taken away, so this criterion is satisfied by something the work did not do — a \
     path, a name or a subsystem that outlives the removal entirely. Rewrite the command so it \
     asserts the BEHAVIOUR, then take the proof again";

/// The reason a removal that timed out proves nothing.
const REASON_REMOVAL_NO_VERDICT: &str = "the REMOVAL was TAKEN but the command was killed by its \
     deadline, so no verdict ever arrived — narrow the command and take the removal again";

/// The reason a removal that could not be attempted proves nothing. Unlike the
/// confirmation's INEXECUTABLE, this is NOT a finding about the criterion: the
/// tree it ran against is a scratch checkout, so a failure to attempt is far
/// more likely to be about that tree than about the command.
const REASON_REMOVAL_NOT_ATTEMPTED: &str = "the REMOVAL was NOT TAKEN: the command could not be \
     attempted against the tree with the work removed — check the scratch checkout is complete, \
     then take the removal again";

/// The reason the removal declined to judge a criterion whose own evidence the
/// strip took away, naming the WORD that gave it away.
///
/// It is deliberately not a finding against the criterion: nothing about the
/// command is wrong, and ordering a rewrite here would be the fourth answer
/// that reads like the fact. It states the limit and the only two things that
/// change it.
///
/// It says "own evidence", not "command", because the word can come from either
/// half the executor grades with — a criterion reading a file the work writes
/// into carries its marker in the `Expect:` regex alone, and a reason that
/// named the command would be pointing the reader at the wrong line.
fn reason_evidence_removed(word: &str) -> String {
    format!(
        "the REMOVAL was NOT TAKEN: this criterion's own evidence (its command or its `Expect:` \
         regex) names `{word}`, which the strip itself deleted from the tree, so the command was \
         going to come back red whatever the behaviour does — that red would be a fact about the \
         strip, not about the criterion. This transition has nothing to say here; it speaks only \
         about a criterion whose evidence outlives the work it checks"
    )
}

/// The reason a criterion has nothing to remove. Naming the missing GREEN
/// confirmation is deliberate: removing the work answers nothing about a
/// criterion nobody has seen pass WITH the work.
const REASON_NOTHING_TO_REMOVE: &str = "there is no GREEN confirmation to test against for the \
     command this criterion carries today — take the confirmation first with `mustard-rt run \
     ac-negative-check --confirm --spec <slug>`";

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

/// Classify ONE removal run from the executor's status.
///
/// The rule is [`classify`]'s again, one transition later and for the opposite
/// tree: with the work taken away, `fail` — red — is the only status the
/// criterion clears on. `pass` is the finding: the command SURVIVED the
/// removal, so it verifies something the work did not do. Pure and total.
///
/// This function is only ever reached for a criterion the strip left able to
/// answer — [`remove_one`] decides that BEFORE spawning anything, so no
/// guaranteed red ever arrives here to be read as a proof.
fn classify_removal(status: &str) -> (Verdict, Removal, Option<String>) {
    match status {
        "fail" => (Verdict::Proven, Removal::Red, None),
        "pass" => (
            Verdict::Unproven,
            Removal::Survived,
            Some(REASON_SURVIVED.to_string()),
        ),
        "timeout" => (
            Verdict::Unproven,
            Removal::NoVerdict,
            Some(REASON_REMOVAL_NO_VERDICT.to_string()),
        ),
        // `skip` and any future status: nothing was actually run.
        _ => (
            Verdict::Unproven,
            Removal::NotAttempted,
            Some(REASON_REMOVAL_NOT_ATTEMPTED.to_string()),
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
        removal: Removal::NotTaken,
        removal_exit: None,
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
        removal: Removal::NotTaken,
        removal_exit: None,
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
///
/// `in_process` says this pass is running INSIDE `mustard-rt` itself (the
/// `close-pipeline` composite). A criterion whose command rebuilds that very
/// binary cannot be attempted from here, and is recorded as a confirmation NOT
/// TAKEN — never as [`Confirmation::Inexecutable`], which is an order to
/// rewrite the criterion.
pub(crate) fn confirm_one(
    root: &Path,
    id: &str,
    command: &str,
    expect: Option<&str>,
    previous: Option<&AcProof>,
    in_process: bool,
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
        removal: Removal::NotTaken,
        removal_exit: None,
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
    // Asked BEFORE anything is spawned, and asked of the PATHS: only when the
    // command would overwrite this very executable does attempting it buy a
    // doomed compile and — worse — a `skip` the classifier would read as
    // INEXECUTABLE. The honest answer there is that nobody looked, and the
    // reason names the shell that can. When the two files differ, the
    // criterion is confirmed like any other.
    if in_process && qa_run::targets_running_binary(&record.command, root) {
        return AcProof {
            verdict: Verdict::Unproven,
            confirmation: Confirmation::NotTaken,
            reason: Some(reason_confirm_not_here(root)),
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

/// Take the REMOVAL for ONE criterion and build its updated record.
///
/// `tree` is a checkout of the project with the work the criterion describes
/// TAKEN AWAY — never the live root, which is why it is a parameter rather than
/// something derived here. The command runs there and must come back RED.
///
/// Only a criterion carrying a GREEN confirmation has anything to remove: one
/// nobody has seen pass WITH the work says nothing about a tree without it, so
/// it is returned untouched but for the reason naming what is actually missing,
/// and NOTHING is run for it.
///
/// `removed_text` is the set of words [`super::work_removed`] says the strip
/// took OUT of the tree. A criterion whose command OR `Expect:` regex names one
/// of them had its own evidence stripped along with the behaviour, so its red
/// was decided before the command ran — it is recorded as
/// [`Removal::EvidenceRemoved`] and, like the unconfirmed case, NOTHING is run
/// for it. Both halves are consulted because the executor grades with both: a
/// criterion whose marker lives only in its expectation is exactly as
/// pre-decided as one that names the marker in its command. Deciding this
/// before spawning is what keeps a guaranteed red out of the proven column
/// instead of trying to read one back out of it afterwards.
///
/// A criterion declined this way keeps the verdict its earlier passes earned:
/// the removal did not speak about it, and turning silence into a failure is the
/// same error as turning it into a proof.
///
/// The earlier columns are carried over verbatim, for the reason
/// [`confirm_one`] carries its own: a later pass must never overwrite the record
/// of an earlier one.
pub(crate) fn remove_one(
    tree: &Path,
    id: &str,
    command: &str,
    expect: Option<&str>,
    previous: Option<&AcProof>,
    removed_text: &BTreeSet<String>,
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
        removal: Removal::NotTaken,
        removal_exit: None,
        reason: None,
        stderr_excerpt: String::new(),
    });
    if record.confirmation != Confirmation::Green {
        return AcProof {
            verdict: Verdict::Unproven,
            reason: Some(REASON_NOTHING_TO_REMOVE.to_string()),
            ..record
        };
    }
    if let Some(word) =
        super::work_removed::taken_away_word(removed_text, &record.command, record.expect.as_deref())
    {
        return AcProof {
            removal: Removal::EvidenceRemoved,
            reason: Some(reason_evidence_removed(&word)),
            ..record
        };
    }
    let result = qa_run::execute_ac(&record.command, record.expect.as_deref(), tree);
    let (verdict, removal, reason) = classify_removal(result.status());
    AcProof {
        verdict,
        removal,
        removal_exit: result.exit(),
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
    run_pass(root, spec, root, Pass::Proof, false, &BTreeSet::new())
}

/// Take the CONFIRMATION for `spec` against an explicit project `root`: run each
/// criterion that cleared the red pass AGAIN, now that its work has landed, and
/// require it to come back green.
///
/// A criterion still red here is reported UNPROVEN — the whole point of the
/// second half. Its earlier failure stays in the record ([`AcProof::proof`]);
/// what it no longer does is clear the criterion on its own.
pub(crate) fn confirm(root: &Path, spec: &str) -> NegativeCheckReport {
    run_pass(root, spec, root, Pass::Confirm, false, &BTreeSet::new())
}

/// Take the REMOVAL pass for `spec`: run each criterion that was CONFIRMED
/// green against `tree` — a checkout of the project with the work taken away —
/// and refuse the ones still green there.
///
/// `removed_text` is what the strip took OUT of that tree
/// ([`super::work_removed::RemovedTree::removed_text`]); it is what lets the
/// pass decline the criteria whose own evidence went with the work instead of
/// booking their guaranteed red as a proof.
///
/// The spec artefacts and the ledger are read and written under `root`; only
/// the commands run in `tree`. Keeping the two apart is the whole safety
/// property of this pass: the record of a run lands beside the spec, never
/// inside the scratch checkout that is about to be deleted.
pub(crate) fn removal(
    root: &Path,
    spec: &str,
    tree: &Path,
    removed_text: &BTreeSet<String>,
) -> NegativeCheckReport {
    run_pass(root, spec, tree, Pass::Removal, false, removed_text)
}

/// The CONFIRMATION taken from INSIDE `mustard-rt` itself — what
/// [`crate::commands::pipeline::close_pipeline`] runs, so the pipeline takes
/// the second half of the proof instead of leaving it to whoever remembers the
/// `--confirm` flag.
///
/// The only difference from [`confirm`] is what it does with a criterion whose
/// command rebuilds this binary: it declines to attempt it and says so
/// ([`Confirmation::NotTaken`]), rather than spending the deadline on a compile
/// that cannot link and recording the resulting non-answer as a finding about
/// the criterion.
pub(crate) fn confirm_in_process(root: &Path, spec: &str) -> NegativeCheckReport {
    run_pass(root, spec, root, Pass::Confirm, true, &BTreeSet::new())
}

/// All three passes, in one engine — see [`Pass`] for why it is a parameter.
///
/// `tree` is the working directory each criterion's command runs in. It is
/// `root` for both of the first two passes and a stripped scratch checkout for
/// the removal pass; the spec, the ledger and every reported path always come
/// from `root`.
///
/// `in_process` is only ever read by the confirmation pass — see
/// [`confirm_in_process`]; `removed_text` only by the removal pass — see
/// [`removal`]. Both are empty for the passes that do not answer to them.
fn run_pass(
    root: &Path,
    spec: &str,
    tree: &Path,
    pass: Pass,
    in_process: bool,
    removed_text: &BTreeSet<String>,
) -> NegativeCheckReport {
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
        if pass == Pass::Removal {
            // The third transition. It speaks only about a criterion the ledger
            // already carries with a GREEN confirmation — see `remove_one`.
            criteria.push(remove_one(
                tree,
                &item.id,
                &item.command,
                expect,
                recorded,
                removed_text,
            ));
            continue;
        }
        if pass == Pass::Confirm {
            // The confirmation only ever speaks about a criterion the ledger
            // already carries. One it does not is missing its RED proof, and a
            // green run here would answer a question nobody asked.
            criteria.push(confirm_one(
                tree,
                &item.id,
                &item.command,
                expect,
                recorded,
                in_process,
            ));
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
    let removed_red = criteria.iter().filter(|c| c.removal == Removal::Red).count();
    let survived = criteria
        .iter()
        .filter(|c| c.removal == Removal::Survived)
        .count();
    let evidence_removed = criteria
        .iter()
        .filter(|c| c.removal == Removal::EvidenceRemoved)
        .count();

    // The ledger is written whichever way the verdicts fell: the proofs already
    // obtained are the expensive part of this run and must not be lost because a
    // sibling criterion is still unproven.
    let ledger = AcProofLedger {
        spec: slug.clone(),
        criteria: criteria.clone(),
        // Preserved verbatim — the amendment operation is this array's author.
        amendments: previous.amendments,
        // Likewise: the ADD door writes this one, every pass preserves it.
        additions: previous.additions,
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
        removed_red,
        survived,
        evidence_removed,
        taken_away: Vec::new(),
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

/// Take the REMOVAL pass end to end: cut the scratch tree, run the pass in it,
/// and let the handle drop so the tree is removed either way.
///
/// A tree that could not be built is an ENGINE error, never a verdict — the
/// same separation `spec-not-found` already draws. `removal-<reason>` names
/// which half failed, because "no cached diff" and "git could not cut a
/// worktree" ask for different actions.
fn take_removal(root: &Path, spec: &str, from: &str) -> NegativeCheckReport {
    let Some(spec_file) = resolve_spec_file(root, spec) else {
        return NegativeCheckReport::aborted(
            Pass::Removal,
            Some(spec.to_string()),
            "spec-not-found",
        );
    };
    let spec_dir = spec_file.parent().unwrap_or(root).to_path_buf();
    match super::work_removed::build(root, &spec_dir, from) {
        Ok(tree) => {
            let mut report = removal(root, spec, tree.path(), tree.removed_text());
            report.taken_away = tree.taken_away().to_vec();
            report
        }
        Err(reason) => NegativeCheckReport::aborted(
            Pass::Removal,
            Some(spec.to_string()),
            &format!("removal-{reason}"),
        ),
    }
}

/// The revision the removal pass restores the work to when the caller names
/// none: the merge base of `HEAD` and the project's primary integration base
/// (`mustard.json#git.flow`), which is where the spec's work branch was cut
/// from. Agnostic — the branch name is the project's, never a literal here.
fn default_removal_from(root: &Path) -> String {
    let base = crate::shared::context::project_config_cached(root)
        .git
        .primary_base();
    for candidate in [format!("origin/{base}"), base] {
        if let Some(merge_base) =
            crate::commands::git_settle::git_out(root, &["merge-base", "HEAD", &candidate])
        {
            if !merge_base.is_empty() {
                return merge_base;
            }
        }
    }
    // Nothing resolved: the previous commit is the smallest honest guess, and
    // `unknown-revision` reports it when even that does not exist.
    "HEAD~1".to_string()
}

/// Dispatch `mustard-rt run ac-negative-check`.
///
/// `confirm` selects the SECOND pass (`--confirm`): the criteria that cleared
/// the red proof are run again, after their work has landed, and must now come
/// back green. `removal` selects the THIRD (`--removal`): each confirmed
/// criterion runs against a scratch tree with its work taken away and must come
/// back red. Three invocations rather than one automatic chain, because each
/// answer is correct only at its own moment — red at PLAN, green after EXECUTE,
/// red again only once somebody pays for the scratch tree.
pub fn run(spec: Option<&str>, confirm: bool, removal: bool, from: Option<&str>) {
    let root = PathBuf::from(crate::shared::context::project_dir());
    let pass = if removal {
        Pass::Removal
    } else if confirm {
        Pass::Confirm
    } else {
        Pass::Proof
    };
    let report = match spec.map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => match pass {
            Pass::Removal => {
                let from = from
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map_or_else(|| default_removal_from(&root), str::to_string);
                take_removal(&root, spec, &from)
            }
            Pass::Confirm => self::confirm(&root, spec),
            Pass::Proof => check(&root, spec),
        },
        None => NegativeCheckReport::aborted(pass, None, "spec-required"),
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
    /// A DIFFERENT green command, for the tests that need the recorded proof to
    /// stop applying because the command STRING changed. It must differ as text
    /// while staying green on both shells — appending an argument does not
    /// qualify, since a POSIX shell refuses `cd` with two of them.
    const OTHER_GREEN_COMMAND: &str = "cd \".\"";

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
                removal: Removal::NotTaken,
                removal_exit: None,
                reason: None,
                stderr_excerpt: String::new(),
            }],
            amendments: vec![serde_json::json!({ "id": "AC-1", "reason": "from wave 3" })],
            additions: Vec::new(),
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
             - **AC-1** — the behaviour holds.\n  Command: `{OTHER_GREEN_COMMAND}`\n\
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

    /// AC-3 — the THIRD transition, and the LIMIT it declares instead of
    /// papering over.
    ///
    /// Three criteria, built so the differences between them are the whole
    /// test, with the work materialised literally as directories:
    ///
    /// * `AC-1` asserts the directory the work creates — `cd` into it. Red
    ///   before, green after, red again in a tree without it, and its own
    ///   evidence (the command's words) outlives the strip: a red the missing
    ///   behaviour earned.
    /// * `AC-2` asserts something OUTSIDE the work — a directory on both trees.
    ///   Red before (it did not exist then), green after, and STILL green with
    ///   the work removed. That is the finding no earlier pass can make.
    /// * `AC-3` names a word the STRIP ITSELF deleted, IN ITS COMMAND. Its red
    ///   is guaranteed before anything runs, so booking it as proof would be
    ///   the pass claiming a coverage it never had. It is declined by name
    ///   instead — never run, never counted with the reds.
    /// * `AC-4` names a word the strip deleted ONLY IN ITS `Expect:` REGEX —
    ///   its command names nothing the removal touched. This is the shape that
    ///   reads a file the work writes into, and the executor grades it with the
    ///   expectation just as much as with the command, so its red is just as
    ///   pre-decided. The stripped tree is rigged so that RUNNING it would come
    ///   back red and be booked as proof: consulting the command alone is a
    ///   FAILING assertion here, not merely an incomplete one.
    ///
    /// Two-sided four ways over the SAME pass and the SAME ledger, so it
    /// cannot pass by calling everything red, everything survived, or
    /// everything unjudgeable.
    #[test]
    fn removal_refuses_a_survivor_and_declines_what_it_cannot_judge() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // "read this file" is spelled `type` by `cmd.exe` and `cat` by a POSIX
        // shell. Chosen at RUN time, not compile time: on Windows the AC shell
        // is now whichever one `crate::util::platform` resolves, so a
        // `cfg!(windows)` fixture would hand `type` to a POSIX shell — where it
        // is a builtin that reports command types and never reads a file.
        let read = {
            #[cfg(windows)]
            {
                if crate::util::platform::posix_shell().is_some() { "cat" } else { "type" }
            }
            #[cfg(not(windows))]
            {
                "cat"
            }
        };
        // The tree WITH the work: all three directories, plus the file AC-4
        // reads — its marker lives in the CONTENT, never in the command.
        let with_work = root.join("with-work");
        std::fs::create_dir_all(with_work.join("behaviour")).unwrap();
        std::fs::create_dir_all(with_work.join("dragged-along")).unwrap();
        std::fs::create_dir_all(with_work.join("own-evidence")).unwrap();
        std::fs::create_dir_all(with_work.join("carrier")).unwrap();
        std::fs::write(with_work.join("carrier").join("note.txt"), "beta_marker\n").unwrap();
        // The tree with the work TAKEN AWAY: the dragged-along directory, and
        // the carrier file WITHOUT its marker — the strip took the marker out
        // of the content, not the file. Running AC-4 here would exit 0, miss
        // its `Expect:` and be booked as a proven red. That is the manufactured
        // red this criterion exists to keep out of the ledger.
        let stripped = root.join("stripped");
        std::fs::create_dir_all(stripped.join("dragged-along")).unwrap();
        std::fs::create_dir_all(stripped.join("carrier")).unwrap();
        std::fs::write(stripped.join("carrier").join("note.txt"), "nothing here\n").unwrap();
        // What the strip took OUT of that tree, as `work_removed` reports it.
        // `behaviour` is deliberately NOT in it: the strip removed the
        // directory, not the criterion's ability to ask about it.
        let removed_text: BTreeSet<String> = ["own-evidence".to_string(), "beta_marker".to_string()]
            .into_iter()
            .collect();

        let body = format!(
            "# S\n\n## Acceptance Criteria\n\
             - **AC-1** — the behaviour is there.\n  Command: `cd behaviour`\n\
             - **AC-2** — a word from outside the work.\n  Command: `cd dragged-along`\n\
             - **AC-3** — a word the strip itself deleted.\n  Command: `cd own-evidence`\n\
             - **AC-4** — a word the strip deleted, named only in the expectation.\n  \
             Command: `cd carrier && {read} note.txt`\n  Expect: `beta_marker`\n\
             - **AC-5** — build green.\n  Command: `cd .`\n"
        );
        let spec_dir = seed(root, "third", &body);

        // 1. The RED proof, against a tree carrying none of the directories —
        //    `root` itself holds only the two scratch trees.
        let proof = check(root, "third");
        assert_eq!(entry(&proof, "AC-1").proof, Proof::Red);
        assert_eq!(entry(&proof, "AC-2").proof, Proof::Red, "red before, honestly");
        assert_eq!(entry(&proof, "AC-3").proof, Proof::Red);
        assert_eq!(entry(&proof, "AC-4").proof, Proof::Red);
        assert_eq!(
            entry(&proof, "AC-1").removal,
            Removal::NotTaken,
            "the red pass never claims a removal it did not take"
        );

        // 2. The CONFIRMATION, against the tree WITH the work: all three pass.
        let confirmed = removal_free_confirm(&with_work, root, "third");
        assert_eq!(entry(&confirmed, "AC-1").confirmation, Confirmation::Green);
        assert_eq!(entry(&confirmed, "AC-2").confirmation, Confirmation::Green);
        assert_eq!(entry(&confirmed, "AC-3").confirmation, Confirmation::Green);
        assert_eq!(
            entry(&confirmed, "AC-4").confirmation,
            Confirmation::Green,
            "the carrier file holds the marker while the work is there"
        );

        // 3. The REMOVAL, against the tree with the work taken away.
        let removed = removal(root, "third", &stripped, &removed_text);
        assert_eq!(removed.pass, "removal");

        let tied = entry(&removed, "AC-1");
        assert_eq!(tied.verdict, Verdict::Proven, "red with the work gone clears it");
        assert_eq!(tied.removal, Removal::Red);
        assert!(tied.removal_exit.is_some(), "the command genuinely ran");
        assert!(tied.reason.is_none(), "a criterion tied to the behaviour needs no remedy");
        assert_eq!(
            (tied.proof, tied.confirmation),
            (Proof::Red, Confirmation::Green),
            "the earlier columns are carried over, never overwritten"
        );

        let vacuous = entry(&removed, "AC-2");
        assert_eq!(
            vacuous.verdict,
            Verdict::Unproven,
            "a criterion that survives its own work's removal verifies nothing"
        );
        assert_eq!(vacuous.removal, Removal::Survived);
        let reason = vacuous.reason.clone().unwrap_or_default();
        assert!(reason.contains("with the work taken away"), "{reason}");
        assert!(
            reason.contains("asserts the BEHAVIOUR"),
            "the remedy names what to rewrite it into: {reason}"
        );

        // The limit, stated by the mechanism rather than by a comment. `cd
        // own-evidence` WOULD have come back red in the stripped tree — the
        // directory is not there — and that red says nothing, because the strip
        // is what took the directory away. It is declined, not booked.
        let unjudgeable = entry(&removed, "AC-3");
        assert_eq!(unjudgeable.removal, Removal::EvidenceRemoved);
        assert!(
            unjudgeable.removal_exit.is_none(),
            "the command must not even be spawned: a guaranteed red is not evidence"
        );
        assert_eq!(
            unjudgeable.verdict,
            Verdict::Proven,
            "the removal did not speak, so it does not unmake what the earlier passes proved"
        );
        let declined = unjudgeable.reason.clone().unwrap_or_default();
        assert!(declined.contains("own-evidence"), "it names the word: {declined}");
        assert!(
            declined.contains("NOT TAKEN"),
            "and says the removal was not taken, never that the criterion failed: {declined}"
        );

        // The SAME limit reached through the other half of the evidence. AC-4's
        // command names nothing the strip took — `cd carrier && <read>
        // note.txt` would have RUN, exited 0, missed its `Expect:` and landed
        // in the ledger as a proven red. The marker it grades against is gone
        // from the tree, so that red was decided before the process started
        // just as surely as AC-3's, and the pass must decline it by name.
        let in_the_expectation = entry(&removed, "AC-4");
        assert_eq!(
            in_the_expectation.removal,
            Removal::EvidenceRemoved,
            "evidence in the `Expect:` regex is evidence: {:?}",
            in_the_expectation.reason
        );
        assert!(
            in_the_expectation.removal_exit.is_none(),
            "and it must not be spawned either — a manufactured red is not evidence"
        );
        assert_eq!(
            in_the_expectation.verdict,
            Verdict::Proven,
            "the removal did not speak about it, so it unmakes nothing"
        );
        let by_name = in_the_expectation.reason.clone().unwrap_or_default();
        assert!(
            by_name.contains("beta_marker"),
            "it names the word the strip took, wherever the criterion carried it: {by_name}"
        );

        assert_eq!(
            (removed.removed_red, removed.survived, removed.evidence_removed),
            (1, 1, 2),
            "the declined criteria are counted apart from the reds"
        );
        assert!(!removed.ok, "a survivor must not report ok");
        assert_eq!(exit_code(&removed), 2, "the blocking exit code");

        // 4. A criterion nobody CONFIRMED has nothing to remove, and the removal
        //    does not become a back door to clearing it.
        let unconfirmed = removal(root, "third", &stripped, &removed_text);
        let trailing = entry(&unconfirmed, "AC-5");
        assert_eq!(trailing.verdict, Verdict::Exempt, "the trailing criterion stays exempt");
        assert_eq!(trailing.removal, Removal::NotTaken, "and is never run");

        // The ledger carries all three columns, so a later reader never re-runs
        // anything to know what happened at each transition.
        let ledger: AcProofLedger =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join(AC_PROOF_JSON)).unwrap())
                .unwrap();
        let recorded = ledger.criteria.iter().find(|c| c.id == "AC-1").unwrap();
        assert_eq!(
            (recorded.proof, recorded.confirmation, recorded.removal),
            (Proof::Red, Confirmation::Green, Removal::Red)
        );
    }

    /// Take the confirmation with the criteria RUN in `tree` while the spec and
    /// ledger stay under `root` — the shape the removal pass needs to set up,
    /// expressed through the same engine so the test cannot drift from it.
    fn removal_free_confirm(tree: &Path, root: &Path, spec: &str) -> NegativeCheckReport {
        run_pass(root, spec, tree, Pass::Confirm, false, &BTreeSet::new())
    }

    /// The removal classification, as a table — the third mirror. `fail` is the
    /// ONLY status that clears, and `pass` earns its own SURVIVED value: a
    /// command still green with its work gone is a finding, not an absence.
    #[test]
    fn only_a_failing_command_clears_the_removal() {
        assert_eq!(classify_removal("fail").0, Verdict::Proven);
        assert_eq!(classify_removal("fail").1, Removal::Red);
        assert!(classify_removal("fail").2.is_none());
        assert_eq!(classify_removal("pass").1, Removal::Survived);
        assert_eq!(classify_removal("timeout").1, Removal::NoVerdict);
        assert_eq!(classify_removal("skip").1, Removal::NotAttempted);
        for status in ["pass", "timeout", "skip"] {
            let (verdict, removal, reason) = classify_removal(status);
            assert_eq!(verdict, Verdict::Unproven, "{status}");
            assert!(reason.is_some(), "{status} must name its remedy");
            assert_ne!(removal, Removal::NotTaken, "{status} WAS taken");
        }
        // NEVER TAKEN is the default and no executed status produces it — the
        // same separation the other two columns draw between absence and a
        // wrong colour.
        assert_eq!(Removal::default(), Removal::NotTaken);
        // And neither does any status produce EVIDENCE REMOVED: that value says
        // the command was never spawned, so a function that reads an executor
        // status must be unable to reach it — otherwise "not run" and "ran and
        // failed" would share a spelling again.
        for status in ["fail", "pass", "timeout", "skip", "anything-else"] {
            assert_ne!(classify_removal(status).1, Removal::EvidenceRemoved, "{status}");
        }
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
