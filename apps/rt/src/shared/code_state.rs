//! `code_state` — a fingerprint of the working tree, so a recorded test run can
//! be told apart from a stale one.
//!
//! ## Why this exists
//!
//! Mustard's law is that a pass is an OBSERVED exit code, never an inferred one.
//! It already keeps the observation: every `qa-run` writes a `qa.result` event,
//! and the close gate reads that record instead of re-executing the criteria.
//! Half of "do not repeat what was already tested" was therefore already true.
//!
//! What was missing is the other half — deciding when the record stopped being
//! about the code in front of you. The staleness check compared the run's
//! timestamp against the mtime of `spec.md` and `wave-plan.md`, and against
//! nothing else. The source tree was not in the comparison at all, so:
//!
//! ```text
//! QA passes → qa.result overall=pass
//!    ├─ edit the SPEC  → detected as stale → re-run          ✔
//!    └─ edit the CODE  → not detected      → close on the old record   ✘
//! ```
//!
//! That is backwards. The file that almost never changes invalidated the
//! record; the one that always changes did not. So a unit could close on a green
//! observed before the change under review — the exact "nobody watched this
//! pass" outcome the law exists to prevent.
//!
//! ## What the fingerprint covers, and what it does not
//!
//! Caching a test result is ordinary practice (Bazel, Nx and Turborepo all do
//! it), and it has exactly one hard condition: the key must cover everything
//! that can change the answer. A key that misses something hands back a green
//! nobody observed, which is worse than re-running. So the limits are stated
//! here rather than assumed:
//!
//! | Covered | Not covered |
//! |---|---|
//! | the commit `HEAD` points at | the CONTENT of untracked files (only their names) |
//! | the content of every tracked modification (`git diff HEAD`) | anything outside the repository — installed toolchains, env vars, services |
//! | which untracked files exist | a test that is simply flaky |
//! | | `.claude/` — the harness's own writes, excluded on purpose (see [`fingerprint`]) |
//!
//! The uncovered column is why [`fingerprint`] is a FRESHNESS check and never a
//! promise: it answers *did the code move*, and a `None` (no repository, git
//! unavailable, any read error) must be read as *cannot tell*, which every
//! caller treats as stale. Fail-closed: an unanswerable question re-runs.

use std::path::Path;
use std::process::Command;

use crate::util::sha256::Sha256;

/// The payload key a `qa.result` carries its fingerprint under.
pub const CODE_STATE_KEY: &str = "codeState";

/// The harness's own bookkeeping, excluded from the fingerprint. See
/// [`fingerprint`] on why this exclusion is what makes the key usable at all.
const HARNESS_DIR: &str = ":(exclude).claude";

/// A short, stable fingerprint of the working tree at `cwd`, or `None` when it
/// cannot be taken — no repository, no `git`, or any command that fails.
///
/// `None` is *cannot tell*, never *nothing changed*: a caller comparing
/// fingerprints must treat an absent one on either side as stale.
///
/// **`.claude/` is excluded, and without that the key is unusable.** The
/// harness writes there constantly — every event lands in
/// `.claude/spec/<slug>/.events/`, including the very `qa.result` that carries
/// this fingerprint. Counting those made the key go stale one write after being
/// taken, so a record could never describe the tree it was recorded on. Caught
/// by the reuse test on the first run: the fingerprint taken before emitting no
/// longer matched the moment the event file existed.
///
/// The exclusion is scoped to the diff and the untracked list. `HEAD` is not
/// scoped, so a commit that touched ONLY harness files still moves the key —
/// conservative, and stated rather than hidden: it re-runs where it need not,
/// which is the safe direction for a freshness check.
///
/// Never panics; every failure degrades to `None`.
#[must_use]
pub fn fingerprint(cwd: &Path) -> Option<String> {
    let head = git(cwd, &["rev-parse", "HEAD"])?;
    // The content of tracked modifications, not merely the fact that some file
    // is dirty: `git status --porcelain` names the same files whatever they now
    // contain, so a second edit to an already-dirty file would not move the key.
    let dirty = git(cwd, &["diff", "HEAD", "--", ".", HARNESS_DIR])?;
    // Untracked files by NAME. Their content is deliberately not read — that is
    // an unbounded walk on every QA run, and the module docs carry the limit.
    let untracked = git(
        cwd,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--",
            ".",
            HARNESS_DIR,
        ],
    )?;

    let mut h = Sha256::new();
    h.update(head.as_bytes());
    h.update(b"\0");
    h.update(dirty.as_bytes());
    h.update(b"\0");
    h.update(untracked.as_bytes());
    Some(h.hex_digest().chars().take(16).collect())
}

/// `true` when a run recorded under `recorded` still describes the tree at
/// `cwd`. An absent recorded fingerprint, or one that cannot be taken now,
/// answers `false` — see [`fingerprint`] on why *cannot tell* is stale.
#[must_use]
pub fn still_current(cwd: &Path, recorded: Option<&str>) -> bool {
    let Some(recorded) = recorded.filter(|s| !s.trim().is_empty()) else {
        return false;
    };
    fingerprint(cwd).is_some_and(|now| now == recorded)
}

/// One git read in `cwd`. `None` on a non-zero exit or any spawn failure — the
/// caller degrades, never propagates.
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn git_ok(root: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    fn repo(root: &Path) {
        git_ok(root, &["init", "-q"]);
        git_ok(root, &["config", "user.email", "t@example.com"]);
        git_ok(root, &["config", "user.name", "t"]);
        std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
        git_ok(root, &["add", "-A"]);
        git_ok(root, &["commit", "-qm", "init"]);
    }

    /// The whole point: an unchanged tree keeps its fingerprint, so a recorded
    /// run stays reusable.
    #[test]
    fn an_unchanged_tree_keeps_its_fingerprint() {
        let dir = tempdir().unwrap();
        repo(dir.path());
        let first = fingerprint(dir.path()).expect("a repository has a fingerprint");
        assert_eq!(fingerprint(dir.path()).as_deref(), Some(first.as_str()));
        assert!(still_current(dir.path(), Some(&first)));
    }

    /// **The hole this closes.** Editing the CODE used to leave the recorded QA
    /// pass looking fresh, because the staleness check only watched the spec.
    #[test]
    fn code_moved_after_qa_changes_the_fingerprint() {
        let dir = tempdir().unwrap();
        repo(dir.path());
        let before = fingerprint(dir.path()).expect("fingerprint");

        std::fs::write(dir.path().join("a.rs"), "fn a() { panic!() }\n").unwrap();
        let after = fingerprint(dir.path()).expect("fingerprint");
        assert_ne!(before, after, "a tracked edit must move the key");
        assert!(
            !still_current(dir.path(), Some(&before)),
            "a run recorded before that edit no longer describes this tree"
        );
    }

    /// A SECOND edit to an already-dirty file must move it too — the reason the
    /// key hashes `git diff HEAD` and not `git status --porcelain`, which names
    /// the same file whatever it now contains.
    #[test]
    fn code_moved_after_qa_moves_again_on_a_second_edit() {
        let dir = tempdir().unwrap();
        repo(dir.path());
        std::fs::write(dir.path().join("a.rs"), "fn a() { 1 }\n").unwrap();
        let once = fingerprint(dir.path()).expect("fingerprint");
        std::fs::write(dir.path().join("a.rs"), "fn a() { 2 }\n").unwrap();
        let twice = fingerprint(dir.path()).expect("fingerprint");
        assert_ne!(
            once, twice,
            "dirty-file CONTENT is part of the key, not just its name"
        );
    }

    /// A new untracked file counts as movement.
    #[test]
    fn code_moved_after_qa_sees_a_new_untracked_file() {
        let dir = tempdir().unwrap();
        repo(dir.path());
        let before = fingerprint(dir.path()).expect("fingerprint");
        std::fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
        assert_ne!(Some(before), fingerprint(dir.path()));
    }

    /// **The harness's own writes do NOT move it**, and without this the key is
    /// unusable: the `qa.result` event carrying the fingerprint lands under
    /// `.claude/`, so counting that directory made every record go stale one
    /// write after being taken. Found by the reuse test on its first run.
    #[test]
    fn the_harness_own_bookkeeping_does_not_move_the_fingerprint() {
        let dir = tempdir().unwrap();
        repo(dir.path());
        let before = fingerprint(dir.path()).expect("fingerprint");

        let events = dir.path().join(".claude/spec/feat/.events");
        std::fs::create_dir_all(&events).unwrap();
        std::fs::write(events.join("1.ndjson"), "{\"event\":\"qa.result\"}\n").unwrap();
        std::fs::write(dir.path().join(".claude/spec/feat/spec.md"), "# feat\n").unwrap();

        assert_eq!(
            fingerprint(dir.path()).as_deref(),
            Some(before.as_str()),
            "recording the run must not invalidate the record it just wrote"
        );
        assert!(still_current(dir.path(), Some(&before)));
    }

    /// A new commit moves it — the ordinary case of work landing.
    #[test]
    fn code_moved_after_qa_sees_a_new_commit() {
        let dir = tempdir().unwrap();
        repo(dir.path());
        let before = fingerprint(dir.path()).expect("fingerprint");
        std::fs::write(dir.path().join("a.rs"), "fn a() { 3 }\n").unwrap();
        git_ok(dir.path(), &["add", "-A"]);
        git_ok(dir.path(), &["commit", "-qm", "second"]);
        assert_ne!(Some(before), fingerprint(dir.path()));
    }

    /// Fail-closed on both halves of *cannot tell*: no repository, and no
    /// recorded fingerprint to compare against.
    #[test]
    fn cannot_tell_reads_as_stale() {
        let dir = tempdir().unwrap();
        assert_eq!(
            fingerprint(dir.path()),
            None,
            "no repository → no fingerprint"
        );
        assert!(!still_current(dir.path(), Some("deadbeefdeadbeef")));

        let repo_dir = tempdir().unwrap();
        repo(repo_dir.path());
        assert!(
            !still_current(repo_dir.path(), None),
            "no record → nothing to trust"
        );
        assert!(
            !still_current(repo_dir.path(), Some("   ")),
            "a blank record is no record"
        );
    }
}
