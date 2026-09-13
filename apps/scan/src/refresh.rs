//! Incremental reading and the git history the map keeps.
//!
//! A pass reads again only what can have changed since the previous one: the
//! files git says changed between the commit the previous pass read and the
//! one checked out now, the files not committed now, and the files that were
//! not committed then (they may have been put back since). Everything else is
//! taken from the previous map as it was. Outside git, without a previous
//! map, or when the previous map was written by another scanner build, every
//! file is read.
//!
//! The history comes from the local repository (`git log`), never from the
//! network: the first pass reads it whole, and the next ones read only the
//! commits after the one the previous pass stopped at.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use mustard_core::domain::project_map::{History, RawCommit, MAX_COMMITS};

use crate::model::ProjectModel;

/// The scanner build tag written into the map. A map written by another
/// build is read again in full, because what a file yields may have changed.
pub(crate) const FORMAT: &str = concat!(env!("CARGO_PKG_VERSION"), "+map-2");

/// Files whose change alters how every other file is classified: when one of
/// them changed, everything is read again.
const GLOBAL_INPUTS: &[&str] = &[".gitattributes", ".editorconfig"];

/// What a pass has to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    /// Every file.
    Full,
    /// Only these files (relative to the scanned root), plus any file the
    /// previous map does not know.
    Only(BTreeSet<String>),
}

/// Run git in `root`, with paths printed as they are (no octal quoting of
/// accented names). `None` when git is missing or the command fails.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The commit checked out in `root`, or `None` outside git and on a branch
/// with no commit yet.
pub(crate) fn head(root: &Path) -> Option<String> {
    git(root, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Where `root` sits inside its repository (`apps/scan/`), empty at the top.
fn prefix(root: &Path) -> Option<String> {
    git(root, &["rev-parse", "--show-prefix"]).map(|s| s.trim().to_string())
}

/// The files under `root` that are not committed — changed, staged or new —
/// relative to `root`, in name order.
pub(crate) fn dirty(root: &Path) -> Option<Vec<String>> {
    let prefix = prefix(root)?;
    let out = git(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."])?;
    let mut paths = BTreeSet::new();
    let mut fields = out.split('\0');
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        // A rename or a copy carries the old path in the next field.
        if code[..2].contains(['R', 'C']) {
            let _ = fields.next();
        }
        // The status paths are relative to the top of the repository.
        if let Some(rel) = path.strip_prefix(prefix.as_str()) {
            paths.insert(rel.to_string());
        }
    }
    Some(paths.into_iter().collect())
}

/// The files under `root` that differ between two commits, relative to
/// `root`. `None` when git cannot compare them (a commit that no longer
/// exists, for one).
fn changed_between(root: &Path, from: &str, to: &str) -> Option<Vec<String>> {
    let out = git(root, &["diff", "--name-only", "-z", "--no-renames", "--relative", from, to])?;
    Some(out.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
}

/// The scanned root as the map records it.
pub(crate) fn canonical(root: &Path) -> String {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).to_string_lossy().to_string()
}

/// Decide what this pass reads, given the previous map.
pub(crate) fn plan(root: &Path, prev: Option<&ProjectModel>) -> Plan {
    let Some(prev) = prev else {
        return Plan::Full;
    };
    if prev.state.format != FORMAT || prev.state.head.is_empty() || prev.root != canonical(root) {
        return Plan::Full;
    }
    let Some(now) = head(root) else {
        return Plan::Full;
    };
    let Some(committed) = changed_between(root, &prev.state.head, &now) else {
        return Plan::Full;
    };
    let Some(open) = dirty(root) else {
        return Plan::Full;
    };
    let changed: BTreeSet<String> = committed.into_iter().chain(open).chain(prev.state.dirty.iter().cloned()).collect();
    let global = changed.iter().any(|p| {
        let name = p.rsplit('/').next().unwrap_or(p);
        GLOBAL_INPUTS.contains(&name)
    });
    if global {
        Plan::Full
    } else {
        Plan::Only(changed)
    }
}

/// `true` when `ancestor` is in the history of `head`.
fn is_ancestor(root: &Path, ancestor: &str, head: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, head])
        .current_dir(root)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The history at `head`: the previous one when nothing was committed since,
/// the previous one plus the new commits when `head` descends from where the
/// previous pass stopped, and the whole log otherwise (another branch, a
/// rewritten history, a first pass).
pub(crate) fn history(root: &Path, prev: Option<&History>, prev_head: &str, head: &str) -> History {
    let prev = prev.filter(|h| !h.is_empty() && !prev_head.is_empty());
    if let Some(prev) = prev {
        if prev_head == head {
            return prev.clone();
        }
        if is_ancestor(root, prev_head, head) {
            return match log(root, &format!("{prev_head}..{head}")) {
                Some(newer) => prev.extended(newer),
                None => prev.clone(),
            };
        }
    }
    log(root, head).map(History::from_raw).unwrap_or_default()
}

/// The commits of `range` that touch `root`, oldest first.
fn log(root: &Path, range: &str) -> Option<Vec<RawCommit>> {
    let max = format!("--max-count={MAX_COMMITS}");
    let out = git(
        root,
        &["log", "--no-merges", "--no-renames", "--relative", "--name-status", "--format=%x00%H %ct", &max, range],
    )?;
    Some(parse_log(&out))
}

/// Parse `git log --name-status --format=%x00%H %ct` into commits, oldest
/// first. A commit that touches nothing under the scanned root is left out.
pub(crate) fn parse_log(text: &str) -> Vec<RawCommit> {
    let mut commits = Vec::new();
    for block in text.split('\0').filter(|b| !b.trim().is_empty()) {
        let mut lines = block.lines();
        let Some(header) = lines.next() else {
            continue;
        };
        let mut parts = header.split_whitespace();
        let (Some(sha), Some(at)) = (parts.next(), parts.next()) else {
            continue;
        };
        let mut commit =
            RawCommit { id: sha.chars().take(10).collect(), at: at.parse().unwrap_or(0), ..RawCommit::default() };
        for line in lines {
            let Some((status, path)) = line.split_once('\t') else {
                continue;
            };
            let path = unquote(path);
            match status.chars().next() {
                Some('A') => commit.added.push(path),
                Some('D') | None => {}
                Some(_) => commit.changed.push(path),
            }
        }
        if !commit.added.is_empty() || !commit.changed.is_empty() {
            commits.push(commit);
        }
    }
    commits.reverse();
    commits
}

/// Undo git's C-style quoting of a path with unusual characters.
fn unquote(path: &str) -> String {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_is_read_oldest_first_without_deletions_or_empty_commits() {
        let text = "\0bbbbbbbbbbbbbbbb 20\n\nM\tsrc/a.rs\nD\tsrc/old.rs\nA\t\"src/with\\\"quote.rs\"\n\
                    \0cccccccccccc 15\n\n\
                    \0aaaaaaaaaaaaaaaa 10\n\nA\tsrc/a.rs\n";
        let commits = parse_log(text);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "aaaaaaaaaa");
        assert_eq!(commits[0].added, vec!["src/a.rs".to_string()]);
        assert_eq!(commits[1].at, 20);
        assert_eq!(commits[1].changed, vec!["src/a.rs".to_string()]);
        assert_eq!(commits[1].added, vec!["src/with\"quote.rs".to_string()]);
    }

    #[test]
    fn without_a_previous_map_everything_is_read() {
        assert_eq!(plan(Path::new("."), None), Plan::Full);
        let other_build = ProjectModel {
            state: crate::model::ScanState { format: "0.0.0+old".to_string(), head: "x".to_string(), ..Default::default() },
            ..Default::default()
        };
        assert_eq!(plan(Path::new("."), Some(&other_build)), Plan::Full);
    }
}
