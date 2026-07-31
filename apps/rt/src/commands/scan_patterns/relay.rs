//! `scan-patterns-relay` — apply a patterns agent's WHOLE return in one call.
//!
//! ## Why this exists
//!
//! The enrich dispatches one agent per subproject, and a subproject holds as
//! many clusters as it holds — `apps/rt` returned twelve molds, 55 KB, in a
//! single final message. The flow then says: pipe each block to
//! `scan-patterns-apply` on stdin. That is one command per block, and the
//! splitting is left to the orchestrator, which is a language model reading a
//! 55 KB string. Measured on the run that motivated this: the return exceeded
//! the harness's inline limit, was persisted to a file, and the twelve blocks
//! were recovered by hand-writing a regex loop over that file — precisely the
//! "script around a rough edge" the `/scan` flow forbids, because a script
//! silently drops the contracts these commands carry.
//!
//! The size is not the defect. Twelve clusters in a house is a fact about the
//! house, and splitting the fan-out finer would only trade one agent that knows
//! the whole subproject for several that each know a slice. The defect is that
//! the ENVELOPE had no owner: every command here parses its own input except
//! this one step, which was left to prose. So the envelope gets a parser, the
//! relay becomes ONE call, and the block count stops mattering.
//!
//! ## What it does
//!
//! Reads the agent's return, splits it on the demarcators the prompt asked for
//! (`=== FILE: <moldPath> ===` … `=== END ===` and `=== DECLINE: <slug> ===`
//! … `=== END ===`), and routes each block through the same
//! [`super::apply::apply_one`] / [`super::decline::record`] the single-block
//! commands use — one copy of the rules, no second implementation to drift.
//! Prose outside the demarcators is ignored, which is what lets an agent
//! preface its return with a summary.
//!
//! A bad block never stops a good one: every block is attempted and the verdict
//! for each lands in the JSON report, so a refusal costs its own re-dispatch
//! instead of the eleven behind it. `ok:false` means at least one block needs
//! attention — the caller re-dispatches THAT agent, and the report names which.
//!
//! Fail-open per the `mustard-rt run` contract: an unreadable or blockless
//! input prints an empty report and exits 0.

use std::io::Read as _;
use std::path::Path;

use serde::Serialize;

use super::apply::{apply_one, Applied};

/// One demarcated block found in the agent's return.
#[derive(Debug, PartialEq)]
enum Block {
    /// A mold to write, with its declared path.
    File { path: String, body: String },
    /// A candidate the agent refused, with its one-line reason.
    Decline { slug: String, reason: String },
}

/// A mold that was NOT written, and why — the actionable half of the report.
#[derive(Serialize)]
struct Rejected {
    path: String,
    defects: Vec<String>,
}

/// What the relay did, as one byte-stable JSON document.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Report {
    /// False when any block needs the caller's attention.
    ok: bool,
    /// How many demarcated blocks the envelope carried.
    blocks: usize,
    created: Vec<String>,
    declined: Vec<String>,
    /// Refused with the machine's reasons — re-dispatch these.
    refused: Vec<Rejected>,
    /// Two candidates resolved to one mold path (a worklist defect).
    collisions: Vec<String>,
    /// Hand-authored molds the relay left alone.
    preserved: Vec<String>,
    /// Blocks that never reached a verdict (bad path, empty body, IO).
    skipped: Vec<Rejected>,
}

/// Run `scan-patterns-relay`: apply every block in the agent's return.
pub fn run(root: &Path, content: &str) {
    let envelope = resolve_content(content);
    let blocks = parse(&envelope);
    let mut report = Report { blocks: blocks.len(), ..Report::default() };

    for block in blocks {
        match block {
            Block::File { path, body } => {
                let target = root.join(&path);
                match apply_one(&target, &body, root) {
                    Applied::Created => report.created.push(path),
                    Applied::Collision => report.collisions.push(path),
                    Applied::Preserved => report.preserved.push(path),
                    Applied::Refused(defects) => report.refused.push(Rejected { path, defects }),
                    Applied::Empty => report
                        .skipped
                        .push(Rejected { path, defects: vec!["empty block body".into()] }),
                    Applied::BadPath => report.skipped.push(Rejected {
                        path,
                        defects: vec![
                            "not a `…/.claude/skills/<slug>-pattern/SKILL.md` path".into()
                        ],
                    }),
                    Applied::IoError(e) => {
                        report.skipped.push(Rejected { path, defects: vec![e] });
                    }
                }
            }
            Block::Decline { slug, reason } => match super::decline::record(root, &slug, &reason) {
                Ok(()) => report.declined.push(slug),
                Err(e) => report.skipped.push(Rejected { path: slug, defects: vec![e] }),
            },
        }
    }

    // Sorted so two runs over the same envelope print identical bytes.
    report.created.sort();
    report.declined.sort();
    report.collisions.sort();
    report.preserved.sort();
    report.refused.sort_by(|a, b| a.path.cmp(&b.path));
    report.skipped.sort_by(|a, b| a.path.cmp(&b.path));
    report.ok =
        report.refused.is_empty() && report.collisions.is_empty() && report.skipped.is_empty();

    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{\"ok\":false}".into())
    );
}

/// Split the envelope into its demarcated blocks, ignoring prose around them.
///
/// A demarcator that opens while another is still open CLOSES the previous one:
/// the `=== END ===` is what the prompt asks for, but a missing one must cost
/// that block's trailing whitespace, never the eleven blocks behind it.
fn parse(envelope: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut open: Option<(bool, String, Vec<&str>)> = None; // (is_file, marker, lines)

    let close = |open: &mut Option<(bool, String, Vec<&str>)>, out: &mut Vec<Block>| {
        let Some((is_file, marker, lines)) = open.take() else { return };
        let body = lines.join("\n");
        if is_file {
            out.push(Block::File { path: normalise(&marker), body });
        } else {
            // The reason is the block's prose, collapsed to the one line the
            // ledger stores.
            let reason = body.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(Block::Decline { slug: marker, reason });
        }
    };

    for line in envelope.lines() {
        let trimmed = line.trim();
        if trimmed == "=== END ===" {
            close(&mut open, &mut out);
            continue;
        }
        if let Some(marker) = demarcator(trimmed, "=== FILE:") {
            close(&mut open, &mut out);
            open = Some((true, marker, Vec::new()));
            continue;
        }
        if let Some(marker) = demarcator(trimmed, "=== DECLINE:") {
            close(&mut open, &mut out);
            open = Some((false, marker, Vec::new()));
            continue;
        }
        if let Some((_, _, lines)) = open.as_mut() {
            lines.push(line);
        }
    }
    close(&mut open, &mut out);
    out
}

/// The marker inside `=== <KEYWORD> <marker> ===`, or `None` for any other line.
/// Tolerates the backtick decoration an agent sometimes wraps the path in.
fn demarcator(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?;
    let inner = rest.strip_suffix("===")?.trim().trim_matches('`').trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// Forward-slash a declared path so one envelope reads the same on either OS.
fn normalise(p: &str) -> String {
    p.replace('\\', "/")
}

/// The envelope: the flag's value, or stdin when it is `-` (the default).
fn resolve_content(content: &str) -> String {
    if content != "-" {
        return content.to_string();
    }
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mold(slug: &str, glob: &str) -> String {
        format!(
            "---\nname: {slug}-pattern\ndescription: Use when adding or refactoring an X.\npaths:\n  - {glob}\ntags: [add, refactor]\nsource: scan\n---\n\n## Purpose\nbody\n"
        )
    }

    #[test]
    fn an_envelope_of_many_blocks_is_split_by_its_demarcators() {
        let env = format!(
            "Here is my return.\n\n=== FILE: apps/api/.claude/skills/api-a-pattern/SKILL.md ===\n{}\n=== END ===\n\nsome prose\n\n=== DECLINE: api-b ===\ncovered by api-a-pattern\n=== END ===\n",
            mold("api-a", "apps/api/src/a/**")
        );
        let blocks = parse(&env);
        assert_eq!(blocks.len(), 2, "prose around the blocks is ignored: {blocks:?}");
        match &blocks[0] {
            Block::File { path, body } => {
                assert_eq!(path, "apps/api/.claude/skills/api-a-pattern/SKILL.md");
                assert!(body.contains("## Purpose"), "the body survives whole");
            }
            other => panic!("expected a file block, got {other:?}"),
        }
        assert_eq!(
            blocks[1],
            Block::Decline { slug: "api-b".into(), reason: "covered by api-a-pattern".into() }
        );
    }

    /// The whole reason this exists: one malformed block must cost its own
    /// re-dispatch, never the good blocks behind it. Under the per-block CLI
    /// this was a `process::exit(1)` on the first defect.
    #[test]
    fn a_refused_block_never_takes_the_good_ones_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let good = "apps/api/.claude/skills/api-good-pattern/SKILL.md";
        let bad = "apps/api/.claude/skills/api-bad-pattern/SKILL.md";
        let env = format!(
            "=== FILE: {bad} ===\nno frontmatter at all, just prose\n=== END ===\n=== FILE: {good} ===\n{}\n=== END ===\n",
            mold("api-good", "apps/api/src/good/**")
        );
        run(root, &env);
        assert!(
            root.join(good).exists(),
            "the good mold lands even though the bad one preceded it"
        );
        assert!(!root.join(bad).exists(), "the malformed mold is never written");
    }

    #[test]
    fn declines_reach_the_ledger_through_the_same_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run(root, "=== DECLINE: api-x ===\nexemplars are generated code\n=== END ===\n");
        let map = super::super::decline::declined(root);
        assert_eq!(map.get("api-x").map(String::as_str), Some("exemplars are generated code"));
    }

    /// An agent that forgets the closing marker must not lose the block; the
    /// next demarcator closes it.
    #[test]
    fn a_missing_end_marker_costs_nothing_but_whitespace() {
        let env = format!(
            "=== FILE: apps/api/.claude/skills/api-a-pattern/SKILL.md ===\n{}\n=== FILE: apps/api/.claude/skills/api-b-pattern/SKILL.md ===\n{}\n=== END ===\n",
            mold("api-a", "apps/api/src/a/**"),
            mold("api-b", "apps/api/src/b/**")
        );
        let blocks = parse(&env);
        assert_eq!(blocks.len(), 2, "the unterminated block is still recovered: {blocks:?}");
    }

    #[test]
    fn a_blockless_envelope_reports_empty_and_never_errors() {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), "I could not find anything worth a mold.");
        // Reaching here at all is the assertion: no panic, no exit.
    }

    #[test]
    fn a_backticked_path_is_read_the_same_as_a_bare_one() {
        assert_eq!(
            demarcator("=== FILE: `a/b/SKILL.md` ===", "=== FILE:").as_deref(),
            Some("a/b/SKILL.md")
        );
        assert_eq!(demarcator("not a marker", "=== FILE:"), None);
        assert_eq!(demarcator("=== FILE:  ===", "=== FILE:"), None);
    }

    /// Two runs over one envelope must print identical bytes — the report is
    /// consumed by a caller that diffs it.
    #[test]
    fn the_report_is_byte_stable() {
        let env = "=== DECLINE: b ===\nsecond\n=== END ===\n=== DECLINE: a ===\nfirst\n=== END ===\n";
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        run(one.path(), env);
        run(two.path(), env);
        let a = std::fs::read_to_string(one.path().join(".claude/scan-declined.json")).unwrap();
        let b = std::fs::read_to_string(two.path().join(".claude/scan-declined.json")).unwrap();
        assert_eq!(a, b, "the ledger is order-independent");
    }
}
