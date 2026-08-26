//! `mustard-rt run notebook` — the per-branch NOTEBOOK of a work unit.
//!
//! Work surfaces things that are true but off-topic: a defect noticed three
//! files away, a rename the spec never asked for, a question the current
//! acceptance criteria cannot answer. The porta rule is one line — **what
//! belongs to the spec AMENDS the spec, what does not goes to the notebook** —
//! and this module is the second half of it.
//!
//! ## Per branch, never one global list
//!
//! The record lives beside the unit's own state, under
//! `.claude/spec/<slug>/notebook.md`, where `<slug>` is what the work branch
//! names after its `{kind}/` prefix. That is the same directory the spec, the
//! waves and the ceremony are materialized into, so the notebook is part of the
//! unit and disappears with it when the branch does — which is the whole point
//! of the work unit living on its branch.
//!
//! A single global list was the alternative and it is worse: WHICH work produced
//! a pendency is information. It changes the item's priority and it is the only
//! thing that makes the item legible six weeks later. A shared list loses that
//! on the first append and rots into the loose to-do file nobody reads.
//!
//! ## Markdown, because of what happens next
//!
//! Once the pull request opens, the notebook IS the next cycle's prompt: the
//! operator reads it on the base and it becomes the request that crosses the
//! base gate and starts the following unit. A file that is already prose needs
//! no rendering step to play that part, and it stays readable in the pull
//! request diff. One `- ` line per item, appended in order, an exact repeat
//! folded away — so recording the same thing twice does not change the file.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::commands::git_settle::git_out;

/// The checkout the notebook belongs to — the CURRENT working tree, not the
/// main one.
///
/// A unit running in its own worktree keeps its `.claude/spec/` inside that
/// worktree, because the whole unit lives on the branch. Resolving the main
/// checkout here (the way the base-side commands do) would write one unit's
/// notebook into another unit's tree.
fn checkout_root(root: &Path) -> PathBuf {
    git_out(root, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf())
}

/// The `- ` items of a notebook file, in file order. Anything else in the file
/// (the title, the reminder of what the notebook is for) is not an item.
fn parse_items(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Render the whole file from its items. Rewriting rather than appending is what
/// keeps the header honest when the branch is renamed, and it is the shape
/// `write_atomic` wants — a half-appended line would corrupt the one artifact
/// the next cycle reads.
fn render(branch: &str, slug: &str, items: &[String]) -> String {
    let mut out = format!(
        "# Notebook — {branch}\n\n\
         > What surfaced during this work and does NOT belong to `{slug}`. What belongs\n\
         > to the spec amends the spec; this file is everything else, and once the pull\n\
         > request opens it is the next cycle's prompt.\n\n"
    );
    for item in items {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
    out
}

/// One item as the file stores it: a single line. Whitespace is collapsed rather
/// than trusted — a pasted multi-line note would otherwise write lines the
/// parser reads back as several items, or as none.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The notebook pass — the testable core of [`run`]. `unit` names the work
/// branch (omitted: the branch the checkout is standing on) and `add` records
/// one item. Never panics.
#[must_use]
pub(crate) fn notebook_at(
    root: &Path,
    unit: Option<&str>,
    add: Option<&str>,
    explains: bool,
) -> Value {
    let project = checkout_root(root);
    let branch = match unit.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => u.to_string(),
        None => git_out(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default(),
    };
    // A name that is nobody's work unit means no notebook — an integration
    // base, a hand-cut branch, a `feature_x` whose prefix names neither a kind
    // nor a declared base. The notebook is per-unit by construction, so there is
    // nowhere to put the item and saying so is the only honest answer.
    //
    // The reading is the crate's one parser, the same the `/pr` door resolves a
    // head ref with: a loose split on the first `_` would accept ANY prefix, so
    // `--unit feature_x` would silently open the notebook of a spec called `x`.
    let config = mustard_core::ProjectConfig::load(&project);
    // ROOTED: a unit still in the `{base}_{slug}` shape whose base the flow no
    // longer declares is resolved by the branches the repository really has —
    // otherwise its own notebook answers `no-unit`.
    let flow = crate::shared::work_kind::BaseFlow::of_at(&config.git, &project);
    let bases: Vec<String> = flow.bases().to_vec();
    let Some(slug) = flow.slug_of(&branch) else {
        return json!({
            "ok": false,
            "reason": "no-unit",
            "branch": branch,
            "bases": bases,
            "hint": "the notebook is per work unit — run it from a `{kind}/{slug}` branch \
                     (feature/, fix/, hotfix/), or name one with `--unit {kind}/{slug}`",
        });
    };

    let path = mustard_core::ClaudePaths::spec_dir_or_unchecked(&project, &slug).join("notebook.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut items = parse_items(&existing);

    let mut added = false;
    if let Some(text) = add.map(one_line).filter(|t| !t.is_empty()) {
        // The marker rides the line itself — see `EXPLAINS_SYMPTOM`.
        let text = if explains && !explains_symptom(&text) {
            format!("{EXPLAINS_SYMPTOM} {text}")
        } else {
            text
        };
        if !items.iter().any(|i| i == &text) {
            items.push(text);
            added = true;
        }
        if let Err(e) = mustard_core::io::fs::write_atomic(&path, render(&branch, &slug, &items).as_bytes()) {
            return json!({
                "ok": false,
                "reason": "write-failed",
                "branch": branch,
                "slug": slug,
                "hint": e.to_string(),
            });
        }
    }

    json!({
        "ok": true,
        "unit": branch,
        "slug": slug,
        // Relative to the checkout, forward slashes: one report reads the same
        // on every platform and carries no machine path.
        "path": path
            .strip_prefix(&project)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/"),
        "added": added,
        // The items naming the operator's own symptom, called out on their own
        // so a gate does not have to know the prefix.
        "explainsSymptom": items.iter().filter(|i| explains_symptom(i)).cloned().collect::<Vec<_>>(),
        "items": items,
    })
}

/// The prefix an item carries when the operator's own reported symptom is what
/// it explains.
///
/// Written INTO the line rather than into a sidecar, because the notebook is a
/// markdown file a person reads and edits, and a second file would drift from
/// it. Every existing reader keeps working; only the ones that ask about this
/// prefix see anything new.
pub(crate) const EXPLAINS_SYMPTOM: &str = "[EXPLICA O SINTOMA]";

/// Does this notebook line claim to explain the symptom the operator reported?
#[must_use]
pub(crate) fn explains_symptom(item: &str) -> bool {
    item.trim_start().starts_with(EXPLAINS_SYMPTOM)
}

/// Run `notebook` from `root` and print the JSON report.
pub fn run(root: &Path, unit: Option<&str>, add: Option<&str>, explains: bool) {
    println!(
        "{}",
        serde_json::to_string_pretty(&notebook_at(root, unit, add, explains))
            .unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A repo sitting on the work branch `dev_my-unit`.
    fn repo() -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);
        git(root, &["checkout", "-b", "dev_my-unit"]);
        dir
    }

    /// AC-7 — an out-of-scope item recorded during a work unit lands in THAT
    /// unit's notebook and reads back when the unit is named.
    #[test]
    fn notebook_records_an_item_and_reads_it_back_by_unit() {
        let dir = repo();
        let root = dir.path();

        // Recorded from inside the unit — the branch names the notebook.
        let wrote = notebook_at(root, None, Some("the statusline truncates on narrow terminals"), false);
        assert_eq!(wrote["ok"], json!(true), "report: {wrote}");
        assert_eq!(wrote["unit"], json!("dev_my-unit"));
        assert_eq!(wrote["slug"], json!("my-unit"));
        assert_eq!(wrote["added"], json!(true));
        assert_eq!(
            wrote["path"],
            json!(".claude/spec/my-unit/notebook.md"),
            "the notebook lives with the unit, forward slashes on every platform"
        );

        // Read back BY UNIT rather than by the branch that happens to be out.
        // The checkout moves to `dev` on purpose: the notebook is addressed by
        // the unit NAME, not by where HEAD stands, which is what makes
        // `--unit` a real handle instead of a decoration. (In this tree the
        // file is still untracked, so it survives the switch — on a real unit
        // the notebook rides its own branch, as the module doc says.)
        git(root, &["checkout", "dev"]);
        let read = notebook_at(root, Some("dev_my-unit"), None, false);
        assert_eq!(read["ok"], json!(true));
        assert_eq!(read["added"], json!(false), "reading records nothing");
        assert_eq!(
            read["items"],
            json!(["the statusline truncates on narrow terminals"]),
            "the item reads back under its own unit"
        );

        // A DIFFERENT unit has its own notebook — the record is per branch, not
        // one global list.
        let other = notebook_at(root, Some("dev_another-unit"), None, false);
        assert_eq!(other["items"], json!([]), "one unit's item never leaks into another's");
    }

    /// The file stays stable under repetition, and a pasted multi-line note is
    /// stored as ONE item rather than as lines the parser would re-read.
    #[test]
    fn notebook_folds_an_exact_repeat_and_keeps_one_item_per_line() {
        let dir = repo();
        let root = dir.path();

        let first = notebook_at(root, None, Some("rename the digest cache"), false);
        assert_eq!(first["added"], json!(true));
        let again = notebook_at(root, None, Some("rename the digest cache"), false);
        assert_eq!(again["added"], json!(false), "an exact repeat is folded away");
        assert_eq!(again["items"], json!(["rename the digest cache"]));

        let pasted = notebook_at(root, None, Some("two lines\nbecome  one"), false);
        assert_eq!(
            pasted["items"],
            json!(["rename the digest cache", "two lines become one"]),
            "whitespace is collapsed so one note is one item"
        );
    }

    /// No work unit, nowhere to put the item: an integration base and a hand-cut
    /// branch are both refused, and the refusal says how to name a unit.
    #[test]
    fn notebook_refuses_when_no_work_unit_names_it() {
        let dir = repo();
        let root = dir.path();
        git(root, &["checkout", "dev"]);

        let base = notebook_at(root, None, Some("orphan note"), false);
        assert_eq!(base["ok"], json!(false));
        assert_eq!(base["reason"], json!("no-unit"));
        assert!(base["hint"].as_str().unwrap_or_default().contains("--unit"), "the refusal says how");

        let loose = notebook_at(root, Some("no-prefix-branch"), None, false);
        assert_eq!(loose["reason"], json!("no-unit"));

        // The prefix must name a DECLARED base, not merely sit before an
        // underscore: `feature_x` is not a unit of this project, and accepting
        // it would open `.claude/spec/x/notebook.md` — another unit's file —
        // without a word.
        let foreign = notebook_at(root, Some("feature_x"), Some("would land in the wrong unit"), false);
        assert_eq!(foreign["reason"], json!("no-unit"), "report: {foreign}");
        assert!(
            !root.join(".claude/spec/x/notebook.md").exists(),
            "an undeclared prefix must never write into another unit's notebook",
        );
        assert_eq!(foreign["bases"], json!(["dev", "main"]), "the refusal names the bases it knows");
    }
}
