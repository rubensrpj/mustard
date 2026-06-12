//! Local git inspection for the dashboard project overview card.
//!
//! [`dashboard_git_info`] shells out to the local `git` binary inside the
//! selected repository and projects a few read-only facts the overview card
//! renders: the `origin` remote URL, the current branch, the ahead/behind
//! counts against its upstream, and the last commit (hash, message, author,
//! ISO date).
//!
//! FAIL-OPEN CONTRACT (mirrors every dashboard command): a missing repository,
//! a missing `git` binary, a detached HEAD, or a missing remote/upstream never
//! surfaces as an `Err` toast — each sub-probe degrades to an empty field so
//! the card shows an empty state instead. The command only returns `Ok`.
//!
//! WINDOWS-INVISIBLE INVOCATION: every spawn goes through
//! [`crate::process_util::no_window_command`], which sets `CREATE_NO_WINDOW`
//! on Windows so packaged users never see a console flash.

use crate::process_util::no_window_command;
use serde::Serialize;
use std::path::Path;

/// Working-tree change counts parsed from `git status --porcelain`. All zero on
/// a clean tree, a non-repo, or any probe failure (fail-open).
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct GitPending {
    /// Files with a staged change (index column of the porcelain status).
    pub staged: u32,
    /// Tracked files with an unstaged change (work-tree column).
    pub unstaged: u32,
    /// Untracked files (`??` entries).
    pub untracked: u32,
}

/// One commit from the recent log, one field per `git log` format token so a
/// subject containing any character never splits the parse.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CommitSummary {
    /// Abbreviated commit hash (`%h`).
    pub hash: String,
    /// Commit subject line (`%s`) — may contain any character.
    pub subject: String,
    /// Author name (`%an`).
    pub author: String,
    /// Committer date, ISO-8601 (`%cI`).
    pub date: String,
}

/// Read-only snapshot of a repository's git state. Every field defaults to its
/// empty form so a non-repo / no-remote path renders as an empty-state card.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct GitInfo {
    /// `true` when `repo_path` is inside a git work tree.
    pub is_repo: bool,
    /// URL of the `origin` remote, empty when there is no remote.
    pub remote_url: String,
    /// Current branch name, empty on a detached HEAD or non-repo.
    pub branch: String,
    /// Commits ahead of the upstream (0 when no upstream is configured).
    pub ahead: u32,
    /// Commits behind the upstream (0 when no upstream is configured).
    pub behind: u32,
    /// Abbreviated hash of the last commit, empty when the repo has no commits.
    pub last_commit_hash: String,
    /// Subject line of the last commit.
    pub last_commit_message: String,
    /// Author name of the last commit.
    pub last_commit_author: String,
    /// Author date of the last commit, ISO-8601 (`%cI`), empty when absent.
    pub last_commit_date: String,
    /// Working-tree change counts (staged / unstaged / untracked).
    pub pending: GitPending,
    /// Local branch names (`git branch`), capped at ~20.
    pub branches: Vec<String>,
    /// The last 10 commits, newest first.
    pub recent_commits: Vec<CommitSummary>,
}

/// Run `git <args>` in `repo_path` and return trimmed stdout, or `None` when
/// the spawn fails or git exits non-zero. The fail-open primitive every probe
/// below is built on — an error is indistinguishable from "no data here".
fn git_capture(repo_path: &Path, args: &[&str]) -> Option<String> {
    let output = no_window_command("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Inspect the local git state of `repo_path`. Always returns `Ok`; absent data
/// (no repo, no remote, no upstream, no commits) yields empty fields, never an
/// error — the overview card renders the empty state instead of a toast.
#[tauri::command]
pub async fn dashboard_git_info(repo_path: String) -> Result<GitInfo, String> {
    // A join error (panic in the closure) degrades to an empty overview, never
    // an Err toast — the failure-tolerant contract.
    let info = tauri::async_runtime::spawn_blocking(move || git_info_impl(&repo_path))
        .await
        .unwrap_or_default();
    Ok(info)
}

/// Synchronous body of [`dashboard_git_info`], kept separate so unit tests call
/// it directly without a Tauri runtime.
fn git_info_impl(repo_path: &str) -> GitInfo {
    let base = Path::new(repo_path);
    let mut info = GitInfo::default();

    // Gate every other probe on being inside a work tree. `rev-parse
    // --is-inside-work-tree` prints `true` on success; anything else (not a
    // repo, git missing) leaves the empty default in place.
    let is_repo = git_capture(base, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s == "true")
        .unwrap_or(false);
    if !is_repo {
        return info;
    }
    info.is_repo = true;

    // Remote URL — `origin` only; absent remote leaves the field empty.
    if let Some(url) = git_capture(base, &["remote", "get-url", "origin"]) {
        info.remote_url = url;
    }

    // Current branch. `HEAD` on a detached checkout is treated as no branch.
    if let Some(branch) = git_capture(base, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        if branch != "HEAD" {
            info.branch = branch;
        }
    }

    // Ahead/behind vs the upstream. `@{upstream}` resolves only when one is
    // configured; the whole probe is skipped (counts stay 0) otherwise. Output
    // is "<behind>\t<ahead>" with --left-right against `@{u}...HEAD`.
    if let Some(counts) = git_capture(
        base,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    ) {
        let mut parts = counts.split_whitespace();
        if let Some(behind) = parts.next().and_then(|s| s.parse::<u32>().ok()) {
            info.behind = behind;
        }
        if let Some(ahead) = parts.next().and_then(|s| s.parse::<u32>().ok()) {
            info.ahead = ahead;
        }
    }

    // Last commit, one field per format token so values that contain the
    // separator (commit subjects do) never split wrong.
    if let Some(hash) = git_capture(base, &["log", "-1", "--format=%h"]) {
        info.last_commit_hash = hash;
    }
    if let Some(message) = git_capture(base, &["log", "-1", "--format=%s"]) {
        info.last_commit_message = message;
    }
    if let Some(author) = git_capture(base, &["log", "-1", "--format=%an"]) {
        info.last_commit_author = author;
    }
    if let Some(date) = git_capture(base, &["log", "-1", "--format=%cI"]) {
        info.last_commit_date = date;
    }

    // Pending changes from porcelain v1. Each line's first two columns are the
    // index (staged) and work-tree (unstaged) status; `??` marks an untracked
    // file. `git_capture` trims the trailing newline, so blank lines never
    // appear except an all-empty (clean tree) output, which yields zero counts.
    if let Some(status) = git_capture(base, &["status", "--porcelain"]) {
        for line in status.lines() {
            if line.starts_with("??") {
                info.pending.untracked += 1;
                continue;
            }
            let mut cols = line.chars();
            let index = cols.next().unwrap_or(' ');
            let worktree = cols.next().unwrap_or(' ');
            if index != ' ' {
                info.pending.staged += 1;
            }
            if worktree != ' ' {
                info.pending.unstaged += 1;
            }
        }
    }

    // Local branches only, one short name per line. Capped so a repo with many
    // branches never floods the card.
    if let Some(out) = git_capture(base, &["branch", "--format=%(refname:short)"]) {
        info.branches = out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(20)
            .map(str::to_string)
            .collect();
    }

    // Recent commits, last 10. Fields are separated by the US control char
    // (0x1f), which cannot occur in a commit subject, so a subject containing
    // `|`, tabs, or newlines never breaks the parse. (`%s` is single-line.)
    if let Some(out) = git_capture(
        base,
        &["log", "-10", "--format=%h%x1f%s%x1f%an%x1f%cI"],
    ) {
        for line in out.lines() {
            let mut fields = line.split('\u{1f}');
            let hash = fields.next().unwrap_or("").to_string();
            let subject = fields.next().unwrap_or("").to_string();
            let author = fields.next().unwrap_or("").to_string();
            let date = fields.next().unwrap_or("").to_string();
            if hash.is_empty() {
                continue;
            }
            info.recent_commits.push(CommitSummary {
                hash,
                subject,
                author,
                date,
            });
        }
    }

    info
}

/// Inspect the commit log of an arbitrary ref so the overview card can switch
/// between branches. Always returns `Ok`; a non-repo, missing `git`, invalid
/// ref, or a zero limit degrades to an empty `Vec`, never an `Err` toast —
/// the same fail-open contract as [`dashboard_git_info`].
///
/// The ref arrives from the front as `gitRef` (camelCase); Tauri maps it to the
/// `git_ref` argument automatically.
#[tauri::command]
pub async fn dashboard_git_log(
    repo_path: String,
    git_ref: String,
    limit: u32,
) -> Result<Vec<CommitSummary>, String> {
    // A join error (panic in the closure) degrades to an empty log, never an
    // Err toast — the failure-tolerant contract.
    let commits =
        tauri::async_runtime::spawn_blocking(move || git_log_impl(&repo_path, &git_ref, limit))
            .await
            .unwrap_or_default();
    Ok(commits)
}

/// Synchronous body of [`dashboard_git_log`], kept separate so unit tests call
/// it directly without a Tauri runtime.
fn git_log_impl(repo_path: &str, git_ref: &str, limit: u32) -> Vec<CommitSummary> {
    // A zero limit means "nothing to show"; short-circuit before spawning git.
    if limit == 0 {
        return Vec::new();
    }
    // An empty ref falls back to HEAD; cap the count so a huge limit never
    // floods the card.
    let git_ref = if git_ref.trim().is_empty() {
        "HEAD"
    } else {
        git_ref
    };
    let capped = limit.min(200);
    let count = format!("-n{capped}");

    let base = Path::new(repo_path);
    // The ref is passed as a positional argument (never interpolated into a
    // shell). `--end-of-options` forces everything after it to be parsed as a
    // revision, so a ref beginning with `-` can never be mistaken for a flag;
    // the trailing `--` then separates the revision from any path. Fields use
    // the US control char (0x1f) separator, the same scheme as
    // `recent_commits`, so a subject with `|`/tabs never splits.
    let Some(out) = git_capture(
        base,
        &[
            "log",
            "--format=%h%x1f%s%x1f%an%x1f%cI",
            &count,
            "--end-of-options",
            git_ref,
            "--",
        ],
    ) else {
        return Vec::new();
    };

    let mut commits = Vec::new();
    for line in out.lines() {
        let mut fields = line.split('\u{1f}');
        let hash = fields.next().unwrap_or("").to_string();
        let subject = fields.next().unwrap_or("").to_string();
        let author = fields.next().unwrap_or("").to_string();
        let date = fields.next().unwrap_or("").to_string();
        if hash.is_empty() {
            continue;
        }
        commits.push(CommitSummary {
            hash,
            subject,
            author,
            date,
        });
    }
    commits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_repo_path_returns_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let info = git_info_impl(&dir.path().to_string_lossy());
        assert!(!info.is_repo);
        assert!(info.remote_url.is_empty());
        assert!(info.branch.is_empty());
        assert_eq!(info.ahead, 0);
        assert_eq!(info.behind, 0);
        assert!(info.last_commit_hash.is_empty());
    }

    #[test]
    fn repo_without_remote_reports_branch_and_commit() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let run = |args: &[&str]| {
            no_window_command("git")
                .args(args)
                .current_dir(base)
                .output()
        };
        // Skip when git is unavailable on the host — fail-open contract.
        if run(&["init", "-b", "trunk"]).is_err() {
            return;
        }
        let _ = run(&["config", "user.email", "qa@example.com"]);
        let _ = run(&["config", "user.name", "QA Bot"]);
        std::fs::write(base.join("a.txt"), b"hello").unwrap();
        let _ = run(&["add", "."]);
        let _ = run(&["commit", "-m", "initial commit"]);
        // An untracked file so `pending.untracked` has something to count.
        std::fs::write(base.join("untracked.txt"), b"new").unwrap();

        let info = git_info_impl(&base.to_string_lossy());
        assert!(info.is_repo);
        assert!(info.remote_url.is_empty(), "no remote configured");
        assert_eq!(info.branch, "trunk");
        assert!(!info.last_commit_hash.is_empty());
        assert_eq!(info.last_commit_message, "initial commit");
        assert_eq!(info.last_commit_author, "QA Bot");
        assert!(!info.last_commit_date.is_empty());

        // Enriched git-client fields.
        assert!(
            !info.recent_commits.is_empty(),
            "recent_commits has the initial commit"
        );
        assert_eq!(info.recent_commits[0].subject, "initial commit");
        assert_eq!(info.recent_commits[0].author, "QA Bot");
        assert!(
            info.branches.iter().any(|b| b == "trunk"),
            "branches lists the created branch"
        );
        assert!(
            info.pending.untracked >= 1,
            "the untracked file is counted"
        );
    }

    #[test]
    fn git_log_returns_commits_for_head_and_empty_for_missing_ref() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let run = |args: &[&str]| {
            no_window_command("git")
                .args(args)
                .current_dir(base)
                .output()
        };
        // Skip when git is unavailable on the host — fail-open contract.
        if run(&["init", "-b", "trunk"]).is_err() {
            return;
        }
        let _ = run(&["config", "user.email", "qa@example.com"]);
        let _ = run(&["config", "user.name", "QA Bot"]);
        std::fs::write(base.join("a.txt"), b"hello").unwrap();
        let _ = run(&["add", "."]);
        let _ = run(&["commit", "-m", "initial commit"]);

        let path = base.to_string_lossy();

        // HEAD resolves to at least the initial commit.
        let head = git_log_impl(&path, "HEAD", 10);
        assert!(!head.is_empty(), "HEAD log has the initial commit");
        assert_eq!(head[0].subject, "initial commit");
        assert_eq!(head[0].author, "QA Bot");

        // A nonexistent ref degrades to an empty Vec (fail-open).
        assert!(
            git_log_impl(&path, "no-such-branch", 10).is_empty(),
            "an invalid ref yields an empty log, never an error"
        );

        // A zero limit short-circuits to empty.
        assert!(
            git_log_impl(&path, "HEAD", 0).is_empty(),
            "a zero limit yields an empty log"
        );

        // An empty ref falls back to HEAD.
        assert!(
            !git_log_impl(&path, "", 10).is_empty(),
            "an empty ref falls back to HEAD"
        );

        // A ref starting with `-` is never parsed as a flag (terminated by --).
        assert!(
            git_log_impl(&path, "--all", 10).is_empty(),
            "a flag-like ref is treated as a ref, not an option"
        );
    }

    #[test]
    fn git_log_on_non_repo_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let commits = git_log_impl(&dir.path().to_string_lossy(), "HEAD", 10);
        assert!(commits.is_empty(), "a non-repo path yields an empty log");
    }
}
