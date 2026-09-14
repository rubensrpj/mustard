//! `mustard-rt run ac-amend` — deliberately change an acceptance criterion
//! AFTER the spec artefacts are frozen, and prove the replacement still knows
//! how to fail.
//!
//! ## Why this exists
//!
//! [`crate::commands::review::ac_negative_check`] refuses a spec whose criterion
//! already exits green before its work exists — a VACUOUS criterion. The only
//! honest answer to that refusal is to rewrite the criterion. Done by hand that
//! rewrite is two silent traps at once:
//!
//! 1. **The replacement is never proven.** A criterion swapped for another
//!    already-green command clears nothing; it merely stops the tool complaining.
//!    So this door asks the SAME engine about the replacement and REFUSES it on
//!    anything but a red. A replacement that already passes proves exactly as
//!    little as the original did.
//!
//! ## The exceptions: a predecessor whose red was never evidence
//!
//! Two criteria the red rule cannot repair, and they share one shape: the
//! predecessor's red says nothing about the work, so demanding a red from its
//! replacement demands a criterion that lies about a feature that exists.
//!
//! 1. **INEXECUTABLE.** The confirmation pass found a criterion the executor
//!    could not attempt AT ALL after its work landed
//!    ([`ac_negative_check::Confirmation::Inexecutable`]). The command is broken
//!    whatever the work does — and by then the work IS done, so the corrected
//!    command legitimately PASSES.
//!
//! 2. **UNSATISFIABLE.** The command runs fine, but the predecessor's own
//!    `Expect:` regex cannot match the output that command produces: a count
//!    anchored at `^` against `grep -c`, which prints `file:count` and never a
//!    bare number. Such a criterion is red before the work and red after it, so
//!    its red is a property of the regex rather than a finding about the tree.
//!    Caught at drafting time by the `expect-anchored-against-prefixed-output`
//!    lint; this door is for one that shipped before that lint existed. Found in
//!    the field, 2026-08-14, on a criterion that failed QA over a record it had
//!    itself asked for and received.
//!
//! For those recorded states, and only them, a replacement that comes back GREEN
//! is accepted, and its record carries a green CONFIRMATION — the evidence the
//! approval gate reads. Everything else keeps refusing a replacement that is not
//! red. Neither exception is a knob and neither can be asked for: each is
//! unlocked by a fact about the SUPERSEDED criterion — one recorded by the
//! engine, one read off the criterion's own command and regex — which is why
//! they cannot be used to smuggle a vacuous criterion past the door.
//! 2. **Only the root is edited.** `wave-plan.md` carries the criterion lines
//!    too — the union QA executes — and a root-only edit leaves QA running the
//!    superseded command. The wave specs carry NO copy: each names only WHICH
//!    ids it satisfies (`satisfies:` frontmatter), and the dispatch prompt cuts
//!    that CURRENT section — the very file the judge reads, the union first —
//!    at dispatch time, so an amendment written here reaches every wave's
//!    prompt without touching a frozen layout, and reaches it even after a
//!    rewave archives the root to `spec.original.md`.
//!
//! ## What this door does NOT do: ADD
//!
//! Every refusal here presupposes a PREDECESSOR — the replacement must prove it
//! can fail against a tree where the criterion it supersedes already lived. An
//! id the spec does not carry has no predecessor, so it is not an amendment at
//! all and this door refuses it (`unknown_criterion`). Introducing one is
//! [`super::ac_add`], a door of its own taking the same negative proof.
//!
//! ## The name
//!
//! `ac-amend`, never a bare `amend`: `amend-finalize`
//! ([`crate::commands::agent::amend_finalize`]) already means the unrelated
//! session-end amendment window, and one bare word for two meanings is how a
//! caller reaches for the wrong door.
//!
//! ## Refusal, not silence
//!
//! Four refusals, each of which writes NOTHING anywhere: a blank reason, an
//! unknown spec directory, an unknown criterion id, and — the load-bearing one —
//! a replacement the negative test does not report as proven. Every refusal
//! names its reason AND the one action that clears it, because a gate whose
//! refusal cannot be acted on teaches the caller to route around it.
//!
//! ## One engine, one parser
//!
//! The proof comes from [`ac_negative_check::prove_one`] and the criterion lines
//! are located with [`qa_run::parse_ac_header`] — the very reader that grades
//! them. Nothing about "is this proven" or "is this an AC line" is re-derived
//! here. The write is confirmed by RE-READING each artefact through
//! [`qa_run::parse_ac_items`]: the amendment reports whether it landed instead
//! of assuming it did.
//!
//! ## Where the timestamp lives
//!
//! In the ledger's `amendments` array, never on stdout — the `run` surface is
//! snapshot-compared and must stay byte-stable.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::commands::review::ac_negative_check::{
    self, AcProof, AcProofLedger, Confirmation, Proof, Verdict, AC_PROOF_JSON,
};
use crate::commands::review::qa_run;
use crate::commands::spec::spec_sections;
use mustard_core::io::fs as mfs;

/// The canonical section key of the acceptance-criteria heading, in the one
/// spelling the shared resolver understands (EN and PT both resolve through it).
///
/// `pub(super)` so the ADD door ([`super::ac_add`]) resolves the same heading:
/// two spellings of "which section holds the criteria" is how a writer would
/// land a criterion where no reader looks.
pub(super) const AC_SECTION_KEY: &str = "acceptanceCriteria";

/// Options for `mustard-rt run ac-amend`.
#[derive(Debug, Clone)]
pub struct AcAmendOpts {
    /// Spec slug under `.claude/spec/`.
    pub spec: String,
    /// The criterion id to amend (`AC-2`, `AC-W4-1`, …).
    pub ac: String,
    /// The replacement command.
    pub command: String,
    /// The replacement `Expect:` evidence regex. Omitted: the criterion keeps
    /// the regex it already carries — each flag changes only what it names.
    pub expect: Option<String>,
    /// The replacement EARS statement. Omitted: the statement is left alone.
    pub statement: Option<String>,
    /// Why the criterion is being changed. Never blank.
    pub reason: String,
    /// The replacement's `Control:` — a command that must come back GREEN
    /// against the tree as it is, proving the replacement's red came from the
    /// missing behaviour rather than from a filter that selects nothing.
    ///
    /// Optional for every command shape. Omitted, the criterion keeps the
    /// control its line already carries; given, it is taken in the same pass
    /// as the red proof and written onto the line. Worth declaring for a
    /// FILTERED TEST RUNNER (`cargo test <unknown>` answers `0 passed` with
    /// exit 0), which the drafting lint names as `test-ac-no-control` — but its
    /// absence never refuses the amendment.
    pub control: Option<String>,
    /// Take the proof against ANOTHER tree — a checkout that does not yet carry
    /// the work — instead of the one being amended.
    ///
    /// The negative proof asks "can this criterion fail?", and it can only be
    /// answered where the behaviour does NOT exist. A criterion corrected after
    /// the code lands therefore has no honest answer in the current tree: the
    /// replacement comes back green, and green proves nothing.
    ///
    /// Measured on this repository: three criteria named tests the
    /// implementation had not created (`cargo test <unknown>` answers
    /// `0 passed` with exit 0, so they would have passed vacuously). Correcting
    /// them meant hiding each test, running the amend, and restoring it — a
    /// contrivance that leaves no trace of WHERE the red was taken.
    ///
    /// The tree is recorded in the ledger alongside the verdict, so the
    /// amendment says where its evidence came from. The spec is still read and
    /// rewritten in the CURRENT tree; only the command runs elsewhere.
    pub proof_tree: Option<PathBuf>,
}

/// JSON report printed on stdout. Deterministic: repo-relative paths, sorted,
/// and no timestamp (that lives in the ledger).
#[derive(Debug, Serialize)]
pub(crate) struct AcAmendReport {
    /// `true` only when the amendment was accepted AND every write is confirmed.
    pub(crate) ok: bool,
    /// The spec the criterion belongs to.
    pub(crate) spec: String,
    /// The criterion id, normalised (`AC-2`).
    pub(crate) ac: String,
    /// The command the criterion now carries.
    pub(crate) command: String,
    /// The evidence regex the criterion now carries, when it has one.
    pub(crate) expect: Option<String>,
    /// The command the amendment superseded (`None` when nothing was accepted).
    pub(crate) superseded_command: Option<String>,
    /// The evidence regex the amendment superseded.
    pub(crate) superseded_expect: Option<String>,
    /// The proof the negative test took on the REPLACEMENT — recorded whichever
    /// way it fell, so a refusal shows exactly what the engine saw.
    pub(crate) proof: Option<AcProof>,
    /// Every artefact whose criterion line was rewritten AND confirmed on
    /// re-read, as repo paths with forward slashes.
    pub(crate) rewritten: Vec<String>,
    /// Where the proof ledger lives, when it was updated.
    pub(crate) ledger: Option<String>,
    /// Refusal / failure code: `blank_reason`, `unknown_spec`,
    /// `unknown_criterion`, `replacement_not_proven`, `rewrite_failed`,
    /// `ledger_write_failed`.
    pub(crate) error: Option<String>,
    /// The one action that clears the refusal. Absent when nothing is wrong.
    pub(crate) remedy: Option<String>,
}

impl AcAmendReport {
    /// A report for an amendment that was refused before anything was written.
    fn refused(opts: &AcAmendOpts, ac: &str, error: &str, remedy: &str) -> Self {
        Self {
            ok: false,
            spec: opts.spec.clone(),
            ac: ac.to_string(),
            command: opts.command.clone(),
            expect: opts.expect.clone(),
            superseded_command: None,
            superseded_expect: None,
            proof: None,
            rewritten: Vec::new(),
            ledger: None,
            error: Some(error.to_string()),
            remedy: Some(remedy.to_string()),
        }
    }
}

/// One amendment's entry in the ledger's `amendments` array — the id, what it
/// superseded, what replaced it, why, and when.
#[derive(Debug, Serialize)]
struct Amendment {
    /// The criterion that was amended.
    id: String,
    /// When the amendment was accepted. The ledger is where a timestamp
    /// belongs: stdout must stay byte-stable.
    at: String,
    /// The stated reason — the whole point of recording an amendment.
    reason: String,
    /// The command the amendment replaced.
    superseded_command: String,
    /// The evidence regex it replaced, when there was one.
    superseded_expect: Option<String>,
    /// The statement it replaced — present only when `--statement` was given.
    superseded_statement: Option<String>,
    /// The command the criterion now carries.
    command: String,
    /// The evidence regex it now carries.
    expect: Option<String>,
    /// The statement it now carries — present only when `--statement` was given.
    statement: Option<String>,
    /// The artefacts the amendment rewrote, as repo paths.
    rewrote: Vec<String>,
    /// WHERE the red was taken, when it was not this tree.
    ///
    /// A proof carries no weight without the state it was taken against, and a
    /// criterion corrected after the work landed can only come back red
    /// somewhere the work is absent. Recording the tree is what keeps
    /// `--proof-tree` an evidenced claim rather than an escape hatch: a reader
    /// can go to that commit and take the same measurement.
    ///
    /// Absent for the ordinary case — the proof was taken here.
    #[serde(skip_serializing_if = "Option::is_none")]
    proof_tree: Option<String>,
}

/// Run one `git -C <tree> …` and return its trimmed stdout, or `None` when git
/// is absent, fails, or answers nothing.
fn git_in(tree: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(tree);
    cmd.args(args);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The commit a proof tree stands on, as a short hash.
///
/// **The question is not "is this tree a repository root" — it is "did git
/// answer about the tree the operator meant, or about the one being amended?"**
/// `rev-parse` walks up through parent directories, so a plain directory nested
/// under the checkout being amended answers with THAT repository's HEAD — the
/// commit which carries the work, the exact opposite of what the flag claims. A
/// reader sent there finds the criterion green, so the record reads as a lie
/// about evidence rather than as a missing field.
///
/// An earlier version asked for exact equality with `--show-toplevel`, which
/// also refused a SUBDIRECTORY of the correct worktree — a tree git names the
/// right commit for. Review measured it. So the test is which repository the
/// walk landed in: any tree resolving to a repository other than the one being
/// amended is answered for, wherever inside it the path points.
///
/// **Both sides are asked of GIT, and that is load-bearing.** A second version
/// compared git's answer for `tree` against the `amending` PATH — which is the
/// project anchor (`mustard.json` + `.claude/`), not the repository root. In a
/// monorepo the two differ, so the equality could never hold and a plain nested
/// directory borrowed the amended tree's own HEAD again, in exactly the layout
/// this repository has. Review measured it in an `apps/thing/` project: the
/// ledger claimed the negative proof was taken at the commit carrying the work.
/// Comparing repository root against repository root is the only form that
/// holds wherever the anchor sits.
///
/// `None` when git cannot answer, or when it answers about the repository being
/// amended — see [`proof_tree_record`] for why that is recorded as nothing
/// rather than as a path.
pub(crate) fn proof_tree_commit(tree: &Path, amending: &Path) -> Option<String> {
    let resolved = git_root(tree)?;
    // The REPOSITORY the amended tree belongs to, asked of git the same way —
    // never the project anchor, which in a monorepo is a subdirectory of it.
    if git_root(amending).is_some_and(|here| here == resolved) {
        return None;
    }
    git_in(tree, &["rev-parse", "--short", "HEAD"])
}

/// The canonical path of the git repository `dir` belongs to, or `None` when
/// git cannot say.
fn git_root(dir: &Path) -> Option<std::path::PathBuf> {
    let toplevel = git_in(dir, &["rev-parse", "--show-toplevel"])?;
    std::fs::canonicalize(toplevel).ok()
}

/// What the ledger records about WHERE a proof was taken.
///
/// The commit, or nothing at all. **Never the path**: `ac-proof.json` is a
/// versioned artefact, and this crate's guard requires its output to be
/// byte-stable — an absolute `/tmp/…` differs on every machine and would make
/// the file churn per checkout. The path is also useless as evidence on its own
/// terms: the doc for `--proof-tree` records the commit precisely BECAUSE a
/// worktree under `/tmp` is gone by the time anyone reads the ledger, so
/// falling back to that path writes down the very thing it argued was worthless.
///
/// A proof git cannot speak for is therefore recorded as absent, and the report
/// says so through the `remedy` a reader can act on rather than through a field
/// that looks like evidence and is not.
pub(crate) fn proof_tree_record(tree: Option<&Path>, amending: &Path) -> Option<String> {
    proof_tree_commit(tree?, amending)
}

/// The WAY OUT of a refusal whose replacement (or addition) came back GREEN in
/// the current tree, spelled out — the paragraph a refusal must carry, because
/// the door it names has existed the whole time and nobody found it.
///
/// Measured in the field: after the work lands, every GOOD criterion passes,
/// so a criterion corrected AFTER its work cannot come back red here, and the
/// only replacement the proof would accept is one that fails for some other
/// reason. The operator read that as a dead end. It is not: `--proof-tree`
/// takes the red where the work is absent, and the recipe is three commands.
/// A refusal that sends the reader looking without saying what to look for
/// teaches them to route around the gate — the same reason every other reason
/// in this family names its one action.
///
/// `door` is the subcommand to repeat (`ac-amend` / `ac-add`); `proof_tree` is
/// the tree the proof was JUST taken in, when one was named — the recipe would
/// be a loop then, so the paragraph says the tree given still carries the work
/// instead.
///
/// `pub(crate)` so `ac_add` says the same thing in the same words: two
/// spellings of the way out is how one of the doors would drift back to silent.
pub(crate) fn green_in_this_tree_way_out(door: &str, proof_tree: Option<&Path>) -> String {
    match proof_tree {
        Some(tree) => format!(
            "The proof was taken in `--proof-tree {}` and the command passes THERE too, so that \
             checkout still carries the work (or the criterion is satisfied by something the work \
             never did) — point `--proof-tree` at a commit from BEFORE the work landed, or rewrite \
             the command so it asserts the behaviour.",
            tree.display()
        ),
        None => format!(
            "A criterion corrected AFTER its work landed cannot come back red here: the behaviour \
             already exists in this tree, so every good command passes. Take the proof where the \
             work is absent instead — `--proof-tree` runs the command in another checkout while \
             every write still lands in this one:\n\
             git worktree add --detach <dir> <commit-before-the-work>\n\
             mustard-rt run {door} … --proof-tree <dir>\n\
             git worktree remove <dir>"
        ),
    }
}

/// What a rewrite must apply to one criterion.
#[derive(Debug, Clone)]
struct Rewrite {
    /// The replacement command — always applied.
    command: String,
    /// The replacement evidence regex, applied ONLY when `--expect` was given.
    /// `None` means "leave whatever regex is there", not "remove it".
    expect: Option<String>,
    /// The replacement statement, applied only when `--statement` was given.
    statement: Option<String>,
    /// The replacement `Control:` command, applied ONLY when `--control` was
    /// given. `None` means "leave whatever control is there", not "remove it" —
    /// the same semantics as [`Self::expect`], and load-bearing for the same
    /// reason: a command-only amendment must not silently strip the one marker
    /// that separates a behaviour red from an empty-selection red.
    ///
    /// **Why the flag has to reach the LINE and not only the ledger.** The
    /// proof engine takes `--control` and records it in `control_command`, but
    /// the next [`ac_negative_check`] pass re-reads the MARKDOWN. A criterion
    /// admitted through `--control` whose line carried no `Control:` was
    /// refused again at the approval gate — the door opened and the gate behind
    /// it closed, which is worse than no flag at all.
    control: Option<String>,
}

/// Normalise a criterion id to the spelling the parser yields: uppercase, and
/// `AC-` prefixed when the caller passed the bare number. Pure, total.
///
/// `pub(super)` so the ADD door normalises an id EXACTLY as the amend door
/// does — otherwise `--ac 5` would add `AC-5` through one door and fail to find
/// it through the other.
pub(super) fn normalise_id(raw: &str) -> String {
    let id = raw.trim().to_uppercase();
    if id.starts_with("AC-") {
        id
    } else {
        format!("AC-{id}")
    }
}

// ---------------------------------------------------------------------------
// Line surgery
//
// The reader side of these lines is `qa_run`'s `extract_marker`: the value sits
// after the LAST `Command:` / `Expect:` label on the line, backtick-quoted (or
// bare, running to end of line). These writers mirror that exactly, and the
// unit tests below assert the ROUND TRIP — a rewritten line is parsed back by
// `parse_ac_items` and must yield the new command. That round trip, not a
// shared helper, is what keeps writer and reader honest.
// ---------------------------------------------------------------------------

/// Byte index just past the LAST case-insensitive `marker` in `line`, or `None`.
/// `marker` is a lowercase `"label:"`. Never panics: an index that would land
/// mid-character (possible when lowercasing changes byte lengths) yields `None`.
fn marker_end(line: &str, marker: &str) -> Option<usize> {
    let end = line.to_lowercase().rfind(marker)? + marker.len();
    line.is_char_boundary(end).then_some(end)
}

/// Split a trailing `\r` off a line so a CRLF document survives surgery that
/// appends to the end of a line. Pure, total.
fn split_cr(line: &str) -> (&str, &str) {
    line.strip_suffix('\r').map_or((line, ""), |body| (body, "\r"))
}

/// Rewrite the value after the LAST `marker` on `line` to `value`, keeping the
/// label, the spacing around it and whatever follows the old value (a trailing
/// `Expect:` marker on the same line, a `\r`, a parenthetical). `None` when the
/// line carries no such marker.
fn replace_marker_value(line: &str, marker: &str, value: &str) -> Option<String> {
    let end = marker_end(line, marker)?;
    let (head, tail) = line.split_at(end);
    let ws_len = tail.len() - tail.trim_start().len();
    let (ws, rest) = tail.split_at(ws_len);
    // Quoted: the value ends at the closing backtick and the remainder stays.
    // Bare: the value runs to end of line, exactly as `extract_marker` reads it.
    let after = match rest.strip_prefix('`') {
        Some(inner) => inner.find('`').map_or("", |close| &inner[close + 1..]),
        None => split_cr(rest).1,
    };
    // A label with no space after it would glue onto the backtick.
    let ws = if ws.is_empty() { " " } else { ws };
    Some(format!("{head}{ws}`{value}`{after}"))
}

/// Give `line` a `marker` carrying `value` — rewriting the one it has, or
/// appending `` <label>: `<value>` `` when it has none.
///
/// The append lands on the COMMAND's own line on purpose: that is the one place
/// both AC shapes read the optional markers from. The one-line historical form
/// (`- [ ] AC-1: … Command: \`c\``) never looks at following lines at all, so a
/// marker appended below it would be silently ignored.
///
/// `marker` is the lowercase `"label:"` the READER matches; `label` is the
/// cased spelling written out. One body for both markers, because "rewrite it
/// or append it" is one rule — two copies is how `Expect:` and `Control:` would
/// drift into landing in different places.
fn ensure_marker(line: &str, marker: &str, label: &str, value: &str) -> String {
    if let Some(rewritten) = replace_marker_value(line, marker, value) {
        return rewritten;
    }
    let (body, cr) = split_cr(line);
    format!("{body} {label}: `{value}`{cr}")
}

/// Give `line` an `Expect:` marker carrying `value` — see [`ensure_marker`].
fn ensure_expect(line: &str, value: &str) -> String {
    ensure_marker(line, "expect:", "Expect", value)
}

/// Give `line` a `Control:` marker carrying `value` — see [`ensure_marker`].
///
/// The twin of [`ensure_expect`], and it exists for the reason
/// [`Rewrite::control`] states: without it a criterion admitted through
/// `--control` reached the ledger with a control and the spec line without one,
/// so the next `ac-negative-check` pass — which reads the MARKDOWN — refused it
/// all over again.
fn ensure_control(line: &str, value: &str) -> String {
    ensure_marker(line, "control:", "Control", value)
}

/// Rewrite the criterion's statement on its header line, keeping the bullet, the
/// id, the id→description separator and any inline `Command:` tail.
///
/// `after_sep_off` is the byte offset of the parser's `after_sep` slice — which
/// is a suffix of `line`, so the offset is exact.
fn replace_statement(line: &str, after_sep_off: usize, statement: &str) -> String {
    if !line.is_char_boundary(after_sep_off) {
        return line.to_string();
    }
    let (head, after) = line.split_at(after_sep_off);
    // Everything from an inline `Command:` marker onward is preserved verbatim.
    let (body, tail) = match after.to_lowercase().rfind("command:") {
        Some(i) if after.is_char_boundary(i) => after.split_at(i),
        _ => (after, ""),
    };
    // Split the body into: statement text | separator run | trailing whitespace.
    // `trim_end_matches` mirrors `qa_run::statement_of`, so what is stripped here
    // is exactly what that reader would have refused to call the statement.
    let trimmed = body.trim_end();
    let trailing_ws = &body[trimmed.len()..];
    let stmt_end = trimmed.trim_end_matches(['—', '-', ' ']).len();
    let separator = &trimmed[stmt_end..];
    format!("{head} {}{separator}{trailing_ws}{tail}", statement.trim())
}

/// WHICH line of a criterion's block an optional marker (`expect:`,
/// `control:`) must be written on.
///
/// The command's own line when it already carries the marker, or when the
/// criterion is in the one-line historical form — that form never looks at
/// following lines. Otherwise the block's standalone marker line, if there is
/// one; failing that the command line, where [`ensure_marker`] appends.
///
/// `command_line` is read from the OUTPUT (the command may already have been
/// rewritten there), while the search for a standalone line reads the pristine
/// `lines` — markers never move, so the two agree.
fn marker_line(
    command_line: &str,
    lines: &[&str],
    k: usize,
    block_end: usize,
    inline: bool,
    marker: &str,
) -> usize {
    if inline || marker_end(command_line, marker).is_some() {
        return k;
    }
    (k + 1..block_end)
        .find(|m| marker_end(lines[*m], marker).is_some())
        .unwrap_or(k)
}

/// `true` when the block of lines starting after an AC header ends at `line` —
/// the same stop rule [`qa_run::parse_ac_items`]'s lookahead applies.
fn ends_block(line: &str) -> bool {
    qa_run::parse_ac_header(line).is_some() || line.trim().is_empty() || line.starts_with("## ")
}

/// `true` when `line` carries neither a `Command:` nor an `Expect:` marker — a
/// line inside an AC block that is nothing but prose.
///
/// Used to decide which lines a statement rewrite CONSUMES: everything between
/// the header and the command line that is pure prose is part of the statement
/// being replaced. Anything carrying a marker is data the rewrite must not eat,
/// however odd its position.
fn is_statement_continuation(line: &str) -> bool {
    marker_end(line, "command:").is_none() && marker_end(line, "expect:").is_none()
}

/// Rewrite every criterion line for `id` in one markdown document.
///
/// Only `## Acceptance Criteria` sections are touched (the i18n-aware heading
/// resolver decides which those are), so a prose bullet elsewhere that happens
/// to mention `AC-2` is never edited. Every homonymous AC section is visited:
/// legacy drafts duplicated the heading, and a rewrite that skipped the extra
/// copy would leave a superseded command on disk.
///
/// ## A statement is a BLOCK, not a line
///
/// The drafter wraps a long EARS statement over several lines:
///
/// ```text
/// - **AC-1** — when a spec is closed, then the pipeline takes the
///   confirmation pass and records the verdict, instead of clearing on
///   the red proof alone
///   Command: `cargo test …`
/// ```
///
/// [`qa_run::parse_ac_header`] only ever reads the FIRST of those lines, so a
/// rewrite that replaces just it leaves the remaining lines on disk, orphaned
/// under a statement they no longer continue — the reader gets the new sentence
/// welded to the tail of the old one. Found in review (2026-07-28) after
/// amending a criterion of this very spec, and cleaned by hand; the hand is exactly
/// what this door exists to replace.
///
/// So a `--statement` rewrite consumes the WHOLE block: the header line is
/// replaced and every pure-prose line between it and the `Command:` line is
/// dropped. Lines carrying a marker are never dropped — see
/// [`is_statement_continuation`].
///
/// And the block STOPS where the parser's lookahead stops, not where the
/// command sits: a criterion carrying no `Command:` line at all (a wave-plan
/// bullet, a criterion whose command was cut) still has continuation lines, and
/// deriving the boundary from the command line is what left that ONE shape
/// orphaned after the multi-line fix.
///
/// `None` when the document carries nothing to change.
fn rewrite_markdown(body: &str, id: &str, plan: &Rewrite) -> Option<String> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
    // Lines the statement rewrite consumed. Marked rather than removed as we
    // go: `out` is index-parallel to `lines`, and a mid-loop deletion would
    // shift every offset the surgery below still needs.
    let mut consumed = vec![false; lines.len()];
    let mut changed = false;

    let mut i = 0;
    while i < lines.len() {
        if !spec_sections::is_heading(lines[i], AC_SECTION_KEY) {
            i += 1;
            continue;
        }
        let end = spec_sections::section_end(&lines, i);
        for j in i + 1..end {
            let Some((line_id, after_sep)) = qa_run::parse_ac_header(lines[j]) else {
                continue;
            };
            if line_id != id {
                continue;
            }
            // The statement lives on the header line; rewrite it FIRST, while
            // the offset computed from the pristine line still holds.
            if let Some(statement) = plan.statement.as_deref() {
                let off = lines[j].len() - after_sep.len();
                out[j] = replace_statement(&out[j], off, statement);
                changed = true;
            }
            // Where this criterion's block STOPS — the same rule the parser's
            // lookahead applies. Computed BEFORE the command is located, because
            // a criterion carrying NO `Command:` line still has a statement
            // block to replace; deriving the boundary from the command line
            // would leave exactly that shape orphaned.
            let mut block_end = j + 1;
            while block_end < end && !ends_block(lines[block_end]) {
                block_end += 1;
            }
            // The command sits on the header line (one-line form) or on the
            // first `Command:` line of the block (drafter form).
            let inline = marker_end(lines[j], "command:").is_some();
            let cmd_line = if inline {
                Some(j)
            } else {
                (j + 1..block_end).find(|k| marker_end(lines[*k], "command:").is_some())
            };
            // The rest of the statement BLOCK: every pure-prose line between the
            // header and the command — or, when there is no command line at all,
            // to the end of the block. A new statement replaces all of it, so
            // what it does not replace it removes.
            if plan.statement.is_some() && !inline {
                for m in j + 1..cmd_line.unwrap_or(block_end) {
                    if is_statement_continuation(lines[m]) {
                        consumed[m] = true;
                        changed = true;
                    }
                }
            }
            let Some(k) = cmd_line else { continue };
            if let Some(rewritten) = replace_marker_value(&out[k], "command:", &plan.command) {
                out[k] = rewritten;
                changed = true;
            }
            // The two OPTIONAL markers, each applied only when its own flag was
            // given: an omitted flag leaves whatever the line already carries.
            if let Some(expect) = plan.expect.as_deref() {
                let at = marker_line(&out[k], &lines, k, block_end, inline, "expect:");
                out[at] = ensure_expect(&out[at], expect);
                changed = true;
            }
            if let Some(control) = plan.control.as_deref() {
                let at = marker_line(&out[k], &lines, k, block_end, inline, "control:");
                out[at] = ensure_control(&out[at], control);
                changed = true;
            }
        }
        i = end;
    }
    changed.then(|| {
        out.into_iter()
            .zip(consumed)
            .filter_map(|(line, dropped)| (!dropped).then_some(line))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

// ---------------------------------------------------------------------------
// Artefacts
// ---------------------------------------------------------------------------

/// The artefacts a criterion edit writes: the root `spec.md` — the list every
/// reader derives from — and `wave-plan.md`, the union QA executes. Nothing
/// else: a wave spec carries no criterion text (the prompt reads the parent at
/// dispatch time), and `qa/` / `review/` transcripts are records of a run into
/// which a rewritten criterion would be forged evidence.
///
/// `pub(super)` — the ADD door writes to exactly the same two files. Sorted, so
/// the report and the ledger are byte-stable.
pub(super) fn artefacts(spec_dir: &Path) -> Vec<PathBuf> {
    ["spec.md", "wave-plan.md"]
        .into_iter()
        .map(|name| spec_dir.join(name))
        .filter(|p| p.is_file())
        .collect()
}

/// The criteria a markdown document declares, through the shared parser.
///
/// `pub(super)` — the ADD door asks the same reader whether an id already
/// exists and whether its own write landed.
pub(super) fn criteria_of(markdown: &str) -> Vec<qa_run::AcItem> {
    qa_run::extract_ac_section(markdown)
        .map(|section| qa_run::parse_ac_items(&section))
        .unwrap_or_default()
}

/// `true` when `path` now declares `id` with exactly `command` — the RE-READ
/// that turns "the write landed" from an assumption into a report.
///
/// `pub(super)` — the ADD door owes the same re-read for exactly the same
/// reason: a write nobody read back is a write nobody can report.
pub(super) fn landed(path: &Path, id: &str, command: &str) -> bool {
    mfs::read_to_string(path).is_ok_and(|body| {
        criteria_of(&body)
            .iter()
            .any(|item| item.id == id && item.command == command)
    })
}

// ---------------------------------------------------------------------------
// The ledger
// ---------------------------------------------------------------------------

/// Read the ledger beside the spec markdown. An absent or unreadable file
/// yields an empty ledger, exactly as the negative test treats it.
///
/// `pub(super)` — shared with the ADD door so both criterion-editing doors
/// read and write ONE ledger through one pair of functions.
pub(super) fn read_ledger(path: &Path) -> AcProofLedger {
    mfs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str::<AcProofLedger>(&body).ok())
        .unwrap_or_default()
}

/// Serialize and write the ledger. `false` when nothing landed on disk.
///
/// `pub(super)` — see [`read_ledger`].
pub(super) fn write_ledger(path: &Path, ledger: &AcProofLedger) -> bool {
    let Ok(mut body) = serde_json::to_string_pretty(ledger) else {
        return false;
    };
    body.push('\n');
    mfs::write_atomic(path, body.as_bytes()).is_ok()
}

// ---------------------------------------------------------------------------
// The operation
// ---------------------------------------------------------------------------

/// Amend one criterion of `spec` under an explicit project `root`.
///
/// `root` is a PARAMETER, never re-derived from the process working directory:
/// this tool cuts a worktree per work unit, so the command runs off-root as a
/// matter of course — and the unit tests drive it against a temp tree.
pub(crate) fn amend(root: &Path, opts: &AcAmendOpts) -> AcAmendReport {
    let id = normalise_id(&opts.ac);

    // A reason nobody stated is an amendment nobody can audit later.
    let reason = opts.reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if reason.is_empty() {
        return AcAmendReport::refused(
            opts,
            &id,
            "blank_reason",
            "state WHY the criterion is being changed: `--reason \"<sentence>\"`. The reason is \
             the only part of an amendment a later reader cannot reconstruct from the diff",
        );
    }

    // An `Expect:` inside `--command` is ALWAYS a call error, and it must be
    // caught here — before any file is touched.
    //
    // What it did before: the marker was accepted verbatim into the command, and
    // the rewriter then appended the criterion's own `Expect:` after it, so the
    // line landed carrying TWO expected values. A criterion with two expected
    // values is ambiguous, and it was born that way with no refusal. Worse, the
    // damage was already on disk when the run reported `rewrite_failed` — the
    // confirmation read the line back, could not recognise it, and failed AFTER
    // the write. The operator then had to repair by hand the one file the flow
    // says never to edit by hand.
    if let Some(at) = expect_marker_in(&opts.command) {
        return AcAmendReport::refused(
            opts,
            &id,
            "expect_inside_command",
            &format!(
                "`--command` carries an `Expect:` marker at byte {at}. The expected value is its \
                 OWN flag: pass the command in `--command` and the evidence regex in `--expect`. \
                 Nothing was written"
            ),
        );
    }

    // A slug with no spec markdown is a typo, not a new spec.
    let Some(spec_file) = qa_run::spec_file_for(root, &opts.spec) else {
        return AcAmendReport::refused(
            opts,
            &id,
            "unknown_spec",
            "no spec markdown under `.claude/spec/<slug>/` for that name — check the slug with \
             `mustard-rt run active-specs`",
        );
    };
    let spec_dir = spec_file.parent().unwrap_or(root).to_path_buf();

    let Ok(markdown) = mfs::read_to_string(&spec_file) else {
        return AcAmendReport::refused(
            opts,
            &id,
            "unknown_spec",
            "the spec markdown could not be read — check the file exists and is readable",
        );
    };
    let items = criteria_of(&markdown);
    let Some(index) = items.iter().position(|item| item.id == id) else {
        let known: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        return AcAmendReport::refused(
            opts,
            &id,
            "unknown_criterion",
            &format!(
                "the spec declares no criterion with that id — amend one of [{}]",
                known.join(", ")
            ),
        );
    };
    let superseded = &items[index];

    // `--expect` omitted keeps the regex the criterion already carries: each
    // flag changes only what it names, so an amendment of the command alone
    // cannot silently drop the evidence half of the grading.
    let expect = opts
        .expect
        .clone()
        .filter(|e| !e.trim().is_empty())
        .or_else(|| superseded.expect.clone());

    let ledger_path = spec_dir.join(AC_PROOF_JSON);

    // THE gate. The trailing criterion is exempt here for the same reason it is
    // exempt from the negative test itself — it is the build-green safety net,
    // green before the work by design.
    let exempt = ac_negative_check::is_exempt(index, items.len());
    // The `Control:` the CALLER declared, if any — blank is the same as absent,
    // so a shell that expanded an empty variable cannot write an empty control
    // onto the line. Only this value is WRITTEN back to the line (see
    // [`Rewrite::control`]), exactly as only `opts.expect` is.
    let declared_control = opts
        .control
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    // `--control` omitted keeps the control the criterion already carries — the
    // same rule `--expect` follows one statement above, and for the same reason:
    // each flag changes only what it names. Without this fallback, amending the
    // COMMAND of a criterion whose line already declares `Control:` handed
    // `None` to the proof engine, and the control the line carries was never
    // taken for the replacement.
    //
    // A SKELETON is not a declared control: the drafter seeds the placeholder
    // on every non-exempt line, and inheriting it would refuse the amendment
    // (`take_control` never runs a `<…>` marker) for a control nobody wrote.
    let inherited_control = superseded
        .control
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty() && !qa_run::is_skeleton(c));
    let control = declared_control.or(inherited_control);
    // WHERE the command runs, which is not always where the spec lives. A
    // criterion corrected after the work landed cannot come back red in this
    // tree — the behaviour exists — so `--proof-tree` points at a checkout that
    // predates it. Everything else (reading the spec, rewriting the artefacts,
    // appending to the ledger) stays here.
    let proof_root: &Path = opts.proof_tree.as_deref().unwrap_or(root);
    if let Some(tree) = opts.proof_tree.as_deref()
        && !tree.is_dir() {
            // `error` is a CODE — a closed vocabulary a caller can match on,
            // and the field the crate's byte-stable-output guard covers. The
            // path is volatile (an absolute `/tmp/…` differs on every machine),
            // so it belongs in `remedy`, which is prose for a human.
            return AcAmendReport::refused(
                opts,
                &id,
                "proof_tree_not_a_directory",
                &format!(
                    "`--proof-tree {}` is not a directory — point it at a checkout of this \
                     repository that does not carry the work yet \
                     (`git worktree add --detach <dir> <base-commit>`)",
                    tree.display()
                ),
            );
        }
    // WHERE the red was taken, resolved ONCE and written to every reader: the
    // criterion's own record (which the approval gate reads), the amendment
    // history, and the report on stdout.
    let proof_tree_record = proof_tree_record(opts.proof_tree.as_deref(), root);
    let mut proof = ac_negative_check::prove_one(
        proof_root,
        &id,
        &opts.command,
        expect.as_deref(),
        control,
        exempt,
    );
    // An INHERITED control that does not come back green today is a finding
    // about the control, never a refusal of the amendment: the caller did not
    // declare it for this replacement, and the engine stops BEFORE the red
    // pass when a control fails — so the replacement would be refused over a
    // line the caller never touched. Say so once, and take the replacement's
    // own proof without it; the line keeps the control it carries, and the
    // next `ac-negative-check` pass re-asks it there.
    if declared_control.is_none()
        && proof.proof == Proof::NotAttempted
        && proof.control != ac_negative_check::Control::NotDeclared
    {
        eprintln!(
            "ac-amend: WARN: the `Control:` {id} carries (`{c}`) did not come back green \
             against this tree ({why}) — it was NOT taken for the replacement. Repair the \
             control, or name another with `--control`; the replacement is proven on its own \
             command below.",
            c = control.unwrap_or_default(),
            why = proof.reason.as_deref().unwrap_or("no verdict"),
        );
        proof = ac_negative_check::prove_one(
            proof_root,
            &id,
            &opts.command,
            expect.as_deref(),
            None,
            exempt,
        );
    }

    // The ONE recorded state the red rule cannot repair (see the module doc).
    // Looked up through the producer's own rule, against the command AND regex
    // the criterion carries TODAY, so a hand-edited line can never claim a
    // finding the engine made about some other command.
    let predecessor_inexecutable = ac_negative_check::recorded_proof(
        &read_ledger(&ledger_path),
        &id,
        &superseded.command,
        superseded.expect.as_deref(),
    )
    .is_some_and(|p| p.confirmation == Confirmation::Inexecutable);

    // The SECOND state the red rule cannot repair, and the same shape as the
    // first: the predecessor's own `Expect:` regex cannot match the output its
    // own command produces, so its red was never evidence about the work. A
    // count anchored at `^` against `grep -c` is the case — the command prints
    // `file:count`, so a bare-number regex misses whatever the work does.
    //
    // Read off the SUPERSEDED pair itself, through the same predicate the
    // drafting lint uses, so it is a fact about the criterion rather than
    // something the caller can ask for — exactly what keeps the first exception
    // from smuggling a vacuous criterion through. The drafting lint
    // (`expect-anchored-against-prefixed-output`) is the cheap door and catches
    // this before approval; this is the late door, for a criterion that shipped
    // before that lint existed.
    let predecessor_unsatisfiable = superseded
        .expect
        .as_deref()
        .is_some_and(|e| e.starts_with('^') && !e.contains(':'))
        && crate::commands::review::analyze_validation::counts_per_file(&superseded.command);

    if (predecessor_inexecutable || predecessor_unsatisfiable) && proof.proof == Proof::Green {
        // The replacement PASSES against a tree in which the work already
        // exists — which is precisely a green CONFIRMATION, and is recorded as
        // one. The red column keeps saying green, because green is what
        // happened; nothing here rewrites history to look like a red proof.
        proof.verdict = Verdict::Proven;
        proof.confirmation = Confirmation::Green;
        proof.confirmation_exit = proof.exit;
        proof.reason = None;
    }

    if proof.verdict == Verdict::Unproven {
        let reason = proof.reason.clone().unwrap_or_default();
        let mut remedy = format!(
            "the REPLACEMENT does not clear the negative test, so it proves exactly as little as \
             the criterion it would replace — {reason}"
        );
        // GREEN in the current tree is the one refusal with a way out the
        // reader cannot see from the reason alone — see
        // `green_in_this_tree_way_out`. The other colours keep the engine's
        // own remedy: it already names their one action.
        if proof.proof == Proof::Green {
            remedy.push_str("\n\n");
            remedy.push_str(&green_in_this_tree_way_out(
                "ac-amend",
                opts.proof_tree.as_deref(),
            ));
        }
        let mut report = AcAmendReport::refused(opts, &id, "replacement_not_proven", &remedy);
        report.proof = Some(proof);
        return report;
    }

    // Accepted. From here on the writes happen; every one of them is re-read.
    let plan = Rewrite {
        command: opts.command.clone(),
        expect: opts.expect.clone().filter(|e| !e.trim().is_empty()),
        statement: opts
            .statement
            .clone()
            .filter(|s| !s.trim().is_empty()),
        // ONLY what `--control` named (already trimmed, blank treated as
        // absent) — the same half `expect` above writes. The line and the ledger
        // still cannot disagree: when the flag is absent the proof was taken
        // with the control the LINE already carries, so leaving that line alone
        // records the value that is on it.
        control: declared_control.map(str::to_string),
    };
    let mut rewritten: Vec<String> = Vec::new();
    for path in artefacts(&spec_dir) {
        let Ok(body) = mfs::read_to_string(&path) else {
            continue;
        };
        let Some(updated) = rewrite_markdown(&body, &id, &plan) else {
            continue;
        };
        if mfs::write_atomic(&path, updated.as_bytes()).is_err() {
            continue;
        }
        if landed(&path, &id, &opts.command) {
            rewritten.push(ac_negative_check::repo_relative(root, &path));
        }
    }
    rewritten.sort();

    // WHERE the red came from travels on the CRITERION's record, not only in
    // the amendment history — the approval gate reads `criteria`, and a gate
    // that cannot tell an imported proof from one taken in place cannot audit
    // the very thing this flag exists to make auditable.
    //
    // Assigned HERE, before the report clones it. It used to be set after, so
    // stdout — the documented interface — omitted the field the ledger carried,
    // and an auditor reading the report could not make the distinction either.
    proof.proof_tree.clone_from(&proof_tree_record);

    let mut report = AcAmendReport {
        ok: false,
        spec: opts.spec.clone(),
        ac: id.clone(),
        command: opts.command.clone(),
        expect: expect.clone(),
        superseded_command: Some(superseded.command.clone()),
        superseded_expect: superseded.expect.clone(),
        proof: Some(proof.clone()),
        rewritten: rewritten.clone(),
        ledger: None,
        error: None,
        remedy: None,
    };

    // The ROOT spec is the one artefact that must have changed: the criterion
    // was found there. Nothing confirmed there is a lost write, reported.
    let root_path = ac_negative_check::repo_relative(root, &spec_file);
    if !rewritten.contains(&root_path) {
        report.error = Some("rewrite_failed".to_string());
        report.remedy = Some(format!(
            "the criterion line was not rewritten in `{root_path}` — re-read the file and check \
             the `Command:` marker of {id} is inside the `## Acceptance Criteria` section"
        ));
        return report;
    }

    let mut ledger = read_ledger(&ledger_path);
    ledger.spec = spec_dir
        .file_name()
        .map_or_else(|| opts.spec.clone(), |n| n.to_string_lossy().into_owned());
    // Replace the criterion's proof record so the approval door accepts the NEW
    // command; a criterion the ledger never carried simply joins it.
    ledger.criteria.retain(|c| c.id != id);
    ledger.criteria.push(proof);
    ledger.criteria.sort_by(|a, b| a.id.cmp(&b.id));

    let entry = Amendment {
        id: id.clone(),
        at: mustard_core::time::now_iso8601(),
        reason,
        superseded_command: superseded.command.clone(),
        superseded_expect: superseded.expect.clone(),
        superseded_statement: plan
            .statement
            .as_ref()
            .map(|_| superseded.statement.clone()),
        command: opts.command.clone(),
        expect,
        statement: plan.statement.clone(),
        rewrote: rewritten,
        proof_tree: proof_tree_record,
    };
    if let Ok(value) = serde_json::to_value(&entry) {
        ledger.amendments.push(value);
    }
    if !write_ledger(&ledger_path, &ledger) {
        report.error = Some("ledger_write_failed".to_string());
        report.remedy = Some(
            "the artefacts were rewritten but the proof ledger did not land — re-run \
             `mustard-rt run ac-negative-check --spec <slug>` to rebuild it"
                .to_string(),
        );
        return report;
    }
    report.ledger = Some(ac_negative_check::repo_relative(root, &ledger_path));
    report.ok = true;
    report
}

/// Byte offset of an `Expect:` marker inside `command`, or `None`.
///
/// Case-insensitive on the label, because the mistake is a human one and
/// `expect:` is the same mistake as `Expect:`. Deliberately NOT anchored to a
/// line start: the value the operator pasted is one line, and the marker sits
/// mid-line in exactly the shape that caused the corruption.
fn expect_marker_in(command: &str) -> Option<usize> {
    let lower = command.to_ascii_lowercase();
    lower.find("expect:")
}

/// The process exit code for a finished report: `0` accepted, `1` refused.
fn exit_code(report: &AcAmendReport) -> i32 {
    i32::from(!report.ok)
}

/// CLI entry — `mustard-rt run ac-amend`. The command has left the flow: it
/// refuses at the door with exit 1, writes nothing and says to revise the
/// criterion in the spec with `run write criterion`. [`amend`] keeps its body,
/// with its tests, until the command leaves.
pub fn run(_opts: AcAmendOpts) {
    crate::commands::retired::refuse(
        Path::new(&crate::shared::context::project_dir()),
        "use-write-criterion",
        "retired.use_write_criterion",
        &[("{command}", "ac-amend")],
    );
}

/// The door's old body, kept until the command leaves.
// A porta recusa, e nada mais chama este corpo: ele espera o comando sair.
#[allow(dead_code)]
fn run_amend(opts: AcAmendOpts) {
    let root = PathBuf::from(crate::shared::context::project_dir());
    let report = amend(&root, &opts);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(exit_code(&report));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    /// A command that comes back RED on both shells (`cmd.exe` and `sh`): the
    /// directory does not exist, so `cd` exits non-zero.
    const RED_COMMAND: &str = "cd no-such-directory-abc";
    /// A second red command, distinguishable from the first.
    const OTHER_RED_COMMAND: &str = "cd no-such-directory-xyz";
    /// A command that comes back GREEN on both shells — `cd .` is a builtin
    /// everywhere and always succeeds.
    const GREEN_COMMAND: &str = "cd .";

    /// A spec whose AC-2 is VACUOUS (green before its work exists), plus the
    /// trailing build-green safety criterion.
    fn spec_body() -> String {
        format!(
            "# S\n\n## Acceptance Criteria\n\
             - **AC-1** — when the work lands, then the new behaviour holds.\n  Command: `{RED_COMMAND}`\n\
             - **AC-2** — when the work lands, then the other thing holds.\n  Command: `{GREEN_COMMAND}`\n\
             - **AC-3** — build green.\n  Command: `{GREEN_COMMAND}`\n"
        )
    }

    /// Seed `<root>/.claude/spec/<spec>/` with a root `spec.md`, the frozen
    /// `wave-plan.md` carrying the SAME criterion lines (the union QA executes),
    /// and a wave that satisfies AC-2 — naming the id in its frontmatter, never
    /// copying the text.
    fn seed(root: &Path, spec: &str) -> PathBuf {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(dir.join("wave-2-rt")).unwrap();
        std::fs::write(dir.join("spec.md"), spec_body()).unwrap();
        std::fs::write(dir.join("wave-plan.md"), spec_body().replace("# S", "# Plan")).unwrap();
        std::fs::write(
            dir.join("wave-2-rt").join("spec.md"),
            format!("---\nid: wave.{spec}.2-rt\nsatisfies: [AC-2]\n---\n\n# Wave 2\n"),
        )
        .unwrap();
        dir
    }

    fn opts(spec: &str, ac: &str, command: &str, reason: &str) -> AcAmendOpts {
        AcAmendOpts {
            spec: spec.to_string(),
            ac: ac.to_string(),
            command: command.to_string(),
            expect: None,
            statement: None,
            reason: reason.to_string(),
            control: None,
            proof_tree: None,
        }
    }

    /// O critério `id` como o MESMO par de leitores que o `ac-negative-check`
    /// usa (`extract_ac_section` + `parse_ac_items`) o lê de volta do markdown.
    ///
    /// Releitura, nunca inspeção de bytes: o que importa não é que a linha tenha
    /// sido escrita, e sim que o portão seguinte consiga LER o que ela diz.
    fn read_back(markdown: &str, id: &str) -> qa_run::AcItem {
        criteria_of(markdown)
            .into_iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("{id} unreadable: {markdown:?}"))
    }

    /// Um executor de teste FILTRADO sem `--control` é julgado pelo COMANDO,
    /// como qualquer outro: a porta não o recusa por falta de controle.
    ///
    /// Substitui `a_filtered_runner_replacement_owes_a_control_and_the_flag_clears_it`,
    /// que trancava a tese "opcional deixa de ser opcional" com `!refused.ok` e
    /// `refused.error == Some("control_required")` para o caso (a) abaixo —
    /// asserções que agora falham por desenho. O que fica: a flag `--control`
    /// continua sendo tomada e registrada quando declarada, e um comando que
    /// não é executor continua não devendo nada.
    #[test]
    fn a_filtered_runner_replacement_without_a_control_is_judged_by_its_command() {
        // Um executor de teste FILTRADO: o `my_new_case` é seleção por nome.
        const FILTERED: &str = "cargo test -p mustard-rt my_new_case";

        // (a) Sem `--control`: o comando é LANÇADO e o veredito é o dele. Que
        // cor sai depende desta máquina (o cargo sem `Cargo.toml` sai vermelho;
        // um cargo ausente sai 127), e as duas leituras são honestas — o que
        // não pode acontecer é a recusa por exigência de controle.
        let a = tempdir().unwrap();
        seed(a.path(), "runner");
        let judged = amend(
            a.path(),
            &opts("runner", "AC-1", FILTERED, "o critério passa a nomear o teste novo"),
        );
        assert_ne!(
            judged.error.as_deref(),
            Some("control_required"),
            "não existe mais essa recusa: {judged:?}",
        );
        let proof = judged.proof.as_ref().expect("a prova é registrada seja qual for o veredito");
        assert!(proof.exit.is_some(), "o comando foi lançado — nada o recusou antes do shell: {judged:?}");
        assert_eq!(proof.control, ac_negative_check::Control::NotDeclared, "{judged:?}");
        assert!(
            !judged.remedy.as_deref().unwrap_or_default().contains("--control"),
            "a recusa, se houver, é sobre o comando: {judged:?}",
        );

        // (b) Com `--control`: o controle declarado entra no registro da prova
        // — a flag continua viva, como entrada OPCIONAL.
        let b = tempdir().unwrap();
        seed(b.path(), "runner");
        let mut with_control =
            opts("runner", "AC-1", FILTERED, "idem, agora com o controle declarado");
        with_control.control = Some(GREEN_COMMAND.to_string());
        let taken = amend(b.path(), &with_control);
        assert_eq!(
            taken.proof.as_ref().and_then(|p| p.control_command.as_deref()),
            Some(GREEN_COMMAND),
            "o controle declarado chega ao registro da prova: {taken:?}",
        );
        assert_eq!(
            taken.proof.as_ref().map(|p| p.control),
            Some(ac_negative_check::Control::Green),
            "e foi tomado no mesmo passo: {taken:?}",
        );

        // (c) A outra metade: um comando que NÃO é executor de teste continua
        // sem dever controle nenhum, e passa.
        let c = tempdir().unwrap();
        seed(c.path(), "runner");
        let plain = amend(
            c.path(),
            &opts("runner", "AC-1", OTHER_RED_COMMAND, "sem executor, sem controle"),
        );
        assert!(plain.ok, "{plain:?}");
    }

    /// `--control` OMITIDO cai no controle que o CRITÉRIO já declara — a mesma
    /// queda que o `--expect` faz uma instrução acima, e pela mesma razão: cada
    /// flag muda só o que ela nomeia.
    ///
    /// A regressão que isto tranca: `control` era `opts.control` e nada mais.
    /// Emendar o COMANDO de um critério cuja linha JÁ carrega `Control:`
    /// entregava `None` ao motor, e o controle escrito na linha nunca era
    /// tomado para a substituta — o registro dizia `not-declared` sobre um
    /// critério que declara.
    #[test]
    fn an_omitted_control_falls_back_to_the_one_the_criterion_declares() {
        // O executor FILTRADO: sai 0 quando o filtro não casa nada, que é toda a
        // razão da exigência.
        const FILTERED: &str = "cargo test -p mustard-rt my_new_case";
        // Um segundo comando verde, distinguível do `GREEN_COMMAND`.
        const OTHER_GREEN: &str = "cd ..";

        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("carried");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            format!(
                "# S\n\n## Acceptance Criteria\n\
                 - **AC-1** — when the work lands, then the behaviour holds.\n  \
                 Command: `{RED_COMMAND}`\n  Control: `{GREEN_COMMAND}`\n\
                 - **AC-2** — when the work lands, then the other thing holds.\n  \
                 Command: `{RED_COMMAND}`\n\
                 - **AC-3** — build green.\n  Command: `{GREEN_COMMAND}`\n"
            ),
        )
        .unwrap();

        // (a) A linha do AC-1 já carrega o controle; a emenda nomeia só o
        // comando, e o controle DA LINHA é o que vai ao motor.
        let taken = amend(
            dir.path(),
            &opts("carried", "AC-1", FILTERED, "o critério passa a nomear o teste novo"),
        );
        assert!(taken.ok, "a emenda é aceita: {taken:?}");
        assert_eq!(
            taken.proof.as_ref().and_then(|p| p.control_command.as_deref()),
            Some(GREEN_COMMAND),
            "é o controle DA LINHA que chega ao motor da prova: {taken:?}",
        );
        let md = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        let item = read_back(&md, "AC-1");
        assert_eq!(item.command, FILTERED, "{md:?}");
        assert_eq!(
            item.control.as_deref(),
            Some(GREEN_COMMAND),
            "e o controle continua na linha, intacto: {md:?}",
        );
        assert_eq!(md.matches("Control:").count(), 1, "marcador duplicado: {md:?}");

        // (b) A metade que não pode afrouxar junto: o AC-2 nunca teve controle,
        // e a emenda dele não INVENTA um — o registro diz `not-declared`, e a
        // porta não recusa por isso.
        let owed = amend(
            dir.path(),
            &opts("carried", "AC-2", FILTERED, "idem, num critério sem controle"),
        );
        assert_ne!(owed.error.as_deref(), Some("control_required"), "{owed:?}");
        let owed_proof = owed.proof.as_ref().expect("a prova é registrada");
        assert_eq!(
            owed_proof.control_command,
            None,
            "sem controle na linha e sem flag, nada é inventado: {owed:?}",
        );
        assert_eq!(owed_proof.control, ac_negative_check::Control::NotDeclared, "{owed:?}");

        // (c) E o `--control` explícito ainda vence o que a linha carrega.
        let mut explicit = opts(
            "carried",
            "AC-1",
            "cargo test -p mustard-rt my_other_case",
            "o controle é trocado por outro",
        );
        explicit.control = Some(OTHER_GREEN.to_string());
        let swapped = amend(dir.path(), &explicit);
        assert_eq!(
            swapped.proof.as_ref().and_then(|p| p.control_command.as_deref()),
            Some(OTHER_GREEN),
            "a flag explícita vence a linha: {swapped:?}",
        );
        let md = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        assert_eq!(read_back(&md, "AC-1").control.as_deref(), Some(OTHER_GREEN), "{md:?}");
    }

    /// O ROUND TRIP que faltava ao `--control`: o controle declarado tem de
    /// chegar à LINHA do critério, porque é o MARKDOWN que a passada seguinte do
    /// `ac-negative-check` lê.
    ///
    /// A regressão que isto tranca: a flag chegava ao motor e ao
    /// `control_command` do ledger, e a reescrita não escrevia marcador nenhum
    /// na linha. O critério admitido por esta porta reaparecia SEM controle na
    /// passada seguinte, que registrava `not-declared` e avisava — sobre um
    /// controle que o operador tinha declarado. Uma flag que parece funcionar e
    /// não funciona é pior que uma que não existe.
    ///
    /// Dois lados, e o veredito de cada um vem do predicado do lint de rascunho
    /// (`test_runner_has_selector`, o que nomeia `test-ac-no-control`)
    /// alimentado pela RELEITURA, não de uma segunda leitura de "isto é um
    /// executor filtrado?".
    #[test]
    fn an_amended_control_lands_on_the_line_the_next_gate_reads() {
        use crate::commands::review::analyze_validation::test_runner_has_selector;
        // Um executor de teste FILTRADO — a forma que o lint de rascunho nomeia
        // quando não acha `Control:` na linha.
        const FILTERED: &str = "cargo test -p mustard-rt my_new_case";
        let md = "## Acceptance Criteria\n- **AC-2** — old statement.\n  Command: `cd old`\n";
        let plan = |control: Option<&str>| Rewrite {
            command: FILTERED.to_string(),
            expect: None,
            statement: None,
            control: control.map(str::to_string),
        };

        // COM `--control`: a releitura acha o controle, e o lint de rascunho
        // nada tem a nomear.
        let with =
            rewrite_markdown(md, "AC-2", &plan(Some(GREEN_COMMAND))).expect("the criterion changed");
        let item = read_back(&with, "AC-2");
        assert_eq!(item.command, FILTERED, "{with:?}");
        assert_eq!(
            item.control.as_deref(),
            Some(GREEN_COMMAND),
            "o `Control:` tem de estar NA LINHA, não só no registro da prova: {with:?}",
        );
        assert!(
            test_runner_has_selector(&item.command) && item.control.is_some(),
            "a passada seguinte lê o controle da linha: {with:?}",
        );

        // SEM ela: o critério fica sem controle e o lint volta a nomeá-lo.
        let without = rewrite_markdown(md, "AC-2", &plan(None)).expect("the command changed");
        let item = read_back(&without, "AC-2");
        assert_eq!(item.control, None, "{without:?}");
        assert!(
            test_runner_has_selector(&item.command) && item.control.is_none(),
            "sem controle nenhum o lint TEM de nomear — senão este teste não mede nada: \
             {without:?}",
        );
    }

    /// `--control` omitido MANTÉM o controle que a linha já carrega — a mesma
    /// semântica que o `--expect` documenta, e a metade que não pode quebrar
    /// junto: toda emenda de comando passaria a apagar em silêncio o controle de
    /// quem já declarou um.
    ///
    /// Em todas as formas que o leitor aceita, porque é em uma delas que a
    /// perda passaria despercebida: marcador em linha própria, marcador na linha
    /// do comando, forma histórica de uma linha só, e documento CRLF.
    #[test]
    fn an_omitted_control_flag_keeps_the_control_the_line_carries() {
        let plan = |control: Option<&str>| Rewrite {
            command: "cd new".to_string(),
            expect: None,
            statement: None,
            control: control.map(str::to_string),
        };
        for original in [
            "## Acceptance Criteria\n- **AC-2** — s.\n  Command: `cd old`\n  Control: `cd .`\n",
            "## Acceptance Criteria\n- **AC-2** — s.\n  Command: `cd old` Control: `cd .`\n",
            "## Acceptance Criteria\n- [ ] AC-2: s — Command: `cd old` Control: `cd .`\n",
            "## Acceptance Criteria\r\n- **AC-2** — s.\r\n  Command: `cd old`\r\n  Control: `cd .`\r\n",
        ] {
            let kept = rewrite_markdown(original, "AC-2", &plan(None)).expect("the command changed");
            let item = read_back(&kept, "AC-2");
            assert_eq!(item.command, "cd new", "{kept:?}");
            assert_eq!(
                item.control.as_deref(),
                Some("cd ."),
                "uma emenda que não nomeia controle não pode apagar o que existe: {kept:?}",
            );

            // E o controle NOVO substitui o antigo, sem duplicar o marcador —
            // duas respostas na mesma linha é um critério ambíguo.
            let swapped =
                rewrite_markdown(original, "AC-2", &plan(Some("cd .."))).expect("the criterion changed");
            let item = read_back(&swapped, "AC-2");
            assert_eq!(item.control.as_deref(), Some("cd .."), "{swapped:?}");
            assert_eq!(swapped.matches("Control:").count(), 1, "marcador duplicado: {swapped:?}");
            if original.contains("\r\n") {
                assert!(
                    !swapped.split('\n').any(|l| l.contains("Control:") && !l.ends_with('\r')),
                    "um documento CRLF não pode ganhar linha com LF sozinho: {swapped:?}",
                );
            }
        }

        // A outra metade: um critério que NUNCA teve controle continua sem — a
        // reescrita não inventa marcador que ninguém pediu.
        let bare = rewrite_markdown(
            "## Acceptance Criteria\n- **AC-2** — s.\n  Command: `cd old`\n",
            "AC-2",
            &plan(None),
        )
        .expect("the command changed");
        assert!(!bare.contains("Control:"), "{bare:?}");
    }

    /// Fim a fim, no disco: o controle declarado na chamada chega à linha do
    /// critério em TODO artefato — o `spec.md` do pai, de onde o prompt da onda
    /// é lido, E a união do `wave-plan.md`, que o QA executa.
    #[test]
    fn the_declared_control_reaches_every_artefact_on_disk() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "controlled");
        let mut o = opts("controlled", "AC-2", OTHER_RED_COMMAND, "o critério ganha um controle");
        o.control = Some(GREEN_COMMAND.to_string());
        let report = amend(dir.path(), &o);
        assert!(report.ok, "unexpected refusal: {:?} / {:?}", report.error, report.remedy);
        // O ledger já carregava isto antes da correção; o que faltava era a
        // LINHA, e é ela que o portão seguinte lê.
        assert_eq!(
            report.proof.as_ref().and_then(|p| p.control_command.as_deref()),
            Some(GREEN_COMMAND),
            "{report:?}",
        );
        for path in [spec_dir.join("spec.md"), spec_dir.join("wave-plan.md")] {
            let body = std::fs::read_to_string(&path).unwrap();
            let item = read_back(&body, "AC-2");
            assert_eq!(item.command, OTHER_RED_COMMAND, "{}", path.display());
            assert_eq!(
                item.control.as_deref(),
                Some(GREEN_COMMAND),
                "o `Control:` tem de estar na linha de {}",
                path.display(),
            );
        }
    }

    /// Um controle HERDADO da linha que não vem verde hoje é um AVISO sobre o
    /// controle, nunca uma recusa da emenda — e o placeholder do rascunho nem
    /// chega a ser herdado: um `<…>` não é um controle declarado.
    ///
    /// A regressão que isto tranca: o fallback herdava o esqueleto do drafter e
    /// o motor recusava a substituta ANTES de rodá-la ("the CONTROL was NEVER
    /// TAKEN"), por um controle que ninguém escreveu; e um controle escrito de
    /// verdade que ficou vermelho recusava do mesmo jeito, por uma linha que a
    /// chamada nunca tocou.
    #[test]
    fn an_inherited_control_that_is_not_green_warns_instead_of_refusing() {
        const SKELETON: &str = "<a command that must be GREEN against the tree as it is today>";
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("inherited");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            format!(
                "# S\n\n## Acceptance Criteria\n\
                 - **AC-1** — when the work lands, then the behaviour holds.\n  \
                 Command: `{GREEN_COMMAND}`\n  Control: `{SKELETON}`\n\
                 - **AC-2** — when the work lands, then the other thing holds.\n  \
                 Command: `{GREEN_COMMAND}`\n  Control: `{RED_COMMAND}`\n\
                 - **AC-3** — build green.\n  Command: `{GREEN_COMMAND}`\n"
            ),
        )
        .unwrap();

        // (a) O esqueleto não é herdado: a prova sai `not-declared`, e a linha
        // fica como estava — a emenda não inventa controle.
        let skel = amend(dir.path(), &opts("inherited", "AC-1", OTHER_RED_COMMAND, "r"));
        assert!(skel.ok, "um placeholder não pode recusar a emenda: {skel:?}");
        let proof = skel.proof.as_ref().expect("a prova é registrada");
        assert_eq!(proof.control, ac_negative_check::Control::NotDeclared, "{skel:?}");
        assert_eq!(proof.control_command, None, "{skel:?}");
        assert_eq!(proof.proof, Proof::Red, "{skel:?}");

        // (b) Um controle real que está VERMELHO hoje avisa e a substituta é
        // provada pelo comando dela; a linha continua carregando o controle.
        let red = amend(dir.path(), &opts("inherited", "AC-2", OTHER_RED_COMMAND, "r"));
        assert!(red.ok, "um controle vermelho é achado sobre o controle, não recusa: {red:?}");
        let proof = red.proof.as_ref().expect("a prova é registrada");
        assert_eq!(proof.proof, Proof::Red, "a substituta foi provada mesmo: {red:?}");
        assert_eq!(proof.verdict, Verdict::Proven, "{red:?}");
        let md = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        assert_eq!(read_back(&md, "AC-2").control.as_deref(), Some(RED_COMMAND), "{md:?}");

        // (c) A metade que não pode afrouxar junto: um controle DECLARADO na
        // chamada que vem vermelho continua recusando — o chamador o nomeou
        // para esta substituta, e o motor tem razão em não ler o vermelho dela.
        let mut declared = opts("inherited", "AC-1", OTHER_RED_COMMAND, "r");
        declared.control = Some(RED_COMMAND.to_string());
        let refused = amend(dir.path(), &declared);
        assert!(!refused.ok, "{refused:?}");
        assert_eq!(refused.error.as_deref(), Some("replacement_not_proven"), "{refused:?}");
    }

    /// A criterion corrected AFTER the work landed takes its red somewhere the
    /// work is absent — and the ledger records where.
    ///
    /// Without this, the only way through was a contrivance: hide the test, run
    /// the amend, restore the test. It worked, and it left no trace of where
    /// the red came from, so the ledger's own claim could not be checked.
    ///
    /// Measured by giving the two trees opposite answers to one command: a
    /// marker file that exists here and not there. The proof runs THERE; the
    /// spec is still read and rewritten HERE.
    #[test]
    fn a_proof_tree_takes_the_red_where_the_work_is_absent() {
        let here = tempfile::tempdir().unwrap();
        seed(here.path(), "demo");
        let marker_cmd = "test -f built.marker";

        // This tree carries the work, so the replacement passes and an
        // ordinary amend has no honest red to take.
        std::fs::write(here.path().join("built.marker"), "done").unwrap();
        let green = amend(here.path(), &opts("demo", "AC-1", marker_cmd, "the work exists here"));
        assert!(!green.ok, "a replacement that passes must be refused");

        // A tree WITHOUT the marker answers the same command red — and it is a
        // REAL repository with a commit, because recording the commit is the
        // branch under test. A bare temp directory is satisfied by the
        // path fallback, which would let the commit branch stay unexercised.
        let before = tempfile::tempdir().unwrap();
        let head = git_repo_with_one_commit(before.path())
            .expect("the test needs a usable `git` to prove the commit branch");
        let mut o = opts("demo", "AC-1", marker_cmd, "corrected after the work landed");
        o.proof_tree = Some(before.path().to_path_buf());
        let red = amend(here.path(), &o);
        assert!(red.ok, "the red taken in the other tree must be accepted: {red:?}");

        // …and BOTH readers say WHERE. Without it the claim is unverifiable,
        // which is the state this flag replaces.
        let ledger: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                here.path().join(".claude/spec/demo").join(AC_PROOF_JSON),
            )
            .unwrap(),
        )
        .unwrap();
        // The COMMIT, not the path: the directory is gone by the time anyone
        // reads this, and only the commit lets a reader repeat the measurement.
        assert_eq!(
            ledger["amendments"][0]["proof_tree"].as_str(),
            Some(head.as_str()),
            "the amendment must record the commit the red stood on",
        );
        // The approval gate reads `criteria`, never `amendments` — recording it
        // only in the history leaves that gate unable to tell an imported proof
        // from one taken in place.
        assert_eq!(
            ledger["criteria"][0]["proof_tree"].as_str(),
            Some(head.as_str()),
            "the criterion's own record must carry it too",
        );
    }

    /// The rule is about WHICH REPOSITORY git answered for, not about whether
    /// the path is a repository root.
    ///
    /// `git rev-parse` walks up through parents, so a plain directory under the
    /// tree being AMENDED answers with that tree's HEAD — the commit which
    /// carries the work, the opposite of what the flag claims. But a
    /// SUBDIRECTORY of a different worktree is a tree git names the right commit
    /// for, and an earlier exact-equality guard refused it. Both directions are
    /// asserted here.
    #[test]
    fn the_proof_tree_answers_for_any_repository_but_the_one_being_amended() {
        let amending = tempfile::tempdir().unwrap();
        let Some(_here) = git_repo_with_one_commit(amending.path()) else {
            return; // no usable git here; the amend path is covered elsewhere
        };
        // Nested in the tree being amended: git walks up and offers the commit
        // that carries the work. Refused.
        let nested = amending.path().join("not-a-repo");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            proof_tree_commit(&nested, amending.path()),
            None,
            "a directory inside the amended tree must not borrow its commit",
        );

        // A DIFFERENT repository answers — at its root…
        let other = tempfile::tempdir().unwrap();
        let Some(head) = git_repo_with_one_commit(other.path()) else {
            return;
        };
        assert_eq!(
            proof_tree_commit(other.path(), amending.path()).as_deref(),
            Some(head.as_str()),
            "another repository's root answers",
        );
        // …and at a subdirectory of it, which the exact-equality guard refused.
        let sub = other.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            proof_tree_commit(&sub, amending.path()).as_deref(),
            Some(head.as_str()),
            "a subdirectory of the right worktree names the right commit",
        );
    }

    /// The comparison is REPOSITORY against REPOSITORY, so it still holds when
    /// the project anchor is a subdirectory of the git root.
    ///
    /// A monorepo is the ordinary case — this repository is one. An earlier fix
    /// compared git's answer for the proof tree against the `amending` PATH,
    /// which is the `mustard.json` + `.claude/` anchor. In `mono/apps/thing/`
    /// the two differ, the equality could never hold, and a plain nested
    /// directory borrowed the amended tree's own HEAD: the ledger then claimed
    /// the negative proof was taken at the very commit carrying the work
    /// (measured in review).
    #[test]
    fn the_guard_holds_when_the_project_anchor_is_not_the_git_root() {
        let mono = tempfile::tempdir().unwrap();
        let Some(_head) = git_repo_with_one_commit(mono.path()) else {
            return; // no usable git here
        };
        // The project anchor sits DEEP inside the repository, as it does in a
        // monorepo — this is the path `amend` receives as `root`.
        let anchor = mono.path().join("apps").join("thing");
        std::fs::create_dir_all(&anchor).unwrap();
        // A plain directory, not a repository, anywhere inside that same repo.
        let nested = mono.path().join("scratch").join("plain");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            proof_tree_commit(&nested, &anchor),
            None,
            "a directory inside the amended REPOSITORY must not borrow its commit, \
             whatever subdirectory the project anchor sits in",
        );

        // …and a genuinely different repository still answers, from that same
        // anchor — the guard refuses the right thing, not everything.
        let other = tempfile::tempdir().unwrap();
        let Some(head) = git_repo_with_one_commit(other.path()) else {
            return;
        };
        assert_eq!(
            proof_tree_commit(other.path(), &anchor).as_deref(),
            Some(head.as_str()),
            "another repository still answers",
        );
    }

    /// A tree git cannot speak for records NOTHING — never an absolute path.
    ///
    /// `ac-proof.json` is versioned and this crate requires byte-stable output,
    /// so a `/tmp/…` path would churn per machine. It is also worthless as
    /// evidence on its own terms: the commit is recorded precisely BECAUSE the
    /// directory is gone by the time anyone reads the ledger.
    #[test]
    fn a_tree_git_cannot_speak_for_records_nothing_rather_than_a_path() {
        let amending = tempfile::tempdir().unwrap();
        let plain = tempfile::tempdir().unwrap();
        // Not a repository, and not inside one that git would walk up into
        // (temp dirs are outside this checkout).
        let recorded = proof_tree_record(Some(plain.path()), amending.path());
        assert!(
            recorded.as_deref().is_none_or(|r| !r.contains('/')),
            "a path must never reach the ledger: {recorded:?}",
        );
    }

    /// Make `dir` a git repository carrying exactly one commit, and return its
    /// short hash. `None` when git is absent or refuses — the caller then skips,
    /// rather than failing a test on the environment.
    fn git_repo_with_one_commit(dir: &Path) -> Option<String> {
        let git = |args: &[&str]| -> bool {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .output()
                .is_ok_and(|o| o.status.success())
        };
        if !git(&["init", "--quiet"]) {
            return None;
        }
        std::fs::write(dir.join("seed.txt"), "seed").ok()?;
        if !git(&["add", "-A"]) || !git(&["commit", "--quiet", "-m", "seed"]) {
            return None;
        }
        proof_tree_commit(dir, Path::new("/nonexistent-amending-tree"))
    }

    /// A `--proof-tree` that is not a directory refuses and says how to make
    /// one. A typo must never silently fall back to proving here — that would
    /// hand back the green this flag exists to avoid.
    #[test]
    fn a_proof_tree_that_is_not_a_directory_refuses() {
        let here = tempfile::tempdir().unwrap();
        seed(here.path(), "demo");
        let mut o = opts("demo", "AC-1", "cd .", "typo in the path");
        o.proof_tree = Some(here.path().join("does-not-exist"));
        let r = amend(here.path(), &o);
        assert!(!r.ok, "a bad --proof-tree must refuse");
        assert!(
            r.remedy.as_deref().is_some_and(|m| m.contains("git worktree add")),
            "the refusal must name how to make one: {:?}",
            r.remedy,
        );
        // `error` is a CODE — matchable, and byte-stable across machines. The
        // volatile absolute path belongs to `remedy`, which is prose.
        assert_eq!(r.error.as_deref(), Some("proof_tree_not_a_directory"));
        assert!(
            !r.error.as_deref().unwrap_or_default().contains('/'),
            "the code must carry no path: {:?}",
            r.error,
        );
    }

    /// The load-bearing refusal: a replacement that ALREADY passes proves
    /// exactly as little as the criterion it would replace, so it is refused and
    /// NOTHING is written — not the artefacts, not the ledger.
    #[test]
    fn amend_refuses_a_vacuous_new_command() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "vacuous");
        let before_root = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        let before_wave =
            std::fs::read_to_string(spec_dir.join("wave-2-rt").join("spec.md")).unwrap();

        let report = amend(
            dir.path(),
            &opts("vacuous", "AC-2", GREEN_COMMAND, "swap one green for another"),
        );

        assert!(!report.ok, "a replacement that passes now must be refused");
        assert_eq!(report.error.as_deref(), Some("replacement_not_proven"));
        let remedy = report.remedy.clone().unwrap_or_default();
        assert!(
            remedy.contains("proves exactly as little"),
            "the refusal names WHY: {remedy}"
        );
        assert!(
            remedy.contains("rewrite the command"),
            "and carries the engine's own remedy: {remedy}"
        );
        // And the WAY OUT — the field read this refusal as a dead end because
        // `--proof-tree` existed and nothing that refused them said so. The
        // recipe is verbatim, three lines, and names THIS door.
        assert!(
            remedy.contains("cannot come back red here"),
            "the refusal says why green is expected after the work: {remedy}"
        );
        assert!(
            remedy.contains("git worktree add --detach <dir> <commit-before-the-work>")
                && remedy.contains("mustard-rt run ac-amend … --proof-tree <dir>")
                && remedy.contains("git worktree remove <dir>"),
            "and gives the recipe verbatim: {remedy}"
        );
        assert_eq!(exit_code(&report), 1, "a refusal exits non-zero");

        // Nothing written, anywhere.
        assert!(report.rewritten.is_empty(), "{:?}", report.rewritten);
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("spec.md")).unwrap(),
            before_root,
            "the root spec must be byte-identical after a refusal"
        );
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("wave-2-rt").join("spec.md")).unwrap(),
            before_wave,
            "and so must the planted wave artefact"
        );
        assert!(
            !spec_dir.join(AC_PROOF_JSON).exists(),
            "a refusal must not even create the ledger"
        );
    }

    /// The accepted direction: a replacement that comes back RED is written into
    /// EVERY artefact carrying the id — the root AND the frozen `wave-plan.md` —
    /// and the ledger records the superseded version with the stated reason.
    /// The wave spec is NOT one of them: it carries no copy, and its prompt
    /// reads the new command off the root.
    #[test]
    fn amend_records_the_previous_version_and_rewrites_every_artifact() {
        use crate::commands::agent::render::sections::read_wave_acceptance;
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "amended");

        let mut o = opts(
            "amended",
            "AC-2",
            OTHER_RED_COMMAND,
            "the original could not tell done from not-done",
        );
        o.expect = Some("1 passed".to_string());
        let report = amend(dir.path(), &o);

        assert!(report.ok, "unexpected refusal: {:?}", report.error);
        assert_eq!(exit_code(&report), 0);
        assert_eq!(report.superseded_command.as_deref(), Some(GREEN_COMMAND));
        assert_eq!(report.expect.as_deref(), Some("1 passed"));

        // Both artefacts rewritten, reported as repo paths with forward slashes.
        assert_eq!(
            report.rewritten,
            vec![
                ".claude/spec/amended/spec.md".to_string(),
                ".claude/spec/amended/wave-plan.md".to_string(),
            ],
            "the root AND the frozen union"
        );

        // QA executes the union, so the NEW command must be there too — this
        // is the whole reason a root-only amendment is not enough.
        for path in [spec_dir.join("spec.md"), spec_dir.join("wave-plan.md")] {
            let body = std::fs::read_to_string(&path).unwrap();
            let item = criteria_of(&body)
                .into_iter()
                .find(|i| i.id == "AC-2")
                .unwrap_or_else(|| panic!("AC-2 missing from {}", path.display()));
            assert_eq!(item.command, OTHER_RED_COMMAND, "{}", path.display());
            assert_eq!(item.expect.as_deref(), Some("1 passed"), "{}", path.display());
        }
        // The wave spec is untouched — it carries no copy to bring forward —
        // and the dispatched agent reads the NEW command anyway, because its
        // prompt is cut from the root at render time.
        let wave_path = spec_dir.join("wave-2-rt").join("spec.md");
        assert!(
            !std::fs::read_to_string(&wave_path).unwrap().contains("Command:"),
            "the wave file must not have grown a copy"
        );
        let ruler = read_wave_acceptance(&spec_dir.join("spec.md"), Some(&wave_path));
        assert!(ruler.contains(OTHER_RED_COMMAND), "the prompt reads the amendment: {ruler}");
        assert!(!ruler.contains(GREEN_COMMAND), "and not the superseded command: {ruler}");
        // No staleness channel is emitted: nothing can go stale.
        let json = serde_json::to_value(&report).expect("report serialises");
        assert!(json.get("wavesStale").is_none() && json.get("staleWaves").is_none(), "{json}");

        // The sibling criteria are untouched — the rewrite is surgical, which is
        // also why the root spec legitimately still contains the green command
        // (AC-3, the trailing safety criterion, is green by design).
        let root_items = criteria_of(&std::fs::read_to_string(spec_dir.join("spec.md")).unwrap());
        assert_eq!(root_items[0].command, RED_COMMAND, "AC-1 untouched");
        assert_eq!(root_items[2].command, GREEN_COMMAND, "AC-3 untouched");

        // The ledger: the criterion's proof record now carries the NEW command,
        // so the approval door accepts it, and the history records what was
        // superseded, why, and when.
        let ledger: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join(AC_PROOF_JSON)).unwrap(),
        )
        .unwrap();
        let record = ledger["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "AC-2")
            .unwrap();
        assert_eq!(record["command"], OTHER_RED_COMMAND);
        assert_eq!(record["verdict"], "proven");
        let amendment = &ledger["amendments"].as_array().unwrap()[0];
        assert_eq!(amendment["id"], "AC-2");
        assert_eq!(amendment["superseded_command"], GREEN_COMMAND);
        assert_eq!(
            amendment["reason"],
            "the original could not tell done from not-done"
        );
        assert!(
            amendment["at"].as_str().is_some_and(|s| s.ends_with('Z')),
            "the timestamp lives in the ledger: {amendment}"
        );
        // ...and NOT on stdout, which is snapshot-compared.
        let printed = serde_json::to_string(&report).unwrap();
        assert!(!printed.contains("\"at\""), "no timestamp on stdout: {printed}");
    }

    /// The one case the red rule cannot express. A criterion the engine
    /// itself recorded as INEXECUTABLE is repaired by a substitute that PASSES,
    /// because by the time inexecutability is discovered the work is done and
    /// the corrected command legitimately passes.
    ///
    /// The inexecutability is produced, not asserted: `AC-1` declares an
    /// `Expect:` regex that is not valid, so while the command exits non-zero
    /// the executor never looks at the regex (a genuine RED proof), and the
    /// moment the work lands and the command exits 0 the invalid regex makes it
    /// unattemptable. That is exactly the field shape — a criterion whose flaw
    /// only surfaces after its work exists.
    ///
    /// Two-sided: the SAME passing substitute against a predecessor with no
    /// such recorded finding is still refused, so the exception cannot be read
    /// as "green is now acceptable".
    #[test]
    fn ac_amend_accepts_inexecutable_predecessor() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // AC-1's evidence regex is not a valid regex; AC-2 is the trailing
        // build-green safety criterion.
        let body = format!(
            "# S\n\n## Acceptance Criteria\n\
             - **AC-1** — when the work lands, then the directory is there.\n  \
             Command: `{RED_COMMAND}`\n  Expect: `[unterminated`\n\
             - **AC-2** — build green.\n  Command: `{GREEN_COMMAND}`\n"
        );
        let spec_dir = root.join(".claude").join("spec").join("inexecutable");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), &body).unwrap();

        // 1. The RED proof: the command fails now, so the regex is never read.
        let proof = ac_negative_check::check(root, "inexecutable");
        let recorded = proof.criteria.iter().find(|c| c.id == "AC-1").unwrap();
        assert_eq!(recorded.verdict, Verdict::Proven, "{recorded:?}");
        assert_eq!(recorded.proof, Proof::Red);

        // 2. THE WORK LANDS — the directory the criterion asserts now exists.
        std::fs::create_dir(root.join("no-such-directory-abc")).unwrap();

        // 3. The CONFIRMATION discovers the criterion is inexecutable: the
        //    command exits 0, and now the invalid regex makes it unattemptable.
        let confirmed = ac_negative_check::confirm(root, "inexecutable");
        let found = confirmed.criteria.iter().find(|c| c.id == "AC-1").unwrap();
        assert_eq!(
            found.confirmation,
            Confirmation::Inexecutable,
            "the confirmation must discover the criterion is broken: {found:?}"
        );
        assert_eq!(found.verdict, Verdict::Unproven);

        // 4. The repair: a substitute that PASSES is accepted for this ONE
        //    recorded state, and its record carries a GREEN confirmation.
        let mut o = opts(
            "inexecutable",
            "AC-1",
            "echo confirmed",
            "the declared Expect regex is not a valid regex, so the criterion can never be run",
        );
        o.expect = Some("confirmed".to_string());
        let report = amend(root, &o);
        assert!(report.ok, "unexpected refusal: {:?} / {:?}", report.error, report.remedy);
        let accepted = report.proof.clone().expect("the amendment records its proof");
        assert_eq!(accepted.proof, Proof::Green, "the substitute genuinely passes");
        assert_eq!(
            accepted.confirmation,
            Confirmation::Green,
            "and that pass is recorded as the CONFIRMATION, which is what it is"
        );
        assert!(accepted.evidenced(), "so the approval gate can act on it");
        assert_eq!(
            report.rewritten,
            vec![".claude/spec/inexecutable/spec.md".to_string()],
            "the criterion line is rewritten"
        );
        // The audit trail names why, as every amendment must.
        let ledger: Value =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join(AC_PROOF_JSON)).unwrap())
                .unwrap();
        assert_eq!(ledger["amendments"][0]["superseded_command"], RED_COMMAND);
        assert!(ledger["amendments"][0]["reason"]
            .as_str()
            .is_some_and(|r| r.contains("not a valid regex")));

        // 5. Two-sided — the same passing substitute against a predecessor the
        //    engine recorded NOTHING about is still refused. Nothing about the
        //    exception generalises to "a green replacement is acceptable".
        let plain = seed(root, "plain");
        let refused = amend(
            root,
            &opts("plain", "AC-2", GREEN_COMMAND, "swap one green for another"),
        );
        assert!(!refused.ok, "a green substitute with no such finding must be refused");
        assert_eq!(refused.error.as_deref(), Some("replacement_not_proven"));
        assert!(
            !plain.join(AC_PROOF_JSON).exists(),
            "and the refusal still writes nothing"
        );
    }

    /// The three refusals that guard the inputs. Each writes nothing and names
    /// the one action that clears it.
    #[test]
    fn blank_reason_unknown_spec_and_unknown_criterion_are_refused() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "guarded");

        for (o, code) in [
            (opts("guarded", "AC-2", OTHER_RED_COMMAND, "   "), "blank_reason"),
            (opts("no-such-spec", "AC-2", OTHER_RED_COMMAND, "why"), "unknown_spec"),
            (opts("guarded", "AC-9", OTHER_RED_COMMAND, "why"), "unknown_criterion"),
        ] {
            let report = amend(dir.path(), &o);
            assert!(!report.ok, "{code} must refuse");
            assert_eq!(report.error.as_deref(), Some(code));
            assert!(
                report.remedy.is_some_and(|r| !r.trim().is_empty()),
                "{code} must name what to do about it"
            );
            assert!(report.rewritten.is_empty(), "{code} wrote an artefact");
            assert!(!spec_dir.join(AC_PROOF_JSON).exists(), "{code} wrote a ledger");
        }
        // The unknown-criterion refusal lists the ids that DO exist.
        let report = amend(dir.path(), &opts("guarded", "AC-9", OTHER_RED_COMMAND, "why"));
        let remedy = report.remedy.unwrap_or_default();
        assert!(remedy.contains("AC-1") && remedy.contains("AC-2"), "{remedy}");

        // Two-sided: the same seed accepts a real amendment, so the assertions
        // above cannot pass by the command being inert.
        let ok = amend(
            dir.path(),
            &opts("guarded", "AC-2", OTHER_RED_COMMAND, "state a real reason"),
        );
        assert!(ok.ok, "unexpected refusal: {:?}", ok.error);
    }

    /// Round trip on the line surgery: every AC shape the parser accepts is
    /// rewritten into a line the SAME parser reads back as the new command.
    #[test]
    fn rewritten_lines_parse_back_as_the_new_command() {
        let plan = Rewrite {
            command: "cargo test -p mustard-rt new_name".to_string(),
            expect: Some("2 passed".to_string()),
            statement: Some("when amended, then the door reads the new command".to_string()),
            control: None,
        };
        for original in [
            // Drafter multi-line form, with and without an Expect line.
            "## Acceptance Criteria\n- **AC-2** — old statement.\n  Command: `cd old`\n",
            "## Acceptance Criteria\n- **AC-2** — old statement.\n  Command: `cd old`\n  Expect: `1 passed`\n",
            // Historical one-line form, with and without a same-line Expect.
            "## Acceptance Criteria\n- [ ] AC-2: old statement — Command: `cd old`\n",
            "## Acceptance Criteria\n- [ ] AC-2: old statement — Command: `cd old` Expect: `1 passed`\n",
            // CRLF document.
            "## Acceptance Criteria\r\n- **AC-2** — old statement.\r\n  Command: `cd old`\r\n",
        ] {
            let updated = rewrite_markdown(original, "AC-2", &plan)
                .unwrap_or_else(|| panic!("nothing rewritten in {original:?}"));
            let item = criteria_of(&updated)
                .into_iter()
                .find(|i| i.id == "AC-2")
                .unwrap_or_else(|| panic!("AC-2 unreadable after rewrite: {updated:?}"));
            assert_eq!(item.command, plan.command, "{updated:?}");
            assert_eq!(item.expect.as_deref(), Some("2 passed"), "{updated:?}");
            assert_eq!(item.statement, plan.statement.clone().unwrap_or_default(), "{updated:?}");
            assert!(!updated.contains("cd old"), "superseded command left behind: {updated:?}");
            assert!(!updated.contains("old statement"), "{updated:?}");
        }
    }

    /// An AC line OUTSIDE the acceptance-criteria section is prose, not a
    /// criterion — the rewrite never touches it.
    #[test]
    fn only_criterion_lines_inside_the_ac_section_are_rewritten() {
        let plan = Rewrite {
            command: "cd new".to_string(),
            expect: None,
            statement: None,
            control: None,
        };
        let md = "## Tasks\n\n- AC-2: mentioned in prose — Command: `cd old`\n\n\
                  ## Acceptance Criteria\n\n- **AC-2** — real.\n  Command: `cd old`\n";
        let updated = rewrite_markdown(md, "AC-2", &plan).expect("the real criterion changed");
        assert!(
            updated.contains("mentioned in prose — Command: `cd old`"),
            "the prose bullet must survive verbatim: {updated}"
        );
        assert!(updated.contains("  Command: `cd new`"), "{updated}");
        // A document with nothing to change reports exactly that.
        assert!(rewrite_markdown("## Acceptance Criteria\n\n- **AC-1** — x.\n  Command: `cd a`\n", "AC-2", &plan).is_none());
    }

    /// A `--statement` rewrite replaces the WHOLE statement block, not just the
    /// line the parser reads.
    ///
    /// The defect this pins, found in review on 2026-07-28: amending a criterion of the
    /// spec that built this door left the superseded statement's continuation
    /// lines on disk, orphaned under the new sentence — the reader saw the new
    /// statement welded to the tail of the old one, and it had to be cleaned by
    /// hand. `wave-plan.md` and the wave spec were untouched only because the
    /// line happened to fit there, which is luck, not a rule.
    ///
    /// Two-sided:
    ///
    /// 1. **The residue is gone.** No fragment of the old statement survives,
    ///    the new one is what the parser reads back, and the command line below
    ///    it is untouched.
    /// 2. **Nothing else is eaten.** Without `--statement` the continuation
    ///    lines stay exactly where they were (a command-only amendment must not
    ///    rewrite prose), and a line carrying an `Expect:` marker is never
    ///    dropped even when it sits inside the block.
    #[test]
    fn ac_amend_rewrites_the_whole_statement_block() {
        // The drafter's wrapped shape: header + two continuation lines.
        let wrapped = "## Acceptance Criteria\n\
                       - **AC-1** — when the criterion being replaced is recorded as\n  \
                       inexecutable, then ac-amend accepts a substitute that passes,\n  \
                       instead of refusing everything that is not red\n  \
                       Command: `cd old`\n  Expect: `1 passed`\n";

        // --- 1. With a statement: the block is replaced whole ---------------
        let plan = Rewrite {
            command: "cd new".to_string(),
            expect: None,
            statement: Some("when a spec is closed, then the pipeline takes the confirmation"
                .to_string()),
            control: None,
        };
        let updated = rewrite_markdown(wrapped, "AC-1", &plan).expect("the criterion changed");
        let item = criteria_of(&updated)
            .into_iter()
            .find(|i| i.id == "AC-1")
            .unwrap_or_else(|| panic!("AC-1 unreadable after rewrite: {updated:?}"));
        assert_eq!(item.statement, plan.statement.clone().unwrap_or_default(), "{updated:?}");
        assert_eq!(item.command, "cd new", "{updated:?}");
        // The orphaned residue: every fragment of the old statement is gone.
        for orphan in [
            "inexecutable, then ac-amend accepts",
            "instead of refusing everything that is not red",
            "recorded as",
        ] {
            assert!(
                !updated.contains(orphan),
                "superseded statement line survived ({orphan:?}): {updated:?}"
            );
        }
        // The command block below it is intact — only the statement was eaten.
        assert!(updated.contains("  Command: `cd new`"), "{updated:?}");
        assert!(updated.contains("  Expect: `1 passed`"), "{updated:?}");

        // --- 2. Without a statement: the prose is left ALONE -----------------
        let command_only = Rewrite {
            command: "cd new".to_string(),
            expect: None,
            statement: None,
            control: None,
        };
        let untouched =
            rewrite_markdown(wrapped, "AC-1", &command_only).expect("the command changed");
        assert!(
            untouched.contains("instead of refusing everything that is not red"),
            "a command-only amendment must not rewrite prose: {untouched:?}"
        );

        // --- 3. A marker line inside the block is never dropped -------------
        let odd = "## Acceptance Criteria\n\
                   - **AC-1** — old statement wrapped\n  \
                   over two lines\n  \
                   Expect: `1 passed`\n  \
                   Command: `cd old`\n";
        let kept = rewrite_markdown(odd, "AC-1", &plan).expect("the criterion changed");
        assert!(kept.contains("Expect: `1 passed`"), "marker line eaten: {kept:?}");
        assert!(!kept.contains("over two lines"), "{kept:?}");
    }

    /// The shape the multi-line fix did NOT reach: a criterion carrying no
    /// `Command:` line at all. The block boundary used to be derived from the
    /// command line, so with no command the rewrite gave up AFTER replacing the
    /// header — leaving the superseded continuation lines orphaned underneath
    /// the new statement, which is the very defect in a different shape.
    ///
    /// Two-sided: the residue is gone from the command-less criterion, and the
    /// NEXT criterion's own statement block — which begins right after it — is
    /// untouched, so the fix cannot pass by eating everything below the header.
    #[test]
    fn a_criterion_without_a_command_line_still_loses_its_whole_statement_block() {
        let commandless = "## Acceptance Criteria\n\
                           - **AC-1** — when the plan is read, then the duty is\n  \
                           stated in full over several lines\n  \
                           and ends with nothing to run\n\
                           - **AC-2** — the sibling keeps its own wrapped\n  \
                           statement intact\n  \
                           Command: `cd other`\n";
        let plan = Rewrite {
            command: "cd new".to_string(),
            expect: None,
            statement: Some("when the plan is read, then the duty is stated once".to_string()),
            control: None,
        };
        let updated = rewrite_markdown(commandless, "AC-1", &plan)
            .expect("the command-less criterion changed");
        for orphan in ["stated in full over several lines", "and ends with nothing to run"] {
            assert!(
                !updated.contains(orphan),
                "superseded line survived under a criterion with no Command ({orphan:?}): {updated:?}"
            );
        }
        assert!(
            updated.contains("when the plan is read, then the duty is stated once"),
            "the new statement is what the reader gets: {updated:?}"
        );
        // The sibling's block starts where AC-1's stops — it must be untouched.
        assert!(
            updated.contains("statement intact") && updated.contains("Command: `cd other`"),
            "the next criterion's block was eaten: {updated:?}"
        );
    }

    /// Criterion ids are normalised to the spelling the parser yields, so a
    /// caller typing `--ac 2` or `--ac ac-2` reaches the same criterion.
    #[test]
    fn criterion_ids_are_normalised() {
        assert_eq!(normalise_id("ac-2"), "AC-2");
        assert_eq!(normalise_id(" AC-W4-1 "), "AC-W4-1");
        assert_eq!(normalise_id("2"), "AC-2");
    }
}
