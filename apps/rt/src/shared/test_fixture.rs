//! Cloning test scenery instead of rebuilding it.
//!
//! ## The cost this exists to remove
//!
//! A git-backed test has to build a world before it can assert anything: a bare
//! origin, a checkout, configs, commits, a remote, worktrees. `git_settle`'s
//! fixture makes EIGHTEEN `git` invocations to do it, and each invocation is a
//! process — 0,037s of pure birth-and-death on Windows even for `git --version`.
//! Measured 2026-08-14: the fixture costs 1,18s of a test's 1,74s, so two thirds
//! of what those 28 tests spend goes into scenery they throw away.
//!
//! The scenery is IDENTICAL every time, so it can be built once per test process
//! and copied. Measured: 0,28s per clone against 1,18s per build — **4,2×**.
//!
//! ## The trap, and why the whole tree is copied
//!
//! Git records the remote URL and each worktree registration as an ABSOLUTE
//! path. Copy only the checkout and the clone still resolves to the TEMPLATE's
//! origin and the TEMPLATE's worktree — so tests running in parallel write over
//! one another. That trades wall clock for intermittent failures, which is a
//! worse defect than the one being fixed.
//!
//! Worse still, `git worktree repair` does not notice: while the template
//! directory exists, nothing looks broken from git's point of view. Observed
//! directly — the first attempt produced clones whose `remote -v` and
//! `worktree list` both pointed back at the template.
//!
//! So [`clone_of`] copies the WHOLE tree, origin included, and the caller rewires
//! the paths its own layout knows about. Three git calls (one `remote set-url`,
//! one `worktree repair` per worktree) against eighteen — and
//! `a_cloned_fixture_is_independent_of_its_template` asserts the rewiring held.

use std::path::Path;

/// Copy a directory tree recursively, including empty directories.
///
/// `std::fs` has no such call, and a fixture template is small (tens of files),
/// so the naive walk is the right shape. Panics on IO error: this runs only
/// under `#[cfg(test)]`, where a failed copy must fail the test loudly rather
/// than degrade into a half-built repository that fails somewhere confusing.
/// A SYMLINK is copied as its target's CONTENT, not re-linked, and a symlink
/// whose target is gone is skipped rather than fatal. That asymmetry is not
/// cosmetic: `entry.file_type()` does NOT follow links (it is `lstat`) while
/// `fs::copy` DOES, so a dangling link reads as a plain file and then fails to
/// copy. It cost a red CI on macOS and Linux — `copy template file: NotFound` —
/// while Windows, which has no such links in a git tree, stayed green. Found by
/// the three-OS matrix, 2026-08-14.
pub(crate) fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create clone dir");
    for entry in std::fs::read_dir(src).expect("read template dir") {
        let entry = entry.expect("template entry");
        let target = dst.join(entry.file_name());
        let from = entry.path();
        if entry.file_type().expect("entry type").is_dir() {
            copy_dir_all(&from, &target);
        } else if from.exists() {
            // `exists()` FOLLOWS the link, so this is exactly the question
            // `fs::copy` is about to ask. Naming the path in the message keeps
            // a future failure diagnosable instead of anonymous.
            std::fs::copy(&from, &target)
                .unwrap_or_else(|e| panic!("copy template file {}: {e}", from.display()));
        }
    }
}

/// A fresh temporary directory holding a copy of `template`.
///
/// The caller is responsible for rewiring whatever absolute paths its layout
/// records — see the module doc: git stores the remote URL and each worktree
/// registration absolutely, and a clone that skips the rewiring silently shares
/// the template's state.
pub(crate) fn clone_of(template: &Path) -> tempfile::TempDir {
    let dest = tempfile::tempdir().expect("clone tempdir");
    copy_dir_all(template, dest.path());
    dest
}
