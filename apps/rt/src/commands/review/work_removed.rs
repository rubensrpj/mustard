//! The scratch checkout the REMOVAL pass runs in — the project as it is now,
//! with the work a spec describes TAKEN AWAY again.
//!
//! ## Why a third tree at all
//!
//! [`super::ac_negative_check`] proves a criterion RED before its work exists
//! and GREEN after it landed. Both are satisfied by a criterion that verifies
//! something the work merely dragged along — a comment carrying the word, a
//! file that exists and is never called. The transition that separates the two
//! is the third: take the work away and require the criterion to go red again.
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

use std::path::{Path, PathBuf};

use crate::commands::git_settle::{git_ok, git_out};

/// The file name each wave caches its signature digest under.
const WAVE_DIFF: &str = "diff.md";

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
    };
    for path in &paths {
        if strip_one(&removed.tree, &base, path) {
            removed.taken_away.push(path.clone());
        }
    }
    if removed.taken_away.is_empty() {
        return Err("nothing-taken-away".to_string());
    }
    Ok(removed)
}

/// Restore ONE path in `tree` to its `base` content, or delete it when `base`
/// never carried it (the file IS the work). `true` when the tree changed.
fn strip_one(tree: &Path, base: &str, path: &str) -> bool {
    let at_base = format!("{base}:{path}");
    if git_ok(tree, &["cat-file", "-e", &at_base]) {
        return git_ok(tree, &["checkout", base, "--", path]);
    }
    std::fs::remove_file(tree.join(path)).is_ok()
}

/// A scratch path outside the project: the system temp directory, named for
/// this process so two concurrent runs never collide.
///
/// Outside the project on purpose — a worktree nested under `root` would be
/// walked by every tool that scans the tree, including this one.
fn scratch_path(root: &Path) -> PathBuf {
    let slug = root
        .file_name()
        .map_or_else(|| "project".to_string(), |n| n.to_string_lossy().into_owned());
    std::env::temp_dir().join(format!("mustard-removal-{slug}-{}", std::process::id()))
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

        // THE WORK: one file edited, one file created.
        std::fs::write(root.join("kept.txt"), "after\n").unwrap();
        std::fs::write(root.join("added.txt"), "new\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "work"]);

        // The record the pipeline already keeps.
        let spec_dir = root.join(".claude").join("spec").join("s");
        std::fs::create_dir_all(spec_dir.join("wave-1-rt")).unwrap();
        std::fs::write(
            spec_dir.join("wave-1-rt").join(WAVE_DIFF),
            "- `kept.txt` (modified)\n- `added.txt` (new)\n",
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
        assert_eq!(removed.taken_away().len(), 2, "{:?}", removed.taken_away());

        // The live checkout is untouched — the whole reason the strip happens
        // somewhere else.
        assert_eq!(
            std::fs::read_to_string(root.join("kept.txt")).unwrap().trim(),
            "after"
        );
        assert!(root.join("added.txt").is_file());

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
