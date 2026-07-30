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
//! ## The one exception: an INEXECUTABLE predecessor
//!
//! There is exactly one criterion the red rule cannot repair. When the
//! confirmation pass finds a criterion the executor could not attempt AT ALL
//! after its work landed
//! ([`ac_negative_check::Confirmation::Inexecutable`]), the command is broken
//! whatever the work does — and by then the work IS done, so the corrected
//! command legitimately PASSES. Demanding a red there is demanding a criterion
//! that lies about a feature that exists.
//!
//! So for that ONE recorded state, and only it, a replacement that comes back
//! GREEN is accepted, and its record carries a green CONFIRMATION — the
//! evidence the approval gate reads. Everything else keeps refusing a
//! replacement that is not red. The exception is not a knob and cannot be
//! asked for: it is unlocked by a finding the engine itself recorded, which is
//! why it cannot be used to smuggle a vacuous criterion past the door.
//! 2. **Only the root is edited.** `wave-plan.md` and each `wave-*/spec.md`
//!    carry the criterion lines too, and the scaffold is frozen after approval
//!    (`wave_scaffold.rs`). A criterion amended only at the root leaves the
//!    dispatched agent reading the superseded command.
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

/// Give `line` an `Expect:` marker carrying `value` — rewriting the one it has,
/// or appending one when it has none.
///
/// The append lands on the COMMAND's own line on purpose: that is the one place
/// both AC shapes read `Expect:` from. The one-line historical form
/// (`- [ ] AC-1: … Command: \`c\``) never looks at following lines at all, so an
/// `Expect:` appended below it would be silently ignored.
fn ensure_expect(line: &str, value: &str) -> String {
    if let Some(rewritten) = replace_marker_value(line, "expect:", value) {
        return rewritten;
    }
    let (body, cr) = split_cr(line);
    format!("{body} Expect: `{value}`{cr}")
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
/// amending AC-1 of this very spec, and cleaned by hand; the hand is exactly
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
            let Some(expect) = plan.expect.as_deref() else {
                continue;
            };
            // A standalone `Expect:` line is only ever read for the drafter
            // form, so only that form looks for one.
            let expect_line = if marker_end(&out[k], "expect:").is_some() || inline {
                k
            } else {
                (k + 1..block_end)
                    .find(|m| marker_end(lines[*m], "expect:").is_some())
                    .unwrap_or(k)
            };
            out[expect_line] = ensure_expect(&out[expect_line], expect);
            changed = true;
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

/// Every markdown artefact under a spec directory that can carry criterion
/// lines: the root `spec.md` / `wave-plan.md` and each `wave-*/spec.md`.
///
/// Sorted, so the report and the ledger are byte-stable. Depth 2 by
/// construction — the scaffold puts wave artefacts exactly one level down, and
/// an unbounded walk would reach `review/` transcripts nobody dispatches from.
fn artefacts(spec_dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(spec_dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Ok(inner) = std::fs::read_dir(&path) else {
                continue;
            };
            found.extend(
                inner
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "md")),
            );
        } else if path.extension().is_some_and(|e| e == "md") {
            found.push(path);
        }
    }
    found.sort();
    found
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
    // No `Control:` is declared through this door: the amendment replaces the
    // command and (optionally) the expect regex, so the record says the control
    // was not declared. When the markdown still carries one, the next
    // `ac-negative-check` pass sees it differ from the record and takes it — the
    // safe direction, since it can only cause a run, never a stale reuse.
    let mut proof = ac_negative_check::prove_one(
        root,
        &id,
        &opts.command,
        expect.as_deref(),
        None,
        exempt,
    );

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

    if predecessor_inexecutable && proof.proof == Proof::Green {
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
        let mut report = AcAmendReport::refused(
            opts,
            &id,
            "replacement_not_proven",
            &format!(
                "the REPLACEMENT does not clear the negative test, so it proves exactly as \
                 little as the criterion it would replace — {reason}"
            ),
        );
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

/// The process exit code for a finished report: `0` accepted, `1` refused.
fn exit_code(report: &AcAmendReport) -> i32 {
    i32::from(!report.ok)
}

/// CLI entry — `mustard-rt run ac-amend`.
pub fn run(opts: AcAmendOpts) {
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

    /// Seed `<root>/.claude/spec/<spec>/` with a root `spec.md` and a planted
    /// wave artefact carrying the SAME criterion line — the frozen scaffold the
    /// dispatched agent actually reads.
    fn seed(root: &Path, spec: &str) -> PathBuf {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(dir.join("wave-2-rt")).unwrap();
        std::fs::write(dir.join("spec.md"), spec_body()).unwrap();
        std::fs::write(
            dir.join("wave-2-rt").join("spec.md"),
            format!(
                "# Wave 2\n\n## Acceptance Criteria\n\
                 - **AC-2** — when the work lands, then the other thing holds.\n  Command: `{GREEN_COMMAND}`\n"
            ),
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
        }
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
    /// EVERY artefact carrying the id — the root AND the frozen wave scaffold —
    /// and the ledger records the superseded version with the stated reason.
    #[test]
    fn amend_records_the_previous_version_and_rewrites_every_artifact() {
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
                ".claude/spec/amended/wave-2-rt/spec.md".to_string(),
            ],
            "the root AND the frozen wave artefact"
        );

        // The dispatched agent reads the NEW command out of the wave scaffold —
        // this is the whole reason a root-only amendment is not enough.
        for path in [spec_dir.join("spec.md"), spec_dir.join("wave-2-rt").join("spec.md")] {
            let body = std::fs::read_to_string(&path).unwrap();
            let item = criteria_of(&body)
                .into_iter()
                .find(|i| i.id == "AC-2")
                .unwrap_or_else(|| panic!("AC-2 missing from {}", path.display()));
            assert_eq!(item.command, OTHER_RED_COMMAND, "{}", path.display());
            assert_eq!(item.expect.as_deref(), Some("1 passed"), "{}", path.display());
        }
        // The wave artefact carries AC-2 alone, so the superseded command must
        // be gone from it entirely — nothing else could be holding it there.
        assert!(
            !std::fs::read_to_string(spec_dir.join("wave-2-rt").join("spec.md"))
                .unwrap()
                .contains(GREEN_COMMAND),
            "the superseded command must be gone from the wave artefact"
        );

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

    /// AC-2 — the one case the red rule cannot express. A criterion the engine
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
    /// The defect this pins, found in review on 2026-07-28: amending AC-1 of the
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
