//! `mustard-rt run finding-collect` — seed `meta.json#findings` from the two
//! producers that already wrote their discoveries to disk.
//!
//! # Why a collector rather than a hand-written list
//!
//! A finding is a verified discovery made INSIDE a work unit, and this project
//! has exactly two machines that produce them today. The reviewer subagent
//! writes `<spec>/review/findings*.md`; the acceptance-criteria proof ledger
//! writes `<spec>/ac-proof.json`, where the `removal` column already names the
//! two discoveries it makes about a criterion — [`Removal::Survived`] (the
//! criterion stayed green with the work torn out, so it verifies something the
//! work did not do) and [`Removal::EvidenceRemoved`] (the strip took the
//! criterion's own evidence with it, a declared coverage gap). Both files are
//! machine-readable and both are read by nobody.
//!
//! Asking the model to retype either of them into a third place reintroduces
//! exactly the loss the pipe exists to remove: what the hand does not retype is
//! gone. So the seeding is DETERMINISTIC — this module reads the files, and the
//! only thing a human ever writes is the finding's DESTINATION.
//!
//! Both producers enter through the SAME gate for the same reason. A collector
//! that read only the reviewer's markdown would leave the ledger exactly as it
//! is, and one that read only the ledger would leave the reviewer. The defect is
//! the pipe with no outlet, not either source.
//!
//! # A finding is a DISCOVERY, never the file that carries it
//!
//! The identity of a finding is its statement, not its container. Both producers
//! REWRITE their file in place — `review_result` overwrites `review/findings.md`
//! on every round, and the proof pass rewrites `ac-proof.json` — so an id taken
//! from the file name (or from the criterion id alone) would hand round two's
//! discovery the destination somebody declared for round one's. That is the
//! opposite of what a record is for: a decision would be inherited by a finding
//! nobody ever read.
//!
//! So an id is `<producer's own name>-<fingerprint of the statement>`: the
//! reviewer's file stem or the criterion id, which keeps the finding walkable
//! back to its source, plus a digest of what was actually found. A different
//! discovery under the same file, or under the same criterion id, is a DIFFERENT
//! finding and is born open. A criterion that moves from `survived` to
//! `evidence-removed` is likewise a new finding, because the removal column is
//! part of the fingerprint: the two columns say different things about it.
//!
//! A reviewer's file carries as many findings as the reviewer itemised, and each
//! one becomes its OWN record — see [`review_statements`]. One file, one record
//! would let a single `--to dropped` settle N discoveries of which N-1 never
//! reached the sidecar at all.
//!
//! # What seeding does and does not touch
//!
//! [`collect`] reconciles rather than overwrites. A finding whose destination
//! was already declared keeps it across every later collection — that decision
//! is the one thing on the record no machine can reproduce, and it survives even
//! when the producer goes quiet: a file rewritten between rounds, or a proof pass
//! not re-taken, must not delete the decision along with the discovery. A finding
//! nobody had decided about, whose source stopped reporting it, IS dropped —
//! there is nothing on that record worth keeping. A new one enters with no
//! destination at all, which is the OPEN position [`FindingItem::is_open`]
//! reports.
//!
//! The proof ledger is read through [`ac_negative_check::load_ledger`], the ONE
//! reader of `ac-proof.json` in the crate, so the producer and this collector can
//! never disagree about what the file says. The reviewer's file names come from
//! the writer's own constants (`review_result::FINDINGS_FILE` /
//! `FINDINGS_SCOPED_PREFIX`) for the same reason.
//!
//! Output is one byte-stable JSON document: findings sorted by (source, id), no
//! timestamps and no absolute paths. This command DECIDES nothing — a verdict
//! over open findings belongs to the close gate that reads the seeded key.

use serde::Serialize;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec::contract::{FindingItem, FindingSource};
use mustard_core::io::fs;
use mustard_core::{read_meta, write_meta};

use crate::commands::review::ac_negative_check::{self, AC_PROOF_JSON, Removal};
use crate::commands::review::review_result::{FINDINGS_FILE, FINDINGS_SCOPED_PREFIX};
use crate::util::sha256::Sha256;

/// The spec-relative directory the reviewer writes its findings into — the same
/// one `review_result` creates when it persists them.
const REVIEW_DIR: &str = "review";

/// The sidecar the findings are seeded into.
const META_JSON: &str = "meta.json";

/// The prefix every reviewer-side finding id carries. Its middle is the findings
/// file's own stem, so `F-findings-apps-rt-1a2b3c4d` names a discovery inside
/// `review/findings-apps-rt.md` and a reader walks back from the id to the file
/// without a lookup table.
const REVIEW_ID_PREFIX: &str = "F-";

/// How many hex characters of the statement's SHA-256 close an id.
///
/// Eight, where the sibling content-id in `tactical_fix_detect` takes sixteen,
/// because this id is typed back by a HUMAN into a `mark-finding` command and it
/// only has to be unique inside one spec's finding list — not across a corpus.
const FINGERPRINT_HEX_CHARS: usize = 8;

/// The severity words the reviewer agent's own contract publishes
/// (`plugin/agents/mustard-review.md`: `"severity":"critical"|"major"|"minor"`).
/// English on purpose and not a locale string: this is the SCHEMA's vocabulary,
/// the same one the reviewer writes into its `<VERDICT>` block, not prose.
const SEVERITY_WORDS: [&str; 3] = ["critical", "major", "minor"];

/// The word that marks a heading as announcing findings ("Findings",
/// "Non-blocking findings", "minor findings"). Matched lowercased and as a
/// substring, so the plural is covered by the singular.
const FINDING_WORD: &str = "finding";

/// How many words past its severity token a heading needs before it is read as a
/// sentence rather than as a label plus, at most, a locator — see
/// [`is_bare_label`].
const SENTENCE_WORDS: usize = 2;

/// Upper bound, in characters, on a statement lifted out of a reviewer's
/// markdown. The ledger's own reason is NOT bounded: this crate authored it as
/// one finished sentence, while a findings file is arbitrary prose whose first
/// line can be a whole paragraph.
const STATEMENT_MAX_CHARS: usize = 240;

/// No spec was named, so nothing could be collected.
const ERR_SPEC_REQUIRED: &str = "spec-required";
/// The named spec does not resolve to a markdown on disk.
const ERR_SPEC_NOT_FOUND: &str = "spec-not-found";
/// The spec directory carries no `meta.json`, so the findings have nowhere to
/// be seeded. Reported rather than repaired: inventing a sidecar for a spec the
/// harness never scaffolded would write state nobody asked for.
const ERR_META_NOT_FOUND: &str = "meta-not-found";
/// The reconciled list could not be persisted.
const ERR_META_WRITE_FAILED: &str = "meta-write-failed";

/// What one collection did.
///
/// `ok` is false only when the collection could not be RECORDED — an unresolved
/// spec, a missing sidecar, a failed write. A spec whose producers have written
/// nothing yet collects zero findings and is a clean `true`: there is nothing
/// wrong with a work unit that has made no discoveries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FindingCollectReport {
    /// See the type doc — recording, not emptiness.
    pub ok: bool,
    /// The spec as the caller named it.
    pub spec: String,
    /// How many findings came from `review/findings*.md`.
    pub from_review: usize,
    /// How many came from the `removal` column of `ac-proof.json`.
    pub from_proof_ledger: usize,
    /// How many of the seeded findings still owe a destination.
    pub open: usize,
    /// How many UNDECIDED findings the sidecar carried that neither source still
    /// produces, and which this collection therefore dropped. A DECIDED one is
    /// never dropped — see `retained`.
    pub stale: usize,
    /// How many findings are on the record only because somebody had already
    /// declared their destination: no producer still reports them, and the
    /// decision is kept so a source that comes back does not find it gone.
    pub retained: usize,
    /// Whether the sidecar was rewritten. False when it already said exactly
    /// this — including the empty case, where the key must not appear at all.
    pub written: bool,
    /// The findings as they now stand, sorted by (source, id).
    pub findings: Vec<FindingItem>,
    /// The one thing that stopped the collection from being recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}

impl FindingCollectReport {
    /// A collection that never happened, naming why.
    fn aborted(spec: &str, error: &'static str) -> Self {
        Self {
            ok: false,
            spec: spec.to_string(),
            from_review: 0,
            from_proof_ledger: 0,
            open: 0,
            stale: 0,
            retained: 0,
            written: false,
            findings: Vec::new(),
            error: Some(error),
        }
    }
}

/// Collect both producers for `spec` and reconcile them into
/// `<spec-dir>/meta.json#findings`.
///
/// The project root is a PARAMETER rather than the process working directory,
/// for the reason the rest of this family takes one: the tool cuts a worktree per
/// work unit, so the engine runs off-root as a matter of course.
#[must_use]
pub(crate) fn collect(root: &Path, spec: &str) -> FindingCollectReport {
    let Some(spec_dir) = ac_negative_check::resolve_spec_file(root, spec)
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
    else {
        return FindingCollectReport::aborted(spec, ERR_SPEC_NOT_FOUND);
    };

    let mut fresh = collect_review(&spec_dir);
    let from_review = fresh.len();
    fresh.extend(collect_ledger(&spec_dir));
    let from_proof_ledger = fresh.len() - from_review;
    // Taken BEFORE the reconciliation consumes the list: a seeded finding that
    // no producer reported is one kept for its decision alone.
    let produced: Vec<(u8, String)> = fresh.iter().map(key_of).collect();

    let meta_path = spec_dir.join(META_JSON);
    let Some(mut meta) = read_meta(&meta_path) else {
        // Nothing to seed into. An empty collection is still the honest clean
        // answer; anything collected has nowhere to go, and says so.
        let seeded = reconcile(&[], fresh);
        let empty = seeded.is_empty();
        return FindingCollectReport {
            ok: empty,
            spec: spec.to_string(),
            from_review,
            from_proof_ledger,
            open: seeded.iter().filter(|f| f.is_open()).count(),
            stale: 0,
            retained: 0,
            written: false,
            findings: seeded,
            error: (!empty).then_some(ERR_META_NOT_FOUND),
        };
    };

    let seeded = reconcile(&meta.findings, fresh);
    let stale = meta
        .findings
        .iter()
        .filter(|old| !seeded.iter().any(|new| same_finding(new, old)))
        .count();
    let retained = seeded.iter().filter(|f| !produced.contains(&key_of(f))).count();
    let open = seeded.iter().filter(|f| f.is_open()).count();

    let mut written = false;
    let mut error = None;
    if seeded == meta.findings {
        // The sidecar already says exactly this. Not writing is what keeps a
        // spec with neither producer from growing a `findings` key it never had.
    } else {
        meta.findings.clone_from(&seeded);
        match write_meta(&meta_path, &meta) {
            Ok(()) => written = true,
            Err(_) => error = Some(ERR_META_WRITE_FAILED),
        }
    }

    FindingCollectReport {
        ok: error.is_none(),
        spec: spec.to_string(),
        from_review,
        from_proof_ledger,
        open,
        stale,
        retained,
        written,
        findings: seeded,
        error,
    }
}

/// Two records are the SAME finding when the producer and the id agree. The
/// source is half the key on purpose: a reviewer file and a criterion can carry
/// the same string, and folding them would hand one producer's destination to
/// the other's discovery.
fn same_finding(a: &FindingItem, b: &FindingItem) -> bool {
    a.id == b.id && a.source == b.source
}

/// The same identity [`same_finding`] compares, as a value that can be held
/// after the finding it came from was moved.
fn key_of(item: &FindingItem) -> (u8, String) {
    (source_rank(item.source), item.id.clone())
}

/// A finding id: what the producer calls the thing, plus a fingerprint of what
/// was actually found.
///
/// The fingerprint is why a rewritten source cannot smuggle a new discovery in
/// under an old decision. `base` stays a PREFIX so the id is still walkable back
/// to its producer — `AC-3-…` is the criterion's finding, `F-findings-…` lives in
/// `review/findings.md` — and `material` is everything that makes this discovery
/// this one and not another.
fn finding_id(base: &str, material: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(material.as_bytes());
    let digest: String = hasher.hex_digest().chars().take(FINGERPRINT_HEX_CHARS).collect();
    format!("{base}-{digest}")
}

/// Declaration order of the producers, so the sort is total and stable without
/// asking [`FindingSource`] to be `Ord` — a wire enum whose ordering would then
/// be a contract nobody wanted.
const fn source_rank(source: FindingSource) -> u8 {
    match source {
        FindingSource::Review => 0,
        FindingSource::ProofLedger => 1,
    }
}

/// Carry every already-declared destination onto the freshly collected list,
/// keep the ones whose producer went quiet, then sort.
///
/// This is the whole idempotency contract: the destination is the one thing on
/// the record a machine cannot reproduce, so a re-collection preserves it
/// VERBATIM — including a route written with a blank reason, which
/// [`FindingItem::route`] keeps reading as no route at all. The statement is
/// taken FRESH from the source, because the file on disk is what the finding
/// says today.
///
/// A finding the sources no longer report is kept when — and only when — a
/// destination was declared for it. Both producers rewrite their file in place,
/// so "not reported this time" is routinely just a round that overwrote the
/// previous one; deleting the record would delete the decision with it, and the
/// decision is precisely what this module promises to keep. Undecided and
/// unreported means nobody ever answered for it and no source still asks: that
/// one is dropped.
fn reconcile(existing: &[FindingItem], fresh: Vec<FindingItem>) -> Vec<FindingItem> {
    let mut out: Vec<FindingItem> = fresh
        .into_iter()
        .map(|mut item| {
            item.routed = existing
                .iter()
                .find(|old| same_finding(old, &item))
                .and_then(|old| old.routed.clone());
            item
        })
        .collect();
    let decided_but_unreported: Vec<FindingItem> = existing
        .iter()
        .filter(|old| old.is_routed() && !out.iter().any(|new| same_finding(new, old)))
        .cloned()
        .collect();
    out.extend(decided_but_unreported);
    out.sort_by(|a, b| {
        source_rank(a.source)
            .cmp(&source_rank(b.source))
            .then_with(|| a.id.cmp(&b.id))
    });
    // Two identical statements in one file fingerprint alike and are one
    // finding; the sort has already put any such pair side by side.
    out.dedup_by(|a, b| same_finding(a, b));
    out
}

/// `true` for a file the reviewer wrote: the spec-wide `findings.md`, or a
/// subproject-scoped `findings-<slug>.md`. Keyed off the WRITER's own constants
/// so the two sides cannot drift into two spellings of the same file.
fn is_findings_file(name: &str) -> bool {
    name == FINDINGS_FILE || (name.starts_with(FINDINGS_SCOPED_PREFIX) && name.ends_with(".md"))
}

/// One finding per finding the reviewer ITEMISED, across every reviewer file,
/// sorted by file name. An absent `review/` directory yields none — the reviewer
/// simply has not spoken yet.
fn collect_review(spec_dir: &Path) -> Vec<FindingItem> {
    let Ok(entries) = fs::read_dir(spec_dir.join(REVIEW_DIR)) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.path.is_file() && is_findings_file(&entry.file_name))
        .collect();
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    files
        .into_iter()
        .flat_map(|entry| {
            let body = fs::read_to_string(&entry.path).unwrap_or_default();
            let stem = entry.file_name.strip_suffix(".md").unwrap_or(&entry.file_name);
            let base = format!("{REVIEW_ID_PREFIX}{stem}");
            let mut statements = review_statements(&body);
            if statements.is_empty() {
                // Nothing itemised: the file still counts as ONE finding, the
                // way it always did — a shape this reader cannot split must
                // never collapse to silence. A file with nothing quotable at all
                // is named by its own file name rather than described:
                // authoring a sentence here would put this collector's words
                // where the reviewer's belong.
                statements.push(statement_of(&body).unwrap_or_else(|| entry.file_name.clone()));
            }
            statements
                .into_iter()
                .map(|statement| FindingItem {
                    id: finding_id(&base, &statement),
                    source: FindingSource::Review,
                    statement,
                    routed: None,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every finding a reviewer's markdown itemises, in file order.
///
/// # The rule, and why it is this one
///
/// The reviewer agent's contract names three severities —
/// `critical` / `major` / `minor` — and the corpus of files it has written uses
/// them in exactly two shapes: a section whose HEADING is severity-led
/// (`## MAJOR — the gate is not reached…`), and a LIST ITEM under a heading that
/// announces findings (`## MINOR`, `## Findings (non-blocking)`) or that is
/// itself severity-led (`- minor — the report emits an absolute path`). Both
/// shapes are read here, and nothing else is: a heading like
/// `## Acceptance Criteria` carries bullets that are results, not discoveries,
/// and minting findings out of them would teach the reader to route around the
/// gate.
///
/// Fenced code blocks are skipped whole — a `# comment` or a `- item` inside a
/// transcript is not a heading and not a finding.
///
/// A file this rule cannot split yields NOTHING here, and the caller falls back
/// to one record for the file: the reader may be narrower than a reviewer's
/// prose, but it is never allowed to turn a file with findings into silence.
fn review_statements(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut section: Option<ReviewSection> = None;
    let mut fenced = false;

    for logical in logical_lines(body) {
        let line = logical.as_str();
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(title) = heading_title(line) {
            close_section(&mut out, section.take());
            section = Some(ReviewSection::new(title));
            continue;
        }
        if let Some(item) = top_level_bullet(line) {
            let announced = section.as_ref().is_some_and(|s| s.announces_findings);
            if announced || starts_with_severity(item) {
                out.push(bound(&one_line(item)));
                if let Some(open) = section.as_mut() {
                    open.claimed = true;
                }
            }
            continue;
        }
        if let Some(open) = section.as_mut() {
            open.remember_body(line);
        }
    }
    close_section(&mut out, section);
    out.retain(|statement| !statement.is_empty());
    out
}

/// Fold a markdown body into LOGICAL lines: a paragraph or a list item the
/// reviewer soft-wrapped across several lines becomes ONE line, so a statement
/// is the sentence the reviewer wrote and not however far the wrap happened to
/// get. Blank lines, headings and fenced blocks are boundaries and come through
/// verbatim — the fence markers included, so a transcript can still be skipped
/// whole. A nested list item is a continuation of the item above it, which is
/// exactly how it reads.
fn logical_lines(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    let mut fenced = false;

    for raw in body.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        let boundary = fence || fenced || trimmed.is_empty() || heading_title(line).is_some();
        if boundary || top_level_bullet(line).is_some() {
            out.extend(pending.take());
        }
        if fence {
            fenced = !fenced;
        }
        match (boundary, top_level_bullet(line).is_some(), pending.as_mut()) {
            (true, _, _) => out.push(line.to_string()),
            (false, true, _) => pending = Some(line.to_string()),
            (false, false, Some(open)) => {
                open.push(' ');
                open.push_str(trimmed);
            }
            (false, false, None) => pending = Some(line.to_string()),
        }
    }
    out.extend(pending);
    out
}

/// The heading a reviewer's markdown is currently under, and what has been made
/// of it. A severity-led section whose bullets never claimed it IS the finding —
/// that is the `## MAJOR — <sentence>` shape, whose body is the explanation.
struct ReviewSection {
    /// The heading text, markers stripped.
    title: String,
    /// The heading opens with `critical` / `major` / `minor`.
    severity_led: bool,
    /// The heading is severity-led OR names findings, so its list items are
    /// findings even when they carry no severity word of their own.
    announces_findings: bool,
    /// A list item has already been taken out of this section.
    claimed: bool,
    /// The section's first quotable body line — used only when the heading is a
    /// bare label (`## MINOR`) and would otherwise say nothing.
    body: Option<String>,
}

impl ReviewSection {
    /// Classify one heading.
    fn new(title: String) -> Self {
        let severity_led = starts_with_severity(&title);
        let announces_findings =
            severity_led || title.to_lowercase().contains(FINDING_WORD);
        Self {
            title,
            severity_led,
            announces_findings,
            claimed: false,
            body: None,
        }
    }

    /// Keep the first line of prose under the heading, decoration aside.
    fn remember_body(&mut self, line: &str) {
        let text = line.trim();
        if self.body.is_some() || text.is_empty() || is_decoration(text) {
            return;
        }
        self.body = Some(one_line(strip_marker(text)));
    }

    /// The statement this heading stands for, or `None` when it stands for
    /// nothing.
    ///
    /// A heading that is only a LABEL (`## MINOR`) stands for the paragraph
    /// under it, and for nothing at all once its own list items have been taken
    /// — the items are the findings and the label was their header. A heading
    /// that carries a SENTENCE is a finding in its own right and stays one even
    /// when bullets under it were also collected: those are a second discovery,
    /// never a restatement of the first.
    fn statement(&self) -> Option<String> {
        if !self.severity_led {
            return None;
        }
        match (is_bare_label(&self.title), &self.body) {
            (true, _) if self.claimed => None,
            (true, Some(body)) => Some(bound(&format!("{} — {body}", self.title))),
            _ => Some(bound(&self.title)),
        }
    }
}

/// `true` when a severity-led heading names no discovery — `## MINOR`,
/// `## MINOR (non-blocking)`, or `## major — `branch_state.rs:515`` , which
/// gives a place and not a finding. One word past the severity is a LOCATOR; two
/// are a sentence, and `## MAJOR — reviewer-side granularity` stands alone. A
/// parenthetical is an aside about the severity, never the discovery, so it does
/// not count either way.
fn is_bare_label(title: &str) -> bool {
    let mut depth = 0usize;
    let outside: String = title
        .chars()
        .filter(|c| {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                _ => return depth == 0,
            }
            false
        })
        .collect();
    outside
        .split_whitespace()
        .skip(1)
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .count()
        < SENTENCE_WORDS
}

/// Push what a closed section stood for, if anything.
fn close_section(out: &mut Vec<String>, section: Option<ReviewSection>) {
    if let Some(statement) = section.as_ref().and_then(ReviewSection::statement) {
        out.push(statement);
    }
}

/// The text of an ATX heading (`## Title`), folded to one line. `None` for any
/// other line.
fn heading_title(line: &str) -> Option<String> {
    let rest = line.strip_prefix('#')?.trim_start_matches('#');
    let title = rest.trim();
    (!title.is_empty()).then(|| one_line(title))
}

/// The text of a TOP-LEVEL list item — indentation excluded on purpose, since a
/// nested bullet is part of the item above it, not a finding of its own.
fn top_level_bullet(line: &str) -> Option<&str> {
    let bulleted =
        line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ");
    bulleted.then(|| strip_marker(line))
}

/// `true` when the first word is one of the reviewer's severities, whatever
/// punctuation or emphasis it is wearing (`**MINOR**`, `minor —`, `CRITICAL 1`).
fn starts_with_severity(text: &str) -> bool {
    text.split_whitespace().next().is_some_and(|first| {
        let word = first
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        SEVERITY_WORDS.contains(&word.as_str())
    })
}

/// One finding per criterion whose `removal` column recorded a discovery.
///
/// [`Removal::Survived`] and [`Removal::EvidenceRemoved`] are the only two
/// values that say something about the criterion nobody has acted on; every
/// other value is either a clean red or a pass that was never taken. The
/// statement is the ledger's own `reason`, carried IN FULL — it already names
/// what happened and the one action that clears it, and re-summarising it here
/// would replace evidence with paraphrase.
fn collect_ledger(spec_dir: &Path) -> Vec<FindingItem> {
    let Some(ledger) = ac_negative_check::load_ledger(&spec_dir.join(AC_PROOF_JSON)) else {
        return Vec::new();
    };
    ledger
        .criteria
        .iter()
        .filter_map(|criterion| {
            let column = removal_word(criterion.removal)?;
            // A ledger that recorded the column without a reason (a later pass
            // owns that field) still identifies the criterion by the command it
            // ran — a fact, never an invented sentence.
            let statement = one_line(
                criterion
                    .reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or(criterion.command.as_str()),
            );
            Some(FindingItem {
                // The COLUMN is part of the fingerprint, not only the reason: a
                // criterion that moves from `survived` to `evidence-removed` has
                // been found out for a different thing, and a decision taken
                // about the first must not settle the second.
                id: finding_id(&criterion.id, &format!("{column}|{statement}")),
                source: FindingSource::ProofLedger,
                statement,
                routed: None,
            })
        })
        .collect()
}

/// The ledger word for a removal column that RECORDED a discovery, and `None`
/// for every column that made none: a clean red, a pass never taken, a run with
/// no verdict. Spelled as the ledger's own kebab-case serde words, so an id
/// minted here and the file it came from cannot drift apart.
const fn removal_word(removal: Removal) -> Option<&'static str> {
    match removal {
        Removal::Survived => Some("survived"),
        Removal::EvidenceRemoved => Some("evidence-removed"),
        Removal::NotTaken | Removal::Red | Removal::NoVerdict | Removal::NotAttempted => None,
    }
}

/// The first line of a reviewer's markdown that carries a statement, folded to
/// one bounded line. `None` when the file is empty or holds nothing but
/// decoration.
fn statement_of(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !is_decoration(line))
        .map(|line| bound(&one_line(strip_marker(line))))
        .filter(|statement| !statement.is_empty())
}

/// `true` for a line that carries no statement: an ATX heading, an HTML comment
/// (the marker shape the harness stamps into its own files), or a thematic
/// break.
fn is_decoration(line: &str) -> bool {
    line.starts_with('#')
        || line.starts_with("<!--")
        || line.chars().all(|c| c == '-' || c == '*' || c == '_')
}

/// Drop a leading list bullet and checkbox so the statement starts at the
/// reviewer's own words.
fn strip_marker(line: &str) -> &str {
    let body = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .unwrap_or(line)
        .trim_start();
    body.strip_prefix("[ ] ")
        .or_else(|| body.strip_prefix("[x] "))
        .or_else(|| body.strip_prefix("[X] "))
        .unwrap_or(body)
        .trim_start()
}

/// Collapse every whitespace run to a single space, so one finding stays one
/// line in a sidecar three consumers read line-wise.
///
/// `pub(crate)` so `mark-finding` folds the DESTINATION's reason exactly as the
/// collector folds the statement: both halves of one record are printed on one
/// line by the close gate, and two foldings are how they would drift.
pub(crate) fn one_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cap a statement at [`STATEMENT_MAX_CHARS`] CHARACTERS (never bytes — a byte
/// cut lands inside a multi-byte grapheme and produces a panic or mojibake).
fn bound(text: &str) -> String {
    if text.chars().count() <= STATEMENT_MAX_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(STATEMENT_MAX_CHARS).collect();
    out.push('…');
    out
}

/// Dispatch `mustard-rt run finding-collect`.
pub fn run(spec: Option<&str>) {
    let root = PathBuf::from(crate::shared::context::project_dir());
    let report = match spec.map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => collect(&root, spec),
        None => FindingCollectReport::aborted("", ERR_SPEC_REQUIRED),
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::spec::contract::FindingRoute;
    use tempfile::tempdir;

    /// A ledger entry the collector must pick up.
    const SURVIVED_REASON: &str = "the REMOVAL was TAKEN and the command still came back green \
         with the work taken away, so this criterion is satisfied by something the work did not do";
    const EVIDENCE_REMOVED_REASON: &str = "the REMOVAL was NOT TAKEN: this criterion's own \
         evidence names `parse_finding`, which the strip itself deleted from the tree";

    /// Seed a spec directory with a `spec.md` and a `meta.json`; returns
    /// `(project, spec_dir)`. The spec is addressed by DIRECTORY in these tests,
    /// which `resolve_spec_file` accepts alongside a slug and a markdown path.
    fn seed(meta: &str) -> (tempfile::TempDir, PathBuf) {
        let project = tempdir().unwrap();
        let spec_dir = project.path().join(".claude").join("spec").join("demo");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Demo\n").unwrap();
        std::fs::write(spec_dir.join(META_JSON), meta).unwrap();
        (project, spec_dir)
    }

    /// Write the two reviewer files this suite uses.
    fn seed_review(spec_dir: &Path) {
        let review = spec_dir.join(REVIEW_DIR);
        std::fs::create_dir_all(&review).unwrap();
        std::fs::write(
            review.join("findings.md"),
            "# Findings\n\n- the close gate never reads this file\n",
        )
        .unwrap();
        std::fs::write(
            review.join("findings-apps-rt.md"),
            "the ledger's third column has no consumer\n",
        )
        .unwrap();
        // A neighbour that is NOT a findings file must be ignored.
        std::fs::write(review.join("verdict.md"), "# Review Verdict\n").unwrap();
    }

    /// Write a proof ledger with one survivor, one evidence-removed and one
    /// honest red (which is NOT a finding).
    fn seed_ledger(spec_dir: &Path) {
        std::fs::write(
            spec_dir.join(AC_PROOF_JSON),
            format!(
                r#"{{"spec":"demo","criteria":[
                    {{"id":"AC-1","command":"cd .","verdict":"unproven","proof":"red",
                      "removal":"survived","reason":"{SURVIVED_REASON}"}},
                    {{"id":"AC-2","command":"cd .","verdict":"proven","proof":"red",
                      "removal":"evidence-removed","reason":"{EVIDENCE_REMOVED_REASON}"}},
                    {{"id":"AC-3","command":"cd .","verdict":"proven","proof":"red",
                      "removal":"red"}}
                ]}}"#
            ),
        )
        .unwrap();
    }

    /// The one finding whose id starts with `prefix`. Ids carry a content
    /// fingerprint, so a test names the producer's half and lets the digest be
    /// whatever the statement makes it.
    fn by_prefix<'a>(report: &'a FindingCollectReport, prefix: &str) -> &'a FindingItem {
        let mut hits = report.findings.iter().filter(|f| f.id.starts_with(prefix));
        let first = hits.next().unwrap_or_else(|| {
            panic!("no finding under \"{prefix}\": {:?}", ids_of(report));
        });
        assert!(hits.next().is_none(), "\"{prefix}\" named more than one finding");
        first
    }

    /// Every collected id, for a failure message that shows what was there.
    fn ids_of(report: &FindingCollectReport) -> Vec<&str> {
        report.findings.iter().map(|f| f.id.as_str()).collect()
    }

    /// The one finding whose statement is `statement` — the identity a finding
    /// now has, which is why a test looks it up this way.
    fn by_statement<'a>(report: &'a FindingCollectReport, statement: &str) -> &'a FindingItem {
        report
            .findings
            .iter()
            .find(|f| f.statement == statement)
            .unwrap_or_else(|| {
                let seen: Vec<&str> = report.findings.iter().map(|f| f.statement.as_str()).collect();
                panic!("no finding said \"{statement}\": {seen:?}");
            })
    }

    /// Both producers enter through the same collection, and only the two
    /// removal columns that actually record a discovery become findings.
    #[test]
    fn finding_collect_reads_both_sources() {
        let (project, spec_dir) = seed(r#"{"stage":"Execute","outcome":"Active"}"#);
        seed_review(&spec_dir);
        seed_ledger(&spec_dir);

        let report = collect(project.path(), spec_dir.to_str().unwrap());
        assert!(report.ok, "{report:?}");
        assert!(report.written, "the sidecar had no findings key yet");
        assert_eq!(report.from_review, 2, "one finding per itemised reviewer finding");
        assert_eq!(report.from_proof_ledger, 2, "an honest red is not a finding");
        assert_eq!(report.open, 4, "nothing has a destination yet");
        assert_eq!(report.stale, 0);
        assert_eq!(report.retained, 0);

        // Sorted by source: the reviewer's two first, then the ledger's. Each id
        // still walks back to its producer through its prefix.
        let sources: Vec<FindingSource> = report.findings.iter().map(|f| f.source).collect();
        assert_eq!(
            sources,
            vec![
                FindingSource::Review,
                FindingSource::Review,
                FindingSource::ProofLedger,
                FindingSource::ProofLedger
            ]
        );
        assert!(
            !report.findings.iter().any(|f| f.id.starts_with("AC-3-")),
            "a criterion whose removal came back red made no discovery: {:?}",
            ids_of(&report)
        );

        // The reviewer's statement is the finding it itemised, with the heading
        // and the bullet marker gone — and its id still names the file it came
        // from, before the fingerprint of what it says.
        let reviewer = by_statement(&report, "the close gate never reads this file");
        assert_eq!(reviewer.source, FindingSource::Review);
        assert!(reviewer.id.starts_with("F-findings-"), "{}", reviewer.id);
        assert_eq!(
            by_statement(&report, "the ledger's third column has no consumer").id,
            finding_id("F-findings-apps-rt", "the ledger's third column has no consumer"),
            "a file this reader cannot itemise still yields its own one record"
        );
        // The ledger's reason is carried in full, never paraphrased.
        let survivor = by_prefix(&report, "AC-1-");
        assert_eq!(survivor.source, FindingSource::ProofLedger);
        assert_eq!(survivor.statement, one_line(SURVIVED_REASON));

        // And it landed in the sidecar, which is the only durable half.
        let meta = read_meta(&spec_dir.join(META_JSON)).expect("reads");
        assert_eq!(meta.findings.len(), 4);
        assert!(meta.findings.iter().all(FindingItem::is_open));
    }

    /// Declare a destination for the one finding that says `statement`, the way
    /// the `mark-finding` door does — by id, straight into the sidecar.
    fn route(spec_dir: &Path, statement: &str, route: FindingRoute) {
        let meta_path = spec_dir.join(META_JSON);
        let mut meta = read_meta(&meta_path).expect("reads");
        let idx = meta
            .findings
            .iter()
            .position(|f| f.statement == statement)
            .unwrap_or_else(|| panic!("nothing said \"{statement}\""));
        meta.findings[idx].routed = Some(route);
        write_meta(&meta_path, &meta).unwrap();
    }

    /// A destination already declared survives every later collection, an
    /// UNDECIDED finding whose source disappeared is dropped, and a new source
    /// enters open.
    #[test]
    fn finding_collect_preserves_declared_route() {
        let (project, spec_dir) = seed(r#"{"stage":"Execute","outcome":"Active"}"#);
        seed_review(&spec_dir);
        seed_ledger(&spec_dir);
        let spec_arg = spec_dir.to_str().unwrap().to_string();

        let _ = collect(project.path(), &spec_arg);

        // Somebody decides what happens to the survivor.
        route(
            &spec_dir,
            &one_line(SURVIVED_REASON),
            FindingRoute::ChangeRequest("rewrite AC-1 so it asserts the behaviour".to_string()),
        );

        // A reviewer file goes away between the two collections, taking a
        // finding nobody had decided anything about.
        std::fs::remove_file(spec_dir.join(REVIEW_DIR).join("findings-apps-rt.md")).unwrap();

        let report = collect(project.path(), &spec_arg);
        assert!(report.ok, "{report:?}");
        assert_eq!(report.stale, 1, "an undecided finding whose source is gone is dropped");
        assert_eq!(report.retained, 0);
        assert_eq!(report.from_review, 1);

        let routed = by_statement(&report, &one_line(SURVIVED_REASON));
        assert!(routed.id.starts_with("AC-1-"), "{}", routed.id);
        assert_eq!(
            routed.route().and_then(FindingRoute::reason),
            Some("rewrite AC-1 so it asserts the behaviour"),
            "a declared destination is the one thing a re-collection must not lose"
        );
        assert!(!routed.is_open(), "a routed finding owes nobody a decision");
        assert_eq!(report.open, 2, "the reviewer's file and AC-2 are still open");
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.statement == "the ledger's third column has no consumer"),
            "an undecided finding whose source is gone is dropped, not kept: {:?}",
            ids_of(&report)
        );

        // Idempotent: a third collection over an unchanged tree writes nothing.
        let again = collect(project.path(), &spec_arg);
        assert!(!again.written, "an unchanged collection must not rewrite the sidecar");
        assert_eq!(again.findings, report.findings);
    }

    /// The reviewer's real scenario: `review_result` OVERWRITES
    /// `review/findings.md` on every round, so round two's discovery arrives
    /// under the same file — and, on the ledger side, the same criterion id can
    /// come back under a different removal column. Neither may inherit the
    /// destination declared for round one: a decision belongs to the discovery
    /// somebody actually read.
    #[test]
    fn finding_collect_mints_a_new_finding_for_a_new_discovery_under_the_same_source() {
        let (project, spec_dir) = seed(r#"{"stage":"Execute","outcome":"Active"}"#);
        let review = spec_dir.join(REVIEW_DIR);
        std::fs::create_dir_all(&review).unwrap();
        std::fs::write(review.join("findings.md"), "# Review\n\n- MINOR — round one\n").unwrap();
        std::fs::write(
            spec_dir.join(AC_PROOF_JSON),
            r#"{"spec":"demo","criteria":[{"id":"AC-1","command":"cd .","verdict":"unproven",
                "proof":"red","removal":"survived","reason":"the criterion survived the removal"}]}"#,
        )
        .unwrap();
        let spec_arg = spec_dir.to_str().unwrap().to_string();

        let first = collect(project.path(), &spec_arg);
        assert_eq!(first.open, 2, "{:?}", ids_of(&first));
        route(
            &spec_dir,
            "MINOR — round one",
            FindingRoute::Dropped("cosmetic, not worth a wave".to_string()),
        );
        route(
            &spec_dir,
            "the criterion survived the removal",
            FindingRoute::Dropped("the criterion is being rewritten anyway".to_string()),
        );
        assert_eq!(collect(project.path(), &spec_arg).open, 0, "both were decided");

        // Round two: the same file, a different discovery — and the same
        // criterion id under the OTHER removal column.
        std::fs::write(
            review.join("findings.md"),
            "# Review\n\n- MINOR — ROUND TWO: the collector inherits a stale decision\n",
        )
        .unwrap();
        std::fs::write(
            spec_dir.join(AC_PROOF_JSON),
            r#"{"spec":"demo","criteria":[{"id":"AC-1","command":"cd .","verdict":"proven",
                "proof":"red","removal":"evidence-removed",
                "reason":"the strip took this criterion's own evidence with it"}]}"#,
        )
        .unwrap();

        let second = collect(project.path(), &spec_arg);
        assert_eq!(
            second.open, 2,
            "a discovery nobody has read is OPEN, whatever the file it arrived in: {:?}",
            ids_of(&second)
        );
        let fresh_review =
            by_statement(&second, "MINOR — ROUND TWO: the collector inherits a stale decision");
        assert!(fresh_review.is_open(), "{fresh_review:?}");
        assert_ne!(
            fresh_review.id,
            finding_id("F-findings", "MINOR — round one"),
            "a new discovery under the same file must not reuse the old id"
        );
        let fresh_column =
            by_statement(&second, "the strip took this criterion's own evidence with it");
        assert!(
            fresh_column.is_open() && fresh_column.id.starts_with("AC-1-"),
            "a criterion found out for something else is a new finding: {fresh_column:?}"
        );
        assert_ne!(
            fresh_column.id,
            by_statement(&second, "the criterion survived the removal").id,
            "`survived` and `evidence-removed` say different things about AC-1"
        );
    }

    /// A reviewer file carrying six findings becomes six records. One `--to` may
    /// never settle five discoveries that never reached the sidecar.
    #[test]
    fn finding_collect_splits_one_file_into_one_record_per_finding() {
        let (project, spec_dir) = seed(r#"{"stage":"Execute","outcome":"Active"}"#);
        let review = spec_dir.join(REVIEW_DIR);
        std::fs::create_dir_all(&review).unwrap();
        std::fs::write(
            review.join("findings.md"),
            "# Review — apps/rt\n\
             \n\
             ## Verdict: APPROVED — 0 blocking findings\n\
             \n\
             Guards all held; the four registrations exist.\n\
             \n\
             ## Acceptance Criteria — each run, each green\n\
             \n\
             - AC-1 `cargo test -p mustard-core finding_item` — 5 passed\n\
             - AC-7 `cargo test --workspace` — 4757 passed\n\
             \n\
             ## MAJOR — the gate is not reached on the documented close path\n\
             \n\
             `close-orchestrate` calls `complete_spec::finalize` directly.\n\
             \n\
             ```\n\
             ## NOT A HEADING — this is a transcript\n\
             - not a finding either\n\
             ```\n\
             \n\
             ## MAJOR — reviewer-side granularity\n\
             \n\
             One finding per findings FILE, so one `--to` settles them all.\n\
             \n\
             ## CRITICAL — the gate is silently defeated on the retry path\n\
             \n\
             Round two inherits round one's decision.\n\
             \n\
             - and the sidecar keeps no trace of the discovery it swallowed\n\
             \n\
             ## MINOR\n\
             \n\
             `write_meta` still injects four null keys into a two-key sidecar.\n\
             \n\
             ## Findings (non-blocking)\n\
             \n\
             - the arg shape diverges from the sibling command\n\
             - the gate writes to disk while it reads\n\
             \n\
             ## Change requests\n\
             \n\
             - minor — the whitelist justification is already stale\n",
        )
        .unwrap();
        let spec_arg = spec_dir.to_str().unwrap().to_string();

        let report = collect(project.path(), &spec_arg);
        assert_eq!(
            report.from_review, 8,
            "one record per itemised finding: {:?}",
            report.findings.iter().map(|f| f.statement.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(report.open, 8);
        for statement in [
            "MAJOR — the gate is not reached on the documented close path",
            "MAJOR — reviewer-side granularity",
            // A heading that carries a sentence stays a finding even when a
            // bullet under it is one too.
            "CRITICAL — the gate is silently defeated on the retry path",
            "and the sidecar keeps no trace of the discovery it swallowed",
            "MINOR — `write_meta` still injects four null keys into a two-key sidecar.",
            "the arg shape diverges from the sibling command",
            "the gate writes to disk while it reads",
            "minor — the whitelist justification is already stale",
        ] {
            let finding = by_statement(&report, statement);
            assert!(finding.id.starts_with("F-findings-"), "{}", finding.id);
        }
        // A verdict line, an AC roll-call and a fenced transcript are not
        // discoveries — reading them as findings would teach the reader to route
        // around the gate.
        assert!(
            !report.findings.iter().any(|f| f.statement.contains("AC-1 `cargo test")
                || f.statement.contains("APPROVED")
                || f.statement.contains("transcript")),
            "{:?}",
            report.findings.iter().map(|f| f.statement.as_str()).collect::<Vec<_>>()
        );

        // Settling one leaves the other five exactly where they were.
        route(
            &spec_dir,
            "MAJOR — reviewer-side granularity",
            FindingRoute::QueuedWork("queued as its own unit".to_string()),
        );
        let after = collect(project.path(), &spec_arg);
        assert_eq!(after.open, 7, "one destination settles ONE finding");
    }

    /// A producer that stops reporting must not take a DECIDED finding with it:
    /// both producers rewrite their file in place, so "gone this round" is
    /// routinely just a round that overwrote the last one — and the decision is
    /// the half no machine can reproduce.
    #[test]
    fn finding_collect_keeps_the_decision_when_the_source_stops_reporting() {
        let (project, spec_dir) = seed(r#"{"stage":"Execute","outcome":"Active"}"#);
        let review = spec_dir.join(REVIEW_DIR);
        std::fs::create_dir_all(&review).unwrap();
        std::fs::write(review.join("findings.md"), "- MINOR — the door writes to disk\n").unwrap();
        let spec_arg = spec_dir.to_str().unwrap().to_string();

        let _ = collect(project.path(), &spec_arg);
        route(
            &spec_dir,
            "MINOR — the door writes to disk",
            FindingRoute::Dropped("documented, atomic, and error-swallowed".to_string()),
        );
        let decided = by_statement(
            &collect(project.path(), &spec_arg),
            "MINOR — the door writes to disk",
        )
        .id
        .clone();

        // The reviewer's next round says nothing about it.
        std::fs::write(review.join("findings.md"), "- MINOR — an entirely different thing\n")
            .unwrap();

        let report = collect(project.path(), &spec_arg);
        assert_eq!(report.stale, 0, "a decided finding is never dropped");
        assert_eq!(report.retained, 1, "{:?}", ids_of(&report));
        let kept = by_statement(&report, "MINOR — the door writes to disk");
        assert_eq!(kept.id, decided);
        assert_eq!(
            kept.route().and_then(FindingRoute::reason),
            Some("documented, atomic, and error-swallowed"),
            "the decision outlives the round that produced the finding"
        );

        // And when the source comes back saying the same thing, the decision is
        // still there — one record, not two.
        std::fs::write(review.join("findings.md"), "- MINOR — the door writes to disk\n").unwrap();
        let back = collect(project.path(), &spec_arg);
        assert_eq!(back.retained, 0, "the source reports it again, so nothing is held back");
        assert_eq!(back.stale, 1, "the round-two finding nobody decided IS dropped");
        assert_eq!(
            back.findings.iter().filter(|f| f.id == decided).count(),
            1,
            "a returning source rejoins its own record: {:?}",
            ids_of(&back)
        );
        assert!(!by_statement(&back, "MINOR — the door writes to disk").is_open());
    }

    /// A spec with neither producer collects zero AND leaves the sidecar
    /// byte-identical — the `findings` key must not appear where nothing was
    /// found.
    #[test]
    fn finding_collect_without_sources_writes_no_key() {
        let (project, spec_dir) = seed(r#"{"stage":"Execute","outcome":"Active"}"#);
        let before = std::fs::read_to_string(spec_dir.join(META_JSON)).unwrap();

        let report = collect(project.path(), spec_dir.to_str().unwrap());
        assert!(report.ok, "no producer on disk is not a failure: {report:?}");
        assert_eq!(report.from_review, 0);
        assert_eq!(report.from_proof_ledger, 0);
        assert!(report.findings.is_empty());
        assert!(!report.written, "nothing to write");

        let after = std::fs::read_to_string(spec_dir.join(META_JSON)).unwrap();
        assert_eq!(before, after, "the sidecar was not touched at all");
        assert!(!after.contains("findings"), "{after}");
    }

    /// The statement reader skips decoration and never cuts a multi-byte
    /// grapheme in half.
    #[test]
    fn finding_collect_statement_skips_decoration_and_bounds_by_chars() {
        assert_eq!(
            statement_of("# Heading\n\n<!-- marker -->\n---\n- [ ] the real line\n").as_deref(),
            Some("the real line")
        );
        assert_eq!(statement_of("\n\n# only a heading\n").as_deref(), None);

        let long = "á".repeat(STATEMENT_MAX_CHARS + 10);
        let bounded = bound(&long);
        assert_eq!(bounded.chars().count(), STATEMENT_MAX_CHARS + 1, "capped plus the ellipsis");
        assert!(bounded.ends_with('…'));
    }
}
