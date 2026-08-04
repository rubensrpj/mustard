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
//! # What seeding does and does not touch
//!
//! [`collect`] reconciles rather than overwrites. A finding whose destination
//! was already declared keeps it across every later collection — that decision
//! is the one thing on the record no machine can reproduce. A finding that is no
//! longer in either source is dropped, and a new one enters with no destination
//! at all, which is the OPEN position [`FindingItem::is_open`] reports.
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

/// The spec-relative directory the reviewer writes its findings into — the same
/// one `review_result` creates when it persists them.
const REVIEW_DIR: &str = "review";

/// The sidecar the findings are seeded into.
const META_JSON: &str = "meta.json";

/// The prefix every reviewer-side finding id carries. Its tail is the findings
/// file's own stem, so `F-findings-apps-rt` names `review/findings-apps-rt.md`
/// and a reader walks back from the id to the file without a lookup table.
const REVIEW_ID_PREFIX: &str = "F-";

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
    /// How many findings the sidecar carried that neither source still
    /// produces, and which this collection therefore dropped.
    pub stale: usize,
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
/// then sort it.
///
/// This is the whole idempotency contract: the destination is the one thing on
/// the record a machine cannot reproduce, so a re-collection preserves it
/// VERBATIM — including a route written with a blank reason, which
/// [`FindingItem::route`] keeps reading as no route at all. The statement is
/// taken FRESH from the source, because the file on disk is what the finding
/// says today.
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
    out.sort_by(|a, b| {
        source_rank(a.source)
            .cmp(&source_rank(b.source))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

/// `true` for a file the reviewer wrote: the spec-wide `findings.md`, or a
/// subproject-scoped `findings-<slug>.md`. Keyed off the WRITER's own constants
/// so the two sides cannot drift into two spellings of the same file.
fn is_findings_file(name: &str) -> bool {
    name == FINDINGS_FILE || (name.starts_with(FINDINGS_SCOPED_PREFIX) && name.ends_with(".md"))
}

/// One finding per reviewer file, sorted by file name. An absent `review/`
/// directory yields none — the reviewer simply has not spoken yet.
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
        .map(|entry| {
            let body = fs::read_to_string(&entry.path).unwrap_or_default();
            let stem = entry.file_name.strip_suffix(".md").unwrap_or(&entry.file_name);
            FindingItem {
                id: format!("{REVIEW_ID_PREFIX}{stem}"),
                source: FindingSource::Review,
                // A file with nothing quotable is named by its own file name
                // rather than described: authoring a sentence here would put
                // this collector's words where the reviewer's belong.
                statement: statement_of(&body).unwrap_or_else(|| entry.file_name.clone()),
                routed: None,
            }
        })
        .collect()
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
        .filter(|criterion| {
            matches!(criterion.removal, Removal::Survived | Removal::EvidenceRemoved)
        })
        .map(|criterion| FindingItem {
            id: criterion.id.clone(),
            source: FindingSource::ProofLedger,
            // A ledger that recorded the column without a reason (a later pass
            // owns that field) still identifies the criterion by the command it
            // ran — a fact, never an invented sentence.
            statement: one_line(
                criterion
                    .reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or(criterion.command.as_str()),
            ),
            routed: None,
        })
        .collect()
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
        assert_eq!(report.from_review, 2, "one finding per reviewer file");
        assert_eq!(report.from_proof_ledger, 2, "an honest red is not a finding");
        assert_eq!(report.open, 4, "nothing has a destination yet");
        assert_eq!(report.stale, 0);

        // Sorted by (source, id): the reviewer's two first, then the ledger's.
        let ids: Vec<&str> = report.findings.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["F-findings", "F-findings-apps-rt", "AC-1", "AC-2"]);
        assert!(
            !ids.contains(&"AC-3"),
            "a criterion whose removal came back red made no discovery"
        );

        // The reviewer's statement is its own first quotable line, with the
        // heading and the bullet marker gone.
        assert_eq!(report.findings[0].statement, "the close gate never reads this file");
        assert_eq!(report.findings[0].source, FindingSource::Review);
        // The ledger's reason is carried in full, never paraphrased.
        assert_eq!(report.findings[2].id, "AC-1");
        assert_eq!(report.findings[2].source, FindingSource::ProofLedger);
        assert_eq!(report.findings[2].statement, one_line(SURVIVED_REASON));

        // And it landed in the sidecar, which is the only durable half.
        let meta = read_meta(&spec_dir.join(META_JSON)).expect("reads");
        assert_eq!(meta.findings.len(), 4);
        assert!(meta.findings.iter().all(FindingItem::is_open));
    }

    /// A destination already declared survives every later collection, a source
    /// that disappeared takes its finding with it, and a new source enters open.
    #[test]
    fn finding_collect_preserves_declared_route() {
        let (project, spec_dir) = seed(r#"{"stage":"Execute","outcome":"Active"}"#);
        seed_review(&spec_dir);
        seed_ledger(&spec_dir);
        let spec_arg = spec_dir.to_str().unwrap().to_string();

        let _ = collect(project.path(), &spec_arg);

        // Somebody decides what happens to the survivor.
        let meta_path = spec_dir.join(META_JSON);
        let mut meta = read_meta(&meta_path).expect("reads");
        let idx = meta.findings.iter().position(|f| f.id == "AC-1").unwrap();
        meta.findings[idx].routed = Some(FindingRoute::ChangeRequest(
            "rewrite AC-1 so it asserts the behaviour".to_string(),
        ));
        write_meta(&meta_path, &meta).unwrap();

        // A reviewer file goes away between the two collections.
        std::fs::remove_file(spec_dir.join(REVIEW_DIR).join("findings-apps-rt.md")).unwrap();

        let report = collect(project.path(), &spec_arg);
        assert!(report.ok, "{report:?}");
        assert_eq!(report.stale, 1, "the vanished source took its finding with it");
        assert_eq!(report.from_review, 1);

        let routed = report
            .findings
            .iter()
            .find(|f| f.id == "AC-1")
            .expect("the survivor is still collected");
        assert_eq!(
            routed.route().and_then(FindingRoute::reason),
            Some("rewrite AC-1 so it asserts the behaviour"),
            "a declared destination is the one thing a re-collection must not lose"
        );
        assert!(!routed.is_open(), "a routed finding owes nobody a decision");
        assert_eq!(report.open, 2, "the reviewer's file and AC-2 are still open");
        assert!(
            !report.findings.iter().any(|f| f.id == "F-findings-apps-rt"),
            "a finding whose source is gone is dropped, not kept"
        );

        // Idempotent: a third collection over an unchanged tree writes nothing.
        let again = collect(project.path(), &spec_arg);
        assert!(!again.written, "an unchanged collection must not rewrite the sidecar");
        assert_eq!(again.findings, report.findings);
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
