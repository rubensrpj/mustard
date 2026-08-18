//! `git_exclude` — the clone-local ignore layer: rules that hide paths from ONE
//! clone's git while leaving nothing behind in any versioned file.
//!
//! ## What it owns
//!
//! Two questions a private install has to ask the host repository, and neither
//! can be answered by reading a file whose path we spelled ourselves:
//!
//! - *"where is this clone's exclude file?"* — resolved by running
//!   `git rev-parse --git-path info/exclude`, **never** by joining the literal
//!   `.git/info/exclude`. Measured against real git in all three repository
//!   shapes (2026-08-17):
//!
//!   | shape            | what git returns                          | `.git` |
//!   |------------------|-------------------------------------------|--------|
//!   | ordinary repo    | `.git/info/exclude` — **relative to cwd** | dir    |
//!   | submodule        | `<super>/.git/modules/<name>/info/exclude` | file  |
//!   | linked worktree  | `<main>/.git/info/exclude` (the common dir) | file  |
//!
//!   Two consequences the code below depends on: in a submodule and in a linked
//!   worktree the literal path **does not exist at all** (`.git` is a pointer
//!   file, so a write there lands nowhere), and the resolved value is sometimes
//!   relative — so it is joined against the project root before use.
//!
//! - *"does this repository already track that path?"* — answered by
//!   `git ls-files`. An exclude rule only acts on an UNTRACKED path, so a
//!   footprint path already in the host's index is residue no rule can hide.
//!   It is reported, never unlinked: `git rm --cached` rewrites the host's
//!   index, and that is the operator's decision, not an install-time cosmetic.
//!
//! ## Contracts honoured
//!
//! - Writes go through [`fs::write_atomic`] only; nothing here panics
//!   (`unwrap`/`expect` are `deny` outside tests).
//! - **This module never errors and never stops anything.** No git on `PATH`,
//!   no repository, an unreadable or unwritable exclude file — each degrades to
//!   "nothing was excluded" and is REPORTED through
//!   [`ExcludeOutcome::unavailable`]. What it does NOT do is flatten the three
//!   into one sentence: [`ExcludeFailure::is_blocking`] separates "there was no
//!   repository to hide from" from "there WAS one and we failed to hide", and a
//!   private install refuses on the second. Deciding that here would be wrong —
//!   a shared install calls the same seam and has nothing to refuse — so the
//!   fact is typed and the judgement stays with the caller.
//! - No `println!`: this is a library seam. Callers render the outcome.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io::fs;
use crate::platform::error::Error;

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// Why NOTHING could be excluded — a closed set, because the caller does not
/// react to all three the same way.
///
/// [`Self::NoRepository`] is the harmless one: there is no git here, so there is
/// nobody for a footprint to be visible TO, and an install may proceed exactly
/// as it does in an ordinary directory. The other two happen only *inside a real
/// repository* — git resolved an exclude file and the write still did not
/// land — and for a private install that is the worst outcome the mode has: the
/// operator believes the client's git cannot see the harness, and it can. So the
/// distinction is carried in the type rather than recovered by reading a
/// sentence, and [`Self::is_blocking`] is the one place it is spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludeFailure {
    /// `git` is not reachable, or the project root is not inside a repository
    /// at all.
    NoRepository,
    /// The exclude file exists but could not be read. Preserving what we could
    /// not inspect is the same rule `seed_static_file` follows.
    Unreadable,
    /// The merged exclude file could not be written back.
    Unwritable,
}

impl ExcludeFailure {
    /// The one short, path-free sentence a report renders. Path-free on purpose:
    /// the install report must stay machine-independent and byte-stable.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::NoRepository => "no git repository here, so nothing was excluded",
            Self::Unreadable => {
                "the clone-local exclude file could not be read, so nothing was excluded"
            }
            Self::Unwritable => {
                "the clone-local exclude file could not be written, so nothing was excluded"
            }
        }
    }

    /// Whether a private install must REFUSE rather than proceed.
    ///
    /// True exactly when there was a repository to hide from and we failed to.
    /// A tree with no repository is not a failure of the mode — nothing there
    /// can ever report a footprint.
    #[must_use]
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Unreadable | Self::Unwritable)
    }
}

/// What one [`ensure_excluded`] run did — facts, not an error.
///
/// The two fields are mutually exclusive in practice: either the write happened
/// (and `appended` names the lines it added, empty when the file already carried
/// them all) or it could not happen at all (and `unavailable` says why).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExcludeOutcome {
    /// The rule lines this run appended, in the order they were supplied.
    /// Empty on a converged second run — the append is idempotent.
    pub appended: Vec<String>,
    /// Why NOTHING was excluded, when the write could not happen. `None` on
    /// every successful run, including the one that had nothing left to add.
    pub unavailable: Option<ExcludeFailure>,
}

/// The header the append writes above the rules it adds, so the lines are
/// attributable rather than mysterious to whoever opens the file next. Mirrors
/// `project_seed::GITIGNORE_BACKFILL_HEADER`, the other line-merge in the
/// install engine.
const EXCLUDE_HEADER: &str =
    "\n# Added by Mustard: this clone only — the exclude file is never committed.\n";

// ---------------------------------------------------------------------------
// The two questions
// ---------------------------------------------------------------------------

/// The clone-local exclude file for the repository containing `root`, as an
/// absolute path, or `None` when git cannot answer (absent binary, no
/// repository, a git that refuses to read this tree).
///
/// Always `git rev-parse --git-path info/exclude`, never the literal
/// `.git/info/exclude` — see the module docs for what each repository shape
/// really returns. A relative answer (which is what an ordinary repository
/// gives) is joined against `root`, so the caller always receives a path it can
/// open from anywhere.
#[must_use]
pub fn exclude_file(root: &Path) -> Option<PathBuf> {
    let raw = git_stdout(root, &["rev-parse", "--git-path", "info/exclude"])?;
    let path = PathBuf::from(raw);
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

/// Idempotently append the rule lines the clone-local exclude file lacks.
///
/// Comparison is on the TRIMMED line, exactly as
/// `project_seed::missing_ignore_patterns` compares ignore patterns: blank
/// lines and comments are layout, and re-appending a rule already in force
/// would grow the file on every install. Whatever the file already carries —
/// the user's own rules, their order, their comments — is untouched, so the
/// operation converges after one run.
///
/// Never errors and never panics: every failure mode degrades to "nothing was
/// excluded" with a reason in [`ExcludeOutcome::unavailable`].
#[must_use]
pub fn ensure_excluded(root: &Path, rules: &[String]) -> ExcludeOutcome {
    let unavailable = |failure: ExcludeFailure| ExcludeOutcome {
        appended: Vec::new(),
        unavailable: Some(failure),
    };

    let Some(path) = exclude_file(root) else {
        return unavailable(ExcludeFailure::NoRepository);
    };
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        // Absent is the normal case on a repository nobody has excluded
        // anything in yet — the rules simply become the whole file.
        Err(Error::NotFound(_)) => String::new(),
        // Present but unreadable is a genuine failure: never stomp what we
        // could not inspect.
        Err(_) => return unavailable(ExcludeFailure::Unreadable),
    };

    let missing = missing_rules(&existing, rules);
    if missing.is_empty() {
        return ExcludeOutcome::default();
    }

    let mut merged = existing;
    let header = if merged.is_empty() {
        // A fresh file opens with the header itself, not a blank first line.
        EXCLUDE_HEADER.trim_start_matches('\n')
    } else {
        if !merged.ends_with('\n') {
            merged.push('\n');
        }
        EXCLUDE_HEADER
    };
    merged.push_str(header);
    for rule in &missing {
        merged.push_str(rule);
        merged.push('\n');
    }
    if fs::write_atomic(&path, merged.as_bytes()).is_err() {
        return unavailable(ExcludeFailure::Unwritable);
    }
    ExcludeOutcome {
        appended: missing,
        unavailable: None,
    }
}

/// Which of `candidates` the repository containing `root` ALREADY tracks, in
/// the order git reports them.
///
/// This is the half an exclude rule cannot cover: git applies ignore rules only
/// to untracked paths, so a host repository that already versions (say) its own
/// `CLAUDE.md` keeps reporting it forever, no matter how many rules exist. The
/// answer is residue for the operator to clear with `git rm --cached` if they
/// choose — this function reads the index and changes nothing.
///
/// Fail-open: no git, no repository, or a git that refuses the pathspec yields
/// an empty list — "we could not prove anything is tracked", which is the safe
/// direction for a function whose output is a warning.
#[must_use]
pub fn tracked_paths(root: &Path, candidates: &[String]) -> Vec<String> {
    if candidates.is_empty() {
        return Vec::new();
    }
    // `-z` because a path may contain a character git would otherwise quote,
    // and a quoted name would not match the candidate it came from.
    let mut args: Vec<&str> = vec!["ls-files", "-z", "--"];
    args.extend(candidates.iter().map(String::as_str));
    let Some(out) = git_stdout(root, &args) else {
        return Vec::new();
    };
    out.split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The rules `existing` does not already carry, in the supplied order and
/// without repeats. Comments and blank lines on either side are layout and are
/// skipped; the comparison is on the trimmed line, so a CRLF file answers the
/// same as an LF one.
fn missing_rules(existing: &str, rules: &[String]) -> Vec<String> {
    let is_rule = |line: &&str| !line.is_empty() && !line.starts_with('#');
    let present: Vec<&str> = existing.lines().map(str::trim).filter(is_rule).collect();
    let mut missing: Vec<String> = Vec::new();
    for rule in rules {
        let rule = rule.trim();
        if !is_rule(&rule) {
            continue;
        }
        if !present.contains(&rule) && !missing.iter().any(|m| m == rule) {
            missing.push(rule.to_string());
        }
    }
    missing
}

/// Run `git <args>` in `dir` and return the trimmed stdout, or `None` on any
/// failure — git absent, not a repository, a non-zero exit, non-UTF-8 output,
/// or nothing printed at all.
///
/// The fail-open seam of this module, and the sibling of
/// `io::workspace::git_rev_parse` (private there, and `rev-parse`-only; this
/// one also has to run `ls-files`).
fn git_stdout(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as std_fs;
    use tempfile::tempdir;

    /// A fresh repository with one committed file — the shape every test here
    /// needs before git will answer anything.
    fn init_repo(root: &Path) {
        for args in [
            vec!["init"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
        ] {
            let ok = Command::new("git")
                .args(&args)
                .current_dir(root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed");
        }
    }

    fn rules(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn exclude_file_is_resolved_by_git_not_spelled_by_us() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let resolved = exclude_file(dir.path()).expect("git resolves the exclude path");
        assert!(resolved.is_absolute(), "a relative answer is joined: {resolved:?}");
        assert!(
            resolved.ends_with("info/exclude") || resolved.ends_with("info\\exclude"),
            "git pointed somewhere unexpected: {resolved:?}",
        );
    }

    #[test]
    fn append_is_idempotent_and_keeps_what_was_there() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let path = exclude_file(dir.path()).unwrap();
        std_fs::create_dir_all(path.parent().unwrap()).unwrap();
        std_fs::write(&path, "# theirs\nbuild/\n").unwrap();

        let first = ensure_excluded(dir.path(), &rules(&["build/", ".claude/", "mustard.json"]));

        assert_eq!(first.appended, vec![".claude/", "mustard.json"], "already-present rule skipped");
        assert_eq!(first.unavailable, None);
        let body = std_fs::read_to_string(&path).unwrap();
        assert!(body.contains("# theirs"), "their comment survives: {body}");
        assert_eq!(body.lines().filter(|l| l.trim() == "build/").count(), 1, "{body}");

        let second = ensure_excluded(dir.path(), &rules(&["build/", ".claude/", "mustard.json"]));
        assert!(second.appended.is_empty(), "converged: {second:?}");
        assert_eq!(std_fs::read_to_string(&path).unwrap(), body, "…byte for byte");
    }

    #[test]
    fn a_tree_without_a_repository_reports_instead_of_failing() {
        let dir = tempdir().unwrap();
        let outcome = ensure_excluded(dir.path(), &rules(&["mustard.json"]));
        assert!(outcome.appended.is_empty());
        assert_eq!(outcome.unavailable, Some(ExcludeFailure::NoRepository));
        assert!(
            !ExcludeFailure::NoRepository.is_blocking(),
            "there is no repository here, so there is nothing a footprint could be visible to",
        );
        // …and the tracked question degrades the same way, to "nothing proven".
        assert!(tracked_paths(dir.path(), &rules(&["mustard.json"])).is_empty());
    }

    /// The half a private install refuses on: git resolved an exclude file and
    /// the write still did not land. Measured in a REAL repository, because the
    /// distinction is precisely "was there a repository to hide from".
    ///
    /// The unwritable case is provoked by making the exclude path a DIRECTORY —
    /// portable, unlike a permission bit, which a Windows administrator and a
    /// POSIX root both walk straight through.
    #[test]
    fn a_repository_we_cannot_hide_in_is_a_blocking_failure() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let path = exclude_file(dir.path()).unwrap();
        if path.exists() {
            std_fs::remove_file(&path).unwrap();
        }
        std_fs::create_dir_all(&path).unwrap();

        let outcome = ensure_excluded(dir.path(), &rules(&["mustard.json"]));

        assert!(outcome.appended.is_empty(), "nothing was excluded: {outcome:?}");
        let failure = outcome.unavailable.expect("a real repository refused the write");
        assert!(
            failure.is_blocking(),
            "{failure:?} happened INSIDE a repository — a caller that proceeds now lays its \
             footprint down visibly while believing it is hidden",
        );
    }

    #[test]
    fn tracked_paths_names_only_what_the_index_holds() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_repo(root);
        std_fs::write(root.join("CLAUDE.md"), "theirs\n").unwrap();
        std_fs::write(root.join("mustard.json"), "{}\n").unwrap();
        let ok = Command::new("git")
            .args(["add", "CLAUDE.md"])
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git add failed");

        let tracked = tracked_paths(root, &rules(&["CLAUDE.md", "mustard.json", ".claude/"]));

        assert_eq!(tracked, vec!["CLAUDE.md"], "only the indexed path is residue");
    }
}
