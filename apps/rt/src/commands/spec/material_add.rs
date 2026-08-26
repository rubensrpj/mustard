//! `mustard-rt run material-add` — record ONE piece of conversation material,
//! at the moment it is settled.
//!
//! ## Why this exists
//!
//! `spec-draft --material <FILE>` reads a finished document. Nothing wrote one.
//! The model was expected to assemble the whole thing by hand at draft time,
//! from memory of a conversation that may already have been compacted away.
//!
//! Measured on this repository, 2026-08-26: two units shipped across two days
//! of conversation, and NEITHER carried a material file. Every definition,
//! decision and reason existed only in the session window. The channel was
//! built, documented and never fed.
//!
//! ## The shape of the fix
//!
//! One call, one item, appended where the draft already looks for it. A
//! decision the conversation settles is written down WHEN it is settled, not
//! reconstructed later — which is the only moment the reason is still known.
//!
//! ## Three kinds, and why they stay apart
//!
//! A definition is a term and its local meaning. A decision is a choice plus
//! the reason it was taken. A finding is a claim plus the file it was checked
//! against. `spec-draft` lands each in a section of its own, and its loader
//! REFUSES an entry missing the half that makes it usable — so this door
//! refuses the same halves, at the moment the operator can still supply them.
//!
//! ## Idempotent
//!
//! An item already present is not appended twice. A conversation revisits the
//! same decision, and a material file that accumulates duplicates is one nobody
//! reads to the end.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file the draft reads, inside the unit's own spec directory.
pub(crate) const MATERIAL_FILE: &str = "spec-material.json";

/// One term and what it means in THIS spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Definition {
    term: String,
    meaning: String,
}

/// One decision and the reason it was taken.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Decision {
    decision: String,
    reason: String,
}

/// One verified statement plus the file that makes it checkable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Finding {
    statement: String,
    file: String,
    /// The line, when the claim is line-precise.
    ///
    /// `u32`, matching `spec_draft`'s own `Finding` exactly. It was `u64` for
    /// one commit, and review measured the consequence: `--line 4294967296`
    /// wrote a file the reader REFUSES, and its loader is deliberately
    /// fail-closed, so the unit's whole material channel died until someone
    /// hand-edited JSON. Two spellings of one contract is how a writer lands
    /// material where no reader looks — the type is half of that contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

/// The accumulating document. Field names and shape mirror what
/// `spec-draft --material` deserialises, byte for byte — two spellings of one
/// contract is how a writer lands material where no reader looks.
///
/// `deny_unknown_fields` mirrors the reader: without it a hand-authored key is
/// accepted here and silently stripped on the next write, and the two structs
/// drift with nothing detecting it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Material {
    #[serde(default)]
    definitions: Vec<Definition>,
    #[serde(default)]
    decisions: Vec<Decision>,
    #[serde(default)]
    findings: Vec<Finding>,
}

/// What the caller is recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Definition,
    Decision,
    Finding,
}

impl Kind {
    /// Parse the `--kind` value. Returns `None` for anything else — a closed
    /// vocabulary, because a typo must not open a fourth silent channel.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "definition" => Some(Self::Definition),
            "decision" => Some(Self::Decision),
            "finding" => Some(Self::Finding),
            _ => None,
        }
    }
}

/// Options for `mustard-rt run material-add`.
#[derive(Debug, Clone)]
pub struct MaterialAddOpts {
    /// The spec slug whose material this is.
    pub spec: String,
    /// Which of the three channels.
    pub kind: String,
    /// The term, the decision, or the statement — the first half.
    pub subject: String,
    /// The meaning, the reason, or the file — the half that makes it usable.
    pub detail: String,
    /// A finding's line number, when the claim is line-precise.
    pub line: Option<u32>,
}

/// The JSON report. Deterministic: repo-relative path, counts, no timestamp.
#[derive(Debug, Serialize)]
pub struct MaterialAddReport {
    pub ok: bool,
    pub spec: String,
    /// `true` when the item was appended; `false` when it was already there.
    pub added: bool,
    /// Where the material lives, repo-relative with forward slashes.
    pub path: String,
    pub definitions: usize,
    pub decisions: usize,
    pub findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

impl MaterialAddReport {
    fn refused(spec: &str, error: &str, remedy: &str) -> Self {
        Self {
            ok: false,
            spec: spec.to_string(),
            added: false,
            path: String::new(),
            definitions: 0,
            decisions: 0,
            findings: 0,
            error: Some(error.to_string()),
            remedy: Some(remedy.to_string()),
        }
    }
}

/// Record one item under `root`, and report what the file now holds.
#[must_use]
pub fn add(root: &Path, opts: &MaterialAddOpts) -> MaterialAddReport {
    let Some(kind) = Kind::parse(&opts.kind) else {
        return MaterialAddReport::refused(
            &opts.spec,
            "unknown_kind",
            "--kind takes exactly one of: definition, decision, finding",
        );
    };
    // BOTH halves, always. `spec-draft`'s own loader refuses an entry missing
    // the second one, so accepting it here would only move the refusal to a
    // moment when the operator can no longer supply what is missing.
    let subject = opts.subject.trim();
    let detail = opts.detail.trim();
    if subject.is_empty() || detail.is_empty() {
        return MaterialAddReport::refused(
            &opts.spec,
            "incomplete_entry",
            match kind {
                Kind::Definition => "a definition needs the term AND what it means here",
                Kind::Decision => "a decision needs the choice AND the reason it was taken — \
                                   a decision without its reason is the one thing a later \
                                   reader cannot use",
                Kind::Finding => "a finding needs the statement AND the file it was checked \
                                  against — a statement with no file is an opinion",
            },
        );
    }

    let dir = mustard_core::ClaudePaths::spec_dir_or_unchecked(root, &opts.spec);
    if !dir.is_dir() {
        return MaterialAddReport::refused(
            &opts.spec,
            "unknown_spec",
            "no spec directory of that name — record material against the unit that is open",
        );
    }
    let path = dir.join(MATERIAL_FILE);
    // FAIL-CLOSED on a file that exists and does not parse.
    //
    // The obvious `unwrap_or_default()` was here for one commit, and review
    // measured what it does: three recorded items, a truncated file, one more
    // `material-add` — and the three were GONE, replaced by the new one, with
    // `ok: true` and no warning. That is the very defect this whole command
    // closes ("the material vanishes and nobody is told"), reintroduced by the
    // writer while the reader guards against it.
    //
    // An ABSENT file is different and still starts empty: nothing was lost, and
    // the first item has to land somewhere.
    let mut doc = Material::default();
    if path.is_file() {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return MaterialAddReport::refused(
                &opts.spec,
                "material_unreadable",
                "the material file exists and could not be read — fix its permissions, or \
                 move it aside if it is no longer wanted",
            );
        };
        // An empty file is an empty document, not a broken one: an interrupted
        // create leaves zero bytes, and refusing there would strand the unit
        // over nothing.
        if !raw.trim().is_empty() {
            match serde_json::from_str::<Material>(&raw) {
                Ok(parsed) => doc = parsed,
                Err(e) => {
                    return MaterialAddReport::refused(
                        &opts.spec,
                        "material_corrupt",
                        &format!(
                            "the material file does not parse ({e}) — appending would \
                             DISCARD everything it holds, so nothing was written. Repair the \
                             JSON, or move the file aside to start over"
                        ),
                    );
                }
            }
        }
    }

    let added = match kind {
        Kind::Definition => {
            let item = Definition { term: subject.to_string(), meaning: detail.to_string() };
            push_unique(&mut doc.definitions, item)
        }
        Kind::Decision => {
            let item = Decision { decision: subject.to_string(), reason: detail.to_string() };
            push_unique(&mut doc.decisions, item)
        }
        Kind::Finding => {
            let item = Finding {
                statement: subject.to_string(),
                file: detail.to_string(),
                line: opts.line,
            };
            push_unique(&mut doc.findings, item)
        }
    };

    let body = match serde_json::to_string_pretty(&doc) {
        Ok(b) => format!("{b}\n"),
        Err(e) => {
            return MaterialAddReport::refused(&opts.spec, "serialise_failed", &e.to_string());
        }
    };
    if mustard_core::io::fs::write_atomic(&path, body.as_bytes()).is_err() {
        return MaterialAddReport::refused(
            &opts.spec,
            "write_failed",
            "the material file could not be written — check the spec directory is writable",
        );
    }

    MaterialAddReport {
        ok: true,
        spec: opts.spec.clone(),
        added,
        path: repo_relative(root, &path),
        definitions: doc.definitions.len(),
        decisions: doc.decisions.len(),
        findings: doc.findings.len(),
        error: None,
        remedy: None,
    }
}

/// Append unless an identical entry is already there. Returns whether it grew.
fn push_unique<T: PartialEq>(items: &mut Vec<T>, item: T) -> bool {
    if items.contains(&item) {
        return false;
    }
    items.push(item);
    true
}

/// Repo-relative, forward slashes: one report reads the same on every platform
/// and carries no machine path.
fn repo_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// CLI entry — `mustard-rt run material-add`.
pub fn run(opts: &MaterialAddOpts) {
    let root = PathBuf::from(crate::shared::context::project_dir());
    let report = add(&root, opts);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(i32::from(!report.ok));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed(root: &std::path::Path, slug: &str) {
        std::fs::create_dir_all(root.join(".claude").join("spec").join(slug)).unwrap();
    }

    fn opts(kind: &str, subject: &str, detail: &str) -> MaterialAddOpts {
        MaterialAddOpts {
            spec: "demo".to_string(),
            kind: kind.to_string(),
            subject: subject.to_string(),
            detail: detail.to_string(),
            line: None,
        }
    }

    /// The three channels accumulate, and the file is the exact shape
    /// `spec-draft --material` deserialises.
    ///
    /// Two spellings of one contract is how a writer lands material where no
    /// reader looks — so the round trip is asserted, not the field names.
    #[test]
    fn the_three_channels_accumulate_into_the_shape_the_draft_reads() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "demo");

        assert!(add(dir.path(), &opts("definition", "injetavel", "arquivo colado na janela")).ok);
        assert!(add(dir.path(), &opts("decision", "hooks raros pelo boot", "custo medido")).ok);
        let r = add(dir.path(), &opts("finding", "o bin nasce vazio", "plugin/bin/README.md"));
        assert!(r.ok, "{r:?}");
        assert_eq!((r.definitions, r.decisions, r.findings), (1, 1, 1));

        let raw = std::fs::read_to_string(
            dir.path().join(".claude/spec/demo").join(MATERIAL_FILE),
        )
        .unwrap();
        let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(doc["definitions"][0]["term"], "injetavel");
        assert_eq!(doc["decisions"][0]["reason"], "custo medido");
        assert_eq!(doc["findings"][0]["file"], "plugin/bin/README.md");
        // A finding with no line carries no key at all, rather than a null the
        // reader would have to special-case.
        assert!(doc["findings"][0].get("line").is_none());
    }

    /// BOTH halves, always — the same refusal `spec-draft`'s loader makes, taken
    /// at the moment the operator can still supply what is missing.
    #[test]
    fn an_entry_missing_the_half_that_makes_it_usable_is_refused() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "demo");
        for (kind, subject, detail) in [
            ("decision", "algo", ""),
            ("decision", "", "porque sim"),
            ("definition", "termo", "   "),
            ("finding", "afirmacao", ""),
        ] {
            let r = add(dir.path(), &opts(kind, subject, detail));
            assert!(!r.ok, "{kind} {subject:?}/{detail:?} must refuse");
            assert_eq!(r.error.as_deref(), Some("incomplete_entry"));
            assert!(r.remedy.is_some_and(|m| !m.is_empty()), "a refusal names its remedy");
        }
        // …and nothing was written for any of them.
        assert!(!dir.path().join(".claude/spec/demo").join(MATERIAL_FILE).exists());
    }

    /// The same item twice does not grow the file. A conversation revisits the
    /// same decision, and a document that accumulates duplicates is one nobody
    /// reads to the end.
    #[test]
    fn recording_the_same_item_twice_is_idempotent() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "demo");
        let first = add(dir.path(), &opts("decision", "escolha", "razao"));
        let again = add(dir.path(), &opts("decision", "escolha", "razao"));
        assert!(first.added, "the first one lands");
        assert!(!again.added, "the second one does not");
        assert_eq!(again.decisions, 1);
    }

    /// An unknown kind and an unknown spec are refused by name, never written
    /// into a fourth silent channel.
    #[test]
    fn an_unknown_kind_or_spec_is_refused_by_name() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "demo");
        assert_eq!(
            add(dir.path(), &opts("decisions", "x", "y")).error.as_deref(),
            Some("unknown_kind"),
            "a near-miss plural must not open a channel",
        );
        let mut o = opts("decision", "x", "y");
        o.spec = "nao-existe".to_string();
        assert_eq!(add(dir.path(), &o).error.as_deref(), Some("unknown_spec"));
    }

    /// A material file that exists and does not parse is REFUSED, never
    /// silently replaced.
    ///
    /// Review measured the earlier `unwrap_or_default()`: three recorded items,
    /// a truncated file, one more `material-add` — and the three were gone,
    /// reported as `ok: true`. That is the defect this command exists to close,
    /// reintroduced by its own writer.
    #[test]
    fn a_corrupt_material_file_is_refused_rather_than_discarded() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "demo");
        let path = dir.path().join(".claude/spec/demo").join(MATERIAL_FILE);

        assert!(add(dir.path(), &opts("decision", "primeira", "razao")).ok);
        assert!(add(dir.path(), &opts("decision", "segunda", "razao")).ok);
        let before = std::fs::read_to_string(&path).unwrap();
        let half = before[..before.len() / 2].to_string();

        // Truncated mid-write — the shape an interrupted save leaves.
        std::fs::write(&path, &half).unwrap();
        let r = add(dir.path(), &opts("decision", "terceira", "razao"));
        assert!(!r.ok, "a corrupt file must refuse: {r:?}");
        assert_eq!(r.error.as_deref(), Some("material_corrupt"));
        assert!(
            r.remedy.as_deref().is_some_and(|m| m.contains("DISCARD")),
            "the refusal must say what was at stake: {:?}",
            r.remedy,
        );
        // …and NOTHING was written: the half-file is exactly as it was.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), half);

        // An EMPTY file is not corrupt — an interrupted create leaves zero
        // bytes, and refusing there would strand the unit over nothing.
        std::fs::write(&path, "").unwrap();
        assert!(add(dir.path(), &opts("decision", "quarta", "razao")).ok, "empty is not corrupt");
    }

    /// The `line` type is the reader's, and a value the reader refuses cannot
    /// be written.
    ///
    /// It was `u64` here against the reader's `u32` for one commit. Review
    /// measured it: `--line 4294967296` produced a file the fail-closed loader
    /// permanently refuses, killing the unit's whole material channel.
    #[test]
    fn the_line_width_is_the_one_the_draft_reads() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "demo");
        let mut o = opts("finding", "afirmacao", "src/x.rs");
        o.line = Some(u32::MAX);
        assert!(add(dir.path(), &o).ok, "the widest value the reader takes must land");

        let raw = std::fs::read_to_string(
            dir.path().join(".claude/spec/demo").join(MATERIAL_FILE),
        )
        .unwrap();
        let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let written = doc["findings"][0]["line"].as_u64().unwrap();
        assert!(
            u32::try_from(written).is_ok(),
            "every value this door can write must fit the reader's width: {written}",
        );
    }
}
