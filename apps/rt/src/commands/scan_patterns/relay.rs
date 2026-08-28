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
//! ## The three channels
//!
//! The envelope arrives through [`super::read_envelope`]: `-` reads stdin,
//! `@<path>` reads that file, anything else is the literal text. The file face
//! exists because the harness PERSISTS a return that exceeds its inline limit
//! and hands the orchestrator only a path — measured at 3 of 10 dispatches on a
//! mid-sized workspace, so it is the ordinary case rather than an edge one.
//!
//! Fail-open per the `mustard-rt run` contract — but fail-open is not the same
//! as fail-SILENT. Two ways of reading nothing used to print the same
//! `ok:true, blocks:0` as an agent that genuinely returned nothing: a `@<path>`
//! that could not be read at all, and a file that WAS read and demarcated no
//! block. Both now land in `skipped`, so `ok:false` names them — and the second
//! covers the whole FILE channel, not only the files that parse as a persisted
//! return, because a readable file that is neither JSON nor demarcated is the
//! same "could not interpret it" wearing the same silent report.
//! A blockless LITERAL envelope (or stdin) keeps the empty, `ok:true` report it
//! always had: there is no file there to have failed.

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
#[derive(Serialize, Debug)]
struct Rejected {
    path: String,
    defects: Vec<String>,
}

/// What the relay did, as one byte-stable JSON document.
#[derive(Serialize, Default, Debug)]
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
    let report = relay(root, content);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{\"ok\":false}".into())
    );
}

/// The testable core of [`run`]: resolve the envelope, apply every block and
/// fold the verdicts into the report — pure of stdout, so a caller (and a test)
/// can read the verdict instead of re-parsing printed JSON. Mirrors the split
/// [`apply_one`]/[`Applied`] already uses for ONE block.
fn relay(root: &Path, content: &str) -> Report {
    let resolved = super::read_envelope(content);
    // The read-but-blockless hole below belongs to the FILE channel as a whole,
    // not to the JSON shape: a file that was read and recognised as neither
    // reaches `Envelope::Raw` and printed the same silent `ok:true, blocks:0`.
    // The shape is kept only to say WHICH kind of file it was.
    let from_file = super::file_face(content).is_some();
    let from_json = matches!(resolved, super::Envelope::Json(_));
    let (envelope, unreadable) = match resolved {
        super::Envelope::Raw(text) | super::Envelope::Json(text) => (text, None),
        super::Envelope::Unreadable(e) => (String::new(), Some(e)),
    };
    // An unreadable path already reports itself below; adding the blockless
    // entry too would name one file twice for one event.
    let read_from_file = from_file && unreadable.is_none();
    let blocks = parse(&envelope);
    let mut report = Report { blocks: blocks.len(), ..Report::default() };
    if let Some(e) = unreadable {
        // Fail-open still exits 0, but never as `ok:true, blocks:0` — that
        // reads as "the agent returned nothing to apply" when in truth nothing
        // was READ.
        report.skipped.push(Rejected { path: super::content_label(content), defects: vec![e] });
    }

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

    // The other door onto the same silence: the file WAS read and demarcated
    // nothing. Left alone that prints `ok:true, blocks:0` — indistinguishable
    // from an agent that authored nothing, which is exactly the report an
    // unreadable path used to print. The door was closed for the JSON shape
    // only, so a readable file that is not a harness-persisted return fell
    // through `Envelope::Raw` and kept printing the silence.
    if read_from_file && report.blocks == 0 {
        let shape = if from_json {
            "a harness-persisted return (valid JSON)"
        } else {
            "plain text (not a harness-persisted return)"
        };
        report.skipped.push(Rejected {
            path: super::content_label(content),
            defects: vec![format!(
                "read as {shape} but it demarcates no `=== FILE:` / `=== DECLINE:` block — \
                 the file WAS read, so this is `nothing could be recovered from it`, not \
                 `the agent returned nothing`"
            )],
        });
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

    report
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mold(slug: &str, glob: &str) -> String {
        format!(
            "---\nname: {slug}-pattern\ndescription: Use when adding or refactoring an X.\npaths:\n  - {glob}\ntags: [add, refactor]\nsource: scan\n---\n\n## Purpose\nbody\n\n## Convention\nbody\n\n## How to apply\nbody\n\n## Examples\nbody\n"
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
            other @ Block::Decline { .. } => panic!("expected a file block, got {other:?}"),
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

    /// AC-1 — the harness persists a return that exceeds its inline limit and
    /// hands the orchestrator only a PATH. `@<path>` must apply those blocks
    /// exactly as if they had arrived on stdin; without it the only route was a
    /// hand-written script, which the `/scan` flow forbids in capitals.
    #[test]
    fn relay_reads_an_envelope_from_a_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = "apps/api/.claude/skills/api-file-pattern/SKILL.md";
        let envelope = format!(
            "Here is my return.\n\n=== FILE: {target} ===\n{}\n=== END ===\n",
            mold("api-file", "apps/api/src/file/**")
        );
        let on_disk = root.join("persisted-return.txt");
        std::fs::write(&on_disk, &envelope).unwrap();

        let report = relay(root, &format!("@{}", on_disk.display()));
        assert!(report.ok, "a readable file is not an error: {report:?}");
        assert_eq!(report.blocks, 1, "the file's block is found: {report:?}");
        assert!(root.join(target).exists(), "the mold lands exactly as it would from stdin");
    }

    /// AC-2 — the persisted file's OWN shape, verified against three real
    /// returns: a pretty-printed array of `{type, text}` whose last element is
    /// the harness's `agentId`/`<usage>` trailer. The envelope is recovered by
    /// harvesting the `text` fields, so no script is needed to unwrap it, and
    /// the trailer (which demarcates nothing) cannot add a block.
    #[test]
    fn relay_reads_the_harness_json_array_of_text_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = "apps/api/.claude/skills/api-json-pattern/SKILL.md";
        let envelope = format!(
            "Grounding complete. Delivering 1 mold.\n\n=== FILE: {target} ===\n{}\n=== END ===\n",
            mold("api-json", "apps/api/src/json/**")
        );
        let persisted = serde_json::json!([
            {"type": "text", "text": envelope},
            {"type": "text", "text": "agentId: a21f1a5dbd1ce6474\n<usage>subagent_tokens: 215294\ntool_uses: 71</usage>"},
        ]);
        let on_disk = root.join("persisted-return.json");
        std::fs::write(&on_disk, serde_json::to_string_pretty(&persisted).unwrap()).unwrap();

        let report = relay(root, &format!("@{}", on_disk.display()));
        assert!(report.ok, "the harness shape is not an error: {report:?}");
        assert_eq!(report.blocks, 1, "the usage trailer adds no block: {report:?}");
        assert!(root.join(target).exists(), "the envelope is recovered from the harness's own shape");
    }

    /// AC-3 — a `@<path>` that cannot be read must NAME the failure. Degrading
    /// to an empty envelope would print `ok:true, blocks:0`, which reads as
    /// "the agent returned nothing to apply" when in truth nothing was READ.
    #[test]
    fn an_unreadable_content_path_is_reported_never_silently_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-return.txt");

        let report = relay(dir.path(), &format!("@{}", missing.display()));
        assert!(!report.ok, "an unread envelope must never print ok:true: {report:?}");
        assert_eq!(report.blocks, 0);
        assert_eq!(report.skipped.len(), 1, "the IO failure is the one entry: {report:?}");
        assert!(
            report.skipped[0].defects[0].contains("no-such-return.txt"),
            "the report names what could not be read: {report:?}"
        );
        // Channel parity: stdin's fail-open behaviour is untouched — only the
        // NEW file channel ever reports an IO failure.
        assert!(
            matches!(super::super::read_envelope("plain literal envelope"), super::super::Envelope::Raw(_)),
            "a literal value is still the envelope itself"
        );
    }

    /// AC-9 — the same silence entering by the other door: the file WAS read
    /// and parsed as a persisted return, yet demarcates nothing. Reporting
    /// `ok:true, blocks:0` there is indistinguishable from an agent that
    /// authored nothing.
    #[test]
    fn a_json_envelope_with_no_blocks_says_so_instead_of_reporting_zero() {
        let dir = tempfile::tempdir().unwrap();
        let on_disk = dir.path().join("persisted-return.json");
        std::fs::write(
            &on_disk,
            r#"[{"type":"text","text":"I could not find anything worth a mold."}]"#,
        )
        .unwrap();

        let report = relay(dir.path(), &format!("@{}", on_disk.display()));
        assert!(!report.ok, "a read-but-blockless return is not a silent ok:true: {report:?}");
        assert_eq!(report.blocks, 0);
        assert!(
            report.skipped.iter().any(|s| s.defects.iter().any(|d| d.contains("demarcates no"))),
            "the report says the file was READ and carried no block: {report:?}"
        );

        // The control the AC names: a blockless LITERAL envelope keeps its
        // existing empty, ok:true report — this must not become a new failure
        // mode. There is no file there to have failed.
        let raw = relay(dir.path(), "I could not find anything worth a mold.");
        assert!(raw.ok && raw.skipped.is_empty(), "raw prose behaviour is unchanged: {raw:?}");
    }

    /// The same silence entering by the LAST door: a file that was read, is not
    /// a harness-persisted return, and demarcates nothing lands in
    /// `Envelope::Raw` — so the JSON-only guard above never saw it and it went
    /// on printing `ok:true, blocks:0`. That reads as "the agent returned
    /// nothing" when what happened is "nothing could be recovered from it".
    #[test]
    fn a_read_file_that_demarcates_nothing_is_never_a_silent_ok() {
        let dir = tempfile::tempdir().unwrap();
        let on_disk = dir.path().join("persisted-return.txt");
        std::fs::write(&on_disk, "I could not find anything worth a mold.").unwrap();

        let report = relay(dir.path(), &format!("@{}", on_disk.display()));
        assert!(!report.ok, "a read-but-blockless file is not a silent ok:true: {report:?}");
        assert_eq!(report.blocks, 0);
        assert_eq!(report.skipped.len(), 1, "one file, one entry: {report:?}");
        assert!(
            report.skipped[0].path.contains("persisted-return.txt"),
            "the report names the file: {report:?}"
        );
        assert!(
            report.skipped[0].defects[0].contains("demarcates no"),
            "the report says it was READ and carried no block: {report:?}"
        );

        // An UNREADABLE path still reports exactly once — the IO failure, not
        // the IO failure plus a blockless entry for the same file.
        let missing = dir.path().join("no-such-return.txt");
        let unread = relay(dir.path(), &format!("@{}", missing.display()));
        assert_eq!(unread.skipped.len(), 1, "one event, one entry: {unread:?}");
        assert!(
            unread.skipped[0].defects[0].contains("cannot read"),
            "the IO failure is the reason, not the blocklessness: {unread:?}"
        );
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
