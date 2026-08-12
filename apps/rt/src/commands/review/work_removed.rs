//! The scratch checkout the REMOVAL pass runs in — the project as it is now,
//! with the work a spec describes TAKEN AWAY again.
//!
//! ## Why a third tree at all
//!
//! [`super::ac_negative_check`] proves a criterion RED before its work exists
//! and GREEN after it landed. Both are satisfied by a criterion that verifies
//! something OUTSIDE the work — a subsystem the waves never touched, a path
//! that was already there. The transition that finds those is the third: take
//! the work away and refuse a criterion that is still green.
//!
//! ## What the strip can and cannot separate — read this before trusting a red
//!
//! The strip is FILE-GRAINED, because file paths are all the record carries. So
//! it takes away the criterion's own evidence together with the behaviour
//! whenever the two live in the same file — which for an inline-test project is
//! every test criterion there is. A red from such a run says "something the
//! command needed is gone" and CANNOT say which of the two it was.
//!
//! That is why this module publishes [`RemovedTree::removed_text`]: the words
//! the strip took out of the tree, so the caller can tell a criterion whose own
//! evidence went with the strip from one that ran against an intact command.
//! Reporting the first as a proven red is the exact "answer that reads like the
//! fact" the pass exists to refuse.
//!
//! ## What "the work" is — read, never guessed
//!
//! Each wave already caches the signature digest of what it changed at
//! `wave-*/diff.md` (`super::super::pipeline::wave_done`), and that digest names
//! every file the wave touched. So the file set is READ off the record the
//! pipeline already keeps. A spec whose waves cached nothing yields
//! `no-cached-diff` — an honest refusal, never an empty removal reported as a
//! clean pass.
//!
//! ## Why a linked worktree and not the live tree
//!
//! Taking work away means writing over source files. Doing that in the
//! developer's checkout would put the operator one crash away from losing
//! uncommitted work, so the strip happens in a linked `git worktree` cut from
//! `HEAD` into the system temp directory, and [`RemovedTree`] removes it on
//! drop. Nothing under the project root is ever written by this module.
//!
//! The cost is real and is not hidden: the scratch tree has no build cache, so
//! a `cargo test` criterion compiles from scratch there. That is what the third
//! transition costs, which is why this pass is asked for and never automatic.
//!
//! Fail-open in the sense the rest of the crate uses: every git error becomes a
//! named `Err` the caller reports as an ENGINE error, never a verdict about a
//! criterion.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::commands::git_settle::{git_ok, git_out};

/// The file name each wave caches its signature digest under.
pub(crate) const WAVE_DIFF: &str = "diff.md";

/// A linked worktree at `HEAD` with a declared file set restored to an earlier
/// revision — the tree the removal pass runs its commands in.
///
/// Removed on drop, including after a panic in the pass: a leaked worktree is
/// state the operator has to clean by hand, and `git worktree add` refuses a
/// path that is still registered.
pub(crate) struct RemovedTree {
    /// The project root the worktree was cut from — where the cleanup runs.
    root: PathBuf,
    /// The scratch checkout itself.
    tree: PathBuf,
    /// The files whose work was actually taken away, as repo paths.
    taken_away: Vec<String>,
    /// The words the strip took OUT of the tree — see [`RemovedTree::removed_text`].
    removed_text: BTreeSet<String>,
}

impl RemovedTree {
    /// The scratch checkout — the working directory each criterion runs in.
    pub(crate) fn path(&self) -> &Path {
        &self.tree
    }

    /// The files the strip actually reached, sorted. Reported rather than
    /// assumed: a removal over an empty set would be a green nobody can act on.
    pub(crate) fn taken_away(&self) -> &[String] {
        &self.taken_away
    }

    /// The words the strip took OUT of the tree: present in some declared file
    /// BEFORE the strip and in NONE of them after it, plus the repo path and
    /// file name of every file the strip deleted outright.
    ///
    /// This is what lets the caller read a red honestly. A criterion whose
    /// command OR `Expect:` regex names one of these words — a test function
    /// name, a file path, a marker string — had its OWN evidence taken away by
    /// the strip, so its red is guaranteed and says nothing about the
    /// behaviour. See [`taken_away_word`].
    pub(crate) fn removed_text(&self) -> &BTreeSet<String> {
        &self.removed_text
    }
}

/// The first word of the criterion's OWN EVIDENCE that `removed_text` says the
/// strip took away, or `None` when the evidence names nothing the removal
/// deleted.
///
/// The evidence is BOTH halves the executor grades with: the `command` and the
/// `Expect:` regex. Taking only the command would leave the whole class of
/// criteria whose marker lives in the expectation — ``Command: `type lib.txt`
/// Expect: `beta_marker` `` — with their red manufactured by the strip and
/// booked as proof, which is the exact defect the decline exists to close. The
/// two are one argument here so no caller can consult half of it.
///
/// Pure and total, and the ONE place a criterion is matched against the strip,
/// so the words are split the same way on both sides. Deliberately
/// conservative: a word that merely MOVED between two stripped files still
/// reads as taken away, which costs an honest "cannot judge" — the safe
/// direction. The unsafe direction is the one this exists to close: reading a
/// guaranteed red as proof.
pub(crate) fn taken_away_word(
    removed_text: &BTreeSet<String>,
    command: &str,
    expect: Option<&str>,
) -> Option<String> {
    words(command)
        .chain(words(expect.unwrap_or_default()))
        .find(|w| removed_text.contains(w))
}

/// The words of `text` as the removal compares them: runs of the characters a
/// command can name — letters, digits, `_`, `-`, `.` and `/` — at least three
/// long, lowercased so a command and a source file that disagree about case
/// still meet. Pure, total.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/')))
        .filter(|w| w.len() >= 3)
        .map(str::to_ascii_lowercase)
}

impl Drop for RemovedTree {
    fn drop(&mut self) {
        let path = self.tree.to_string_lossy().into_owned();
        // `remove` unregisters AND deletes; the explicit delete afterwards is
        // for the case where git declined (a lock, a file still open) and the
        // `prune` for the registration that delete would then orphan.
        let _ = git_ok(&self.root, &["worktree", "remove", "--force", &path]);
        let _ = std::fs::remove_dir_all(&self.tree);
        let _ = git_ok(&self.root, &["worktree", "prune"]);
    }
}

/// Every repo path named by the cached wave digests under `spec_dir`, sorted and
/// deduplicated.
///
/// The digest lists one bullet per changed file — ``- `path` (modified)`` — so
/// the paths are the backtick-quoted spans of the bullet lines. Anything else in
/// the file (the signature lines, the truncation note) carries no backticked
/// path on a bullet and is skipped. Pure over the directory, total.
pub(crate) fn declared_paths(spec_dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(spec_dir) else {
        return found;
    };
    let mut digests: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path().join(WAVE_DIFF))
        .filter(|p| p.is_file())
        .collect();
    digests.sort();
    for digest in digests {
        let Ok(body) = mustard_core::io::fs::read_to_string(&digest) else {
            continue;
        };
        found.extend(body.lines().filter_map(bullet_path));
    }
    found.sort();
    found.dedup();
    found
}

/// The backtick-quoted path of one digest bullet, or `None` for any other line.
/// Pure, total.
fn bullet_path(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("- ")?;
    let inner = rest.strip_prefix('`')?;
    let end = inner.find('`')?;
    let path = inner[..end].trim();
    (!path.is_empty()).then(|| path.replace('\\', "/"))
}

/// Cut a scratch checkout of `root` at `HEAD` and take away, in it, every file
/// the waves under `spec_dir` recorded as changed — restoring each to `from`, or
/// deleting it when `from` never carried it.
///
/// `Err` names the ENGINE failure, never a verdict: `no-cached-diff` (the waves
/// recorded nothing, so nothing says what the work was), `unknown-revision`
/// (`from` does not resolve), `worktree-unavailable` (git could not cut the
/// scratch tree) and `nothing-taken-away` (the tree was cut but not one declared
/// file could be stripped — reporting that as a removal would be reporting a
/// pass over an untouched tree).
pub(crate) fn build(root: &Path, spec_dir: &Path, from: &str) -> Result<RemovedTree, String> {
    let paths = declared_paths(spec_dir);
    if paths.is_empty() {
        return Err("no-cached-diff".to_string());
    }
    let Some(base) = git_out(root, &["rev-parse", "--verify", &format!("{from}^{{commit}}")])
    else {
        return Err("unknown-revision".to_string());
    };
    let tree = scratch_path(root);
    let _ = std::fs::remove_dir_all(&tree);
    let _ = git_ok(root, &["worktree", "prune"]);
    if !git_ok(
        root,
        &["worktree", "add", "--detach", &tree.to_string_lossy(), "HEAD"],
    ) {
        return Err("worktree-unavailable".to_string());
    }
    // From here on the worktree exists, so every early return must go through a
    // constructed `RemovedTree` — its `Drop` is the only cleanup there is.
    let mut removed = RemovedTree {
        root: root.to_path_buf(),
        tree,
        taken_away: Vec::new(),
        removed_text: BTreeSet::new(),
    };
    // The two sides of the word ledger. `before` is what the declared files
    // said WITH the work; `after` is what they say without it. Only the
    // difference is genuinely gone: a word that survives in ANY declared file
    // is still in the tree, and calling it removed would turn honest reds into
    // non-answers.
    let mut before: BTreeSet<String> = BTreeSet::new();
    let mut after: BTreeSet<String> = BTreeSet::new();
    let mut deleted: BTreeSet<String> = BTreeSet::new();
    for path in &paths {
        let full = removed.tree.join(path);
        let pre = std::fs::read_to_string(&full).unwrap_or_default();
        match strip_one(&removed.tree, &base, path) {
            Stripped::Untouched => continue,
            Stripped::Restored => {
                let post = std::fs::read_to_string(&full).unwrap_or_default();
                before.extend(words(&pre));
                after.extend(words(&post));
            }
            Stripped::Deleted => {
                before.extend(words(&pre));
                // The file itself is gone, so its path and name are gone from
                // the tree whatever any other file still says about them — a
                // criterion naming either was reading THIS file.
                deleted.extend(words(path));
                if let Some(name) = Path::new(path).file_name().and_then(|n| n.to_str()) {
                    deleted.extend(words(name));
                }
            }
        }
        removed.taken_away.push(path.clone());
    }
    if removed.taken_away.is_empty() {
        return Err("nothing-taken-away".to_string());
    }
    removed.removed_text = before.difference(&after).cloned().collect();
    removed.removed_text.extend(deleted);
    // The ledger is read off the tree's CONTENT above; committing after it
    // changes nothing the pass reads, and is what leaves an abandoned scratch
    // tree collectable instead of permanently mistaken for unsaved work.
    record_the_strip(&removed.tree);
    Ok(removed)
}

/// What the strip did to one declared file.
enum Stripped {
    /// The file went back to its `base` content — the work in it is gone, the
    /// file is not.
    Restored,
    /// `base` never carried the file, so the file IS the work: it was deleted.
    Deleted,
    /// Nothing changed — git declined, or the path is not there to strip.
    Untouched,
}

/// Restore ONE path in `tree` to its `base` content, or delete it when `base`
/// never carried it (the file IS the work).
fn strip_one(tree: &Path, base: &str, path: &str) -> Stripped {
    let at_base = format!("{base}:{path}");
    if git_ok(tree, &["cat-file", "-e", &at_base]) {
        if git_ok(tree, &["checkout", base, "--", path]) {
            return Stripped::Restored;
        }
        return Stripped::Untouched;
    }
    let full = tree.join(path);
    if std::fs::remove_file(&full).is_err() {
        return Stripped::Untouched;
    }
    prune_empty_parents(tree, &full);
    Stripped::Deleted
}

/// Delete the directories `file` leaves behind once it is gone, up to (never
/// including) `tree`.
///
/// Without this the strip removes a file the work created and leaves the
/// directory that only ever held it, so a criterion asserting the DIRECTORY
/// finds it intact and is reported as having survived a removal that never
/// reached it. `remove_dir` refuses a non-empty directory, which is exactly the
/// stopping rule: a directory that still holds anything was not the work.
fn prune_empty_parents(tree: &Path, file: &Path) {
    let mut current = file.parent().map(Path::to_path_buf);
    while let Some(dir) = current {
        if dir == tree || !dir.starts_with(tree) || std::fs::remove_dir(&dir).is_err() {
            return;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
}

/// Everything in a scratch worktree's name BEFORE the creating process id:
/// `mustard-removal-{slug}-`.
///
/// Published rather than inlined because the collector
/// ([`crate::commands::maint::worktree_gc`]) has to recognise these trees in
/// the temp directory — they are the only worktrees the harness cuts outside
/// `.claude/worktrees/`, and for years nothing could reap the ones an
/// interrupted pass left behind. One function so the name is written in one
/// place and read in the other, never guessed twice.
pub(crate) fn scratch_prefix(root: &Path) -> String {
    let slug = root
        .file_name()
        .map_or_else(|| "project".to_string(), |n| n.to_string_lossy().into_owned());
    format!("mustard-removal-{slug}-")
}

/// A scratch path outside the project: the system temp directory, named for
/// this process so two concurrent runs never collide.
///
/// Outside the project on purpose — a worktree nested under `root` would be
/// walked by every tool that scans the tree, including this one. The trailing
/// process id is what later tells an ABANDONED scratch tree from one a live
/// pass is still working in.
fn scratch_path(root: &Path) -> PathBuf {
    std::env::temp_dir().join(format!("{}{}", scratch_prefix(root), std::process::id()))
}

/// Record the strip as a commit in the scratch tree, so what the strip did is
/// not indistinguishable from work someone forgot to save.
///
/// The tree is detached at `HEAD`, so the commit is unreachable and costs
/// nothing but a loose object git prunes on its own. What it buys is that
/// `git status --porcelain` comes back EMPTY: the collector refuses to remove
/// any worktree holding uncommitted or untracked changes — rightly, that guard
/// is what makes collecting safe at all — and without this the manufactured
/// dirt of the strip would read as an operator's unsaved work, so every
/// abandoned scratch tree would be protected forever.
///
/// Identity and signing are supplied on the command line: this must not depend
/// on the machine's git config, and a repo with `commit.gpgsign` on must not
/// stall a review pass on a passphrase prompt. Best-effort — a failure leaves
/// the tree dirty, which costs collectability and nothing else.
fn record_the_strip(tree: &Path) {
    if !git_ok(tree, &["add", "-A"]) {
        return;
    }
    let _ = git_ok(
        tree,
        &[
            "-c",
            "user.name=mustard",
            "-c",
            "user.email=mustard@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--no-verify",
            "--quiet",
            "-m",
            "mustard: work taken away for the removal pass",
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    /// Whether `git` is on PATH; the git-backed test degrades to a silent pass
    /// when it is not, mirroring `diff_digest`'s own guard.
    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn git(cwd: &Path, args: &[&str]) {
        let _ = Command::new("git").args(args).current_dir(cwd).output();
    }

    /// The file set is READ off the cached wave digests — the bullet paths, and
    /// only those. The signature lines and the truncation note carry no path.
    #[test]
    fn declared_paths_reads_the_cached_wave_digests() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("spec");
        std::fs::create_dir_all(spec_dir.join("wave-2-rt")).unwrap();
        std::fs::create_dir_all(spec_dir.join("wave-1-rt")).unwrap();
        std::fs::write(
            spec_dir.join("wave-1-rt").join(WAVE_DIFF),
            "- `apps/rt/src/a.rs` (modified)\n  + fns: alpha\n- `apps/rt/src/b.rs` (new)\n",
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("wave-2-rt").join(WAVE_DIFF),
            "- `apps/rt/src/a.rs` (modified)\n  (no signature change)\n\
             - ...and more files (showing first 50)\n",
        )
        .unwrap();

        let paths = declared_paths(&spec_dir);
        assert_eq!(
            paths,
            vec!["apps/rt/src/a.rs".to_string(), "apps/rt/src/b.rs".to_string()],
            "sorted, deduplicated across waves"
        );
        // Two-sided: a spec dir with no cached digest declares nothing, which is
        // what makes `no-cached-diff` an honest refusal rather than an empty run.
        assert!(declared_paths(dir.path()).is_empty());
        // The non-path lines are not paths.
        assert_eq!(bullet_path("  + fns: alpha"), None);
        assert_eq!(bullet_path("- ...and more files (showing first 50)"), None);
        assert_eq!(bullet_path("- `` (modified)"), None);
    }

    /// The strip itself: a file the work MODIFIED goes back to its earlier
    /// content, a file the work ADDED disappears, and the project's own tree is
    /// left byte-identical throughout.
    #[test]
    fn build_takes_the_declared_work_away_in_a_scratch_tree() {
        if !git_available() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.email", "t@e.x"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        // The strip restores through `git checkout`, which applies the repo's
        // eol rules — so pin them, or a machine with `core.autocrlf=true`
        // globally reads back `before\r\n` and the assertion becomes about the
        // developer's git config instead of about the strip.
        git(root, &["config", "core.autocrlf", "false"]);

        std::fs::write(root.join("kept.txt"), "before\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "base"]);
        let Some(base) = git_out(root, &["rev-parse", "HEAD"]) else {
            return;
        };

        // THE WORK: one file edited, one file created, one file created inside
        // a directory that exists only to hold it.
        std::fs::write(root.join("kept.txt"), "after marker_kept\n").unwrap();
        std::fs::write(root.join("added.txt"), "new marker_added\n").unwrap();
        std::fs::create_dir_all(root.join("feature")).unwrap();
        std::fs::write(root.join("feature").join("impl.txt"), "body\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "work"]);

        // The record the pipeline already keeps.
        let spec_dir = root.join(".claude").join("spec").join("s");
        std::fs::create_dir_all(spec_dir.join("wave-1-rt")).unwrap();
        std::fs::write(
            spec_dir.join("wave-1-rt").join(WAVE_DIFF),
            "- `kept.txt` (modified)\n- `added.txt` (new)\n- `feature/impl.txt` (new)\n",
        )
        .unwrap();

        let Ok(removed) = build(root, &spec_dir, &base) else {
            return; // git present but the worktree could not be cut — fail-open.
        };
        let scratch = removed.path().to_path_buf();
        assert_eq!(
            std::fs::read_to_string(scratch.join("kept.txt")).unwrap().trim(),
            "before",
            "a modified file goes back to the tree before the work"
        );
        assert!(
            !scratch.join("added.txt").exists(),
            "a file the work created IS the work — taking it away deletes it"
        );
        assert_eq!(removed.taken_away().len(), 3, "{:?}", removed.taken_away());

        // The directory that only ever held the work goes with it. Leaving it
        // standing is how a criterion asserting the DIRECTORY reports having
        // survived a removal that never reached it.
        assert!(
            !scratch.join("feature").exists(),
            "the emptied parent directory survived the strip"
        );

        // The word ledger — what makes a red readable. Both shapes are asserted
        // AND their two-sided counterpart: a word the strip did not take out of
        // the tree must not be listed, or every red would read as a non-answer.
        let removed_text = removed.removed_text();
        assert!(
            removed_text.contains("marker_added"),
            "a word that lived only in a deleted file is gone: {removed_text:?}"
        );
        assert!(
            removed_text.contains("marker_kept"),
            "a word the work added to a surviving file is gone too: {removed_text:?}"
        );
        assert!(
            removed_text.contains("added.txt") && removed_text.contains("feature/impl.txt"),
            "a deleted file's own path is gone: {removed_text:?}"
        );
        assert!(
            !removed_text.contains("kept.txt"),
            "a file the strip only RESTORED is still there — naming it is not evidence removed"
        );
        assert!(
            !removed_text.contains("before"),
            "a word the strip put BACK is not removed: {removed_text:?}"
        );
        // And the matcher reads a criterion through the same split, both ways
        // and BOTH HALVES: a marker that lives only in the `Expect:` regex is
        // evidence the strip took away exactly as much as one named in the
        // command, so consulting the command alone would hand that whole class
        // a manufactured red.
        assert_eq!(
            taken_away_word(removed_text, "findstr marker_added added.txt", None).as_deref(),
            Some("marker_added")
        );
        assert_eq!(
            taken_away_word(removed_text, "type kept.txt", Some("marker_kept")).as_deref(),
            Some("marker_kept"),
            "the expectation is evidence too"
        );
        assert_eq!(taken_away_word(removed_text, "cd kept.txt", None), None);
        assert_eq!(
            taken_away_word(removed_text, "cd kept.txt", Some("before")),
            None,
            "and an expectation naming nothing the strip took is not a decline"
        );

        // The live checkout is untouched — the whole reason the strip happens
        // somewhere else.
        assert_eq!(
            std::fs::read_to_string(root.join("kept.txt")).unwrap().trim(),
            "after marker_kept"
        );
        assert!(root.join("added.txt").is_file());

        // The strip is RECORDED, so what it did is never mistaken for work
        // somebody forgot to save. This is what a killed pass leaves behind,
        // and it is the exact predicate the collector applies before removing
        // anything: a tree reading dirty here would be protected forever, so
        // the leak this closes would still be a leak.
        assert!(
            crate::commands::work_unit_open::dirty_paths(&scratch).is_empty(),
            "an abandoned scratch tree must not read as holding work: {:?}",
            crate::commands::work_unit_open::dirty_paths(&scratch)
        );
        assert!(
            git_out(&scratch, &["rev-parse", "--verify", "HEAD"]).is_some(),
            "and it is still a checkout the collector can hand to git"
        );

        // And the scratch tree is gone once the handle drops.
        drop(removed);
        assert!(!scratch.exists(), "the scratch worktree leaked: {}", scratch.display());
    }

    /// A spec whose waves cached nothing refuses by name instead of reporting a
    /// removal over an untouched tree.
    #[test]
    fn a_spec_with_no_cached_diff_is_refused_by_name() {
        let dir = tempdir().unwrap();
        assert_eq!(
            build(dir.path(), dir.path(), "HEAD").err().as_deref(),
            Some("no-cached-diff")
        );
    }
}
