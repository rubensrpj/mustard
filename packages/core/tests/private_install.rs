//! A private install must leave the host repository's git with nothing to
//! report — and a shared install must be untouched by the mode existing.
//!
//! ## Why this file asks git instead of reading the code
//!
//! Every claim here is about what a REPOSITORY sees, and none of it can be
//! proven by inspecting a constant. Whether a rule hides a path depends on where
//! the exclude file sits and whether the path is already in the index; whether
//! the exclude file is even reachable depends on the repository's shape, because
//! in a submodule and in a linked worktree `.git` is a pointer FILE and the
//! literal `.git/info/exclude` does not exist. So each test seeds a real
//! repository in a temporary directory, runs the real install, and asks git —
//! the same shape `seeded_ignore.rs` uses for the seeded ignore list.
//!
//! The exclude file's location is never spelled out below either: the tests
//! resolve it exactly as the code does, through
//! `git rev-parse --git-path info/exclude`. A test that hard-coded the path
//! would pass in an ordinary repository while the mechanism it guards was
//! silently writing nowhere in the two shapes that matter most.
//!
//! ## The four criteria
//!
//! - AC-1 — the rules land in the clone-local exclude file, idempotently.
//! - AC-2 — the settings land on the untracked local layer, and the shared
//!   settings file is never created.
//! - AC-3 — a footprint path the host ALREADY tracks is named as residue, and
//!   nothing is unlinked.
//! - AC-4 — the regression guard: a shared install writes the same paths and
//!   the same bytes it wrote before the mode existed.

#[cfg(unix)]
// Unix-only: the AC-11 fixture seals a directory with mode 0o555, an API and a
// semantic Windows does not have (an NTFS read-only directory still accepts new
// files, so the same seal would refuse nothing there).
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::{
    footprint_rules, upsert_project, InstallMode, CLAUDE_GITIGNORE, ORCHESTRATOR_MD, SETTINGS_SEED,
};

// ---------------------------------------------------------------------------
// AC-1 — the clone-local exclude file
// ---------------------------------------------------------------------------

#[test]
fn ac1_private_upsert_writes_clone_local_exclude() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);
    let exclude = exclude_file(root);

    let report = upsert_project(root, Some("9.9.9"), InstallMode::Private).expect("upsert");

    assert!(report.private, "the run must declare itself private");
    assert_eq!(report.exclude_unavailable, None, "git was available: {report:?}");
    assert_eq!(
        report.excluded,
        footprint_rules(),
        "a repository nobody excluded anything in yet receives the whole footprint",
    );

    // Every rule is really IN the file git pointed at — not in a path we chose.
    let body = read(&exclude).expect("git resolved an exclude file that exists");
    for rule in footprint_rules() {
        assert!(
            body.lines().any(|l| l.trim() == rule),
            "rule {rule:?} never reached {exclude:?}: {body}",
        );
    }

    // The rules WORK: the four seeds the install just wrote are invisible.
    assert_eq!(
        git_status(root),
        "",
        "the install wrote its footprint and git must see none of it",
    );

    // …and they work on OUR footprint ONLY. Asked of real git, because the
    // failure this pins is a property of gitignore syntax that no reading of the
    // rule list reveals: a pattern carrying no slash matches at EVERY depth, so a
    // bare `mustard.json` or `CLAUDE.md` line silently swallowed files the CLIENT
    // authored. A criterion that only demands a CLEAN status cannot see it —
    // an over-broad rule makes that assertion MORE likely to pass, not less.
    write(&root.join("CLAUDE.md"), "# the client's own rules\n");
    write(&root.join("services/billing/CLAUDE.md"), "# their subproject\n");
    write(&root.join("services/billing/mustard.json"), "{}\n");
    let status = git_status(root);
    for theirs in ["CLAUDE.md", "services/billing/CLAUDE.md", "services/billing/mustard.json"] {
        assert!(
            status.contains(theirs),
            "the install hid {theirs}, which it never wrote — under a `git add -A` law \
             that file would silently never be committed: {status:?}",
        );
    }

    // Idempotent: a second run adds nothing and changes not one byte.
    let second = upsert_project(root, Some("9.9.9"), InstallMode::Private).expect("upsert");
    assert!(second.excluded.is_empty(), "a second run appends nothing: {second:?}");
    assert_eq!(read(&exclude), Some(body), "…and the file is byte-identical");
}

// ---------------------------------------------------------------------------
// AC-2 — the local settings layer
// ---------------------------------------------------------------------------

#[test]
fn ac2_private_upsert_seeds_local_settings() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);

    let report = upsert_project(root, Some("9.9.9"), InstallMode::Private).expect("upsert");

    let local = root.join(".claude/settings.local.json");
    let shared = root.join(".claude/settings.json");
    assert!(local.is_file(), "the harness settings must land on the local layer");
    assert!(
        !shared.exists(),
        "a private install must never create the file the host repository versions",
    );

    // It is the real seed, not an empty placeholder.
    let seeded: serde_json::Value =
        serde_json::from_str(&read(&local).unwrap_or_default()).expect("valid JSON");
    assert!(seeded.get("statusLine").is_some(), "real seed content: {seeded}");
    assert!(seeded.get("permissions").is_some(), "real seed content: {seeded}");

    // And the report names the file that was actually written.
    assert!(
        report.created.contains(&".claude/settings.local.json".to_string()),
        "report names the local layer: {report:?}",
    );
    assert!(
        !report.created.contains(&".claude/settings.json".to_string()),
        "report must not claim a file nobody wrote: {report:?}",
    );
}

// ---------------------------------------------------------------------------
// AC-3 — already-tracked residue
// ---------------------------------------------------------------------------

#[test]
fn ac3_already_tracked_paths_are_reported_not_unlinked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);
    // The host's OWN instruction file, versioned before Mustard ever arrived.
    // An ignore rule is powerless against it — that is the whole point of the
    // report.
    let host_md = root.join("CLAUDE.md");
    write(&host_md, "# the client's own rules\n");
    git(root, &["add", "CLAUDE.md"]);
    git(root, &["commit", "-m", "their file"]);
    let before = read(&host_md);

    let report = upsert_project(root, Some("9.9.9"), InstallMode::Private).expect("upsert");

    assert!(
        report.already_tracked.contains(&"CLAUDE.md".to_string()),
        "the residue no rule can hide must be named: {report:?}",
    );

    // Nothing was unlinked: the path is still in the index, with its own bytes.
    assert!(
        git_out(root, &["ls-files"]).lines().any(|l| l.trim() == "CLAUDE.md"),
        "`git rm --cached` rewrites the host's index — that is the operator's call",
    );
    assert_eq!(read(&host_md), before, "the client's file is byte-identical");
    assert_eq!(
        git_status(root),
        "",
        "a tracked-but-untouched file plus an excluded footprint is a clean tree",
    );
}

// ---------------------------------------------------------------------------
// AC-4 — the regression guard
// ---------------------------------------------------------------------------

#[test]
fn ac4_shared_install_is_byte_identical_to_today() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);
    let exclude = exclude_file(root);
    let exclude_before = read(&exclude);

    let report = upsert_project(root, Some("9.9.9"), InstallMode::Shared).expect("upsert");

    // 1. The same four paths, in the same order, as before the mode existed.
    assert_eq!(
        report.created,
        vec![
            ".claude/settings.json",
            ".claude/mustard/orchestrator.md",
            ".claude/.gitignore",
            "mustard.json",
        ],
    );

    // 2. The same bytes. The settings file is the parsed seed re-rendered, which
    //    is what the engine has always written — computed here rather than
    //    quoted so the comparison stays honest if the template changes.
    let seed: serde_json::Value = serde_json::from_str(SETTINGS_SEED).expect("seed is JSON");
    let expected_settings = format!(
        "{}\n",
        serde_json::to_string_pretty(&seed).expect("re-render the seed"),
    );
    assert_eq!(read(&root.join(".claude/settings.json")), Some(expected_settings));
    assert_eq!(
        read(&root.join(".claude/mustard/orchestrator.md")),
        Some(ORCHESTRATOR_MD.to_string()),
    );
    assert_eq!(read(&root.join(".claude/.gitignore")), Some(CLAUDE_GITIGNORE.to_string()));
    assert!(root.join("mustard.json").is_file(), "the project config is written");

    // 3. Nothing of the private mode happened: no local layer, no exclude write,
    //    and the report carries no private key at all.
    assert!(
        !root.join(".claude/settings.local.json").exists(),
        "the local layer belongs to the private mode only",
    );
    assert_eq!(read(&exclude), exclude_before, "the exclude file was not touched");
    assert!(!report.private);
    assert!(report.excluded.is_empty());
    assert!(report.already_tracked.is_empty());
    assert_eq!(report.exclude_unavailable, None);
    let json = serde_json::to_string_pretty(&report).expect("serialize");
    for key in ["private", "excluded", "alreadyTracked", "excludeUnavailable"] {
        assert!(!json.contains(key), "{key} leaked into a shared report: {json}");
    }

    // 4. The negative control that makes the three assertions above mean
    //    something: a shared footprint IS visible to git. If this were empty the
    //    test would pass while the modes had silently become the same thing.
    let status = git_status(root);
    assert!(status.contains("mustard.json"), "a shared install is versionable: {status:?}");
}

// ---------------------------------------------------------------------------
// AC-11 — the one failure that must not be narrated away
// ---------------------------------------------------------------------------

/// A private install that cannot hide REFUSES, and writes nothing.
///
/// Every other degradation in this engine costs a feature. This one costs the
/// operator's belief: they asked for an install the client's git cannot see, and
/// a quiet `excludeUnavailable` line would let all four seeds land VISIBLY under
/// exactly that belief — in a repository they do not own, under a `git add -A`
/// law. It is also the failure they cannot notice for themselves, because a
/// successful private install and a failed one look identical until the commit.
///
/// "Cannot hide" means a repository that EXISTS refused the write. A tree with
/// no repository at all is a different thing — there is nobody for a footprint
/// to be visible to — and the second half asserts that it still installs.
///
/// The unwritable exclude file is provoked by sealing its parent directory
/// (mode 0o555), which keeps the rename-into-the-directory failing while the
/// exclude file itself stays readable — so `git status` still answers and the
/// clean-tree assertion below measures something. Unix-only, twice over: the
/// permission API does not exist on Windows, and the equivalent NTFS read-only
/// attribute on a directory does not refuse new files anyway, so the failure
/// cannot be provoked there at all. (A POSIX root would also walk through the
/// seal — the test runs as an ordinary user, which CI is.)
#[test]
#[cfg(unix)]
fn ac11_private_install_refuses_when_it_cannot_hide() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);
    // Make the exclude file UNWRITABLE by sealing its directory, not by turning
    // the path into a directory.
    //
    // The old fixture created `.git/info/exclude` AS a directory, which does
    // make the write fail — and also makes `git status` fail outright: git 2.55
    // answers `fatal: cannot use .git/info/exclude as an exclude file` (exit
    // 128) for the whole command. The last assertion below then measured
    // nothing, because git could not answer at all. Sealing the parent keeps
    // the rename-into-the-directory failing (which is what `write_atomic`
    // does) while leaving a readable exclude file, so `git status` still works
    // and the assertion means what it says.
    let exclude = exclude_file(root);
    let info_dir = exclude.parent().expect("exclude lives in a directory").to_path_buf();
    std::fs::create_dir_all(&info_dir).expect("the info directory exists");
    if !exclude.exists() {
        std::fs::write(&exclude, "").expect("seed an empty exclude file");
    }
    let sealed = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(&info_dir, sealed).expect("seal the info directory");

    let err = upsert_project(root, Some("9.9.9"), InstallMode::Private)
        .expect_err("an install that cannot hide must not report success");
    let rendered = err.to_string();
    assert!(
        rendered.contains("private install cannot hide"),
        "the refusal must name what went wrong: {rendered}",
    );

    // NOTHING was written — not the seeds, not the local layer, not even the
    // directory they live in. A refusal that still left a footprint behind would
    // be the same defect wearing an error message.
    for path in [
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".claude/mustard/orchestrator.md",
        ".claude/.gitignore",
        ".claude",
        "mustard.json",
    ] {
        assert!(!root.join(path).exists(), "the refused install wrote {path}");
    }
    assert_eq!(
        git_status(root),
        "",
        "the host repository must have nothing to report after a refused install",
    );

    // Unseal before the tempdir is dropped: a read-only directory cannot be
    // removed, and the failure would surface far from here.
    let _ = std::fs::set_permissions(&info_dir, std::fs::Permissions::from_mode(0o755));

    // The negative control, and it is what makes the refusal a decision rather
    // than a broken code path: the same repository, with a healthy exclude file,
    // installs. Without it this test would also pass if `--private` had simply
    // stopped working.
    // The seal is already lifted above; the exclude file is an ordinary
    // writable file again, so the control needs no repair beyond that.
    let report = upsert_project(root, Some("9.9.9"), InstallMode::Private)
        .expect("a repository whose exclude file is writable installs privately");
    assert!(report.private, "the control really installed privately: {report:?}");
    assert_eq!(report.exclude_unavailable, None, "…with nothing unavailable: {report:?}");
    assert!(root.join(".claude/settings.local.json").is_file(), "the seeds landed");
    assert_eq!(git_status(root), "", "…and git sees none of them");

    // A tree with NO repository is not this failure: there is nobody a footprint
    // could be visible to, so the install proceeds and only reports the reason.
    let bare = tempfile::tempdir().expect("temp dir");
    let report = upsert_project(bare.path(), Some("9.9.9"), InstallMode::Private)
        .expect("a directory outside any repository still installs");
    assert!(
        report.exclude_unavailable.is_some(),
        "…and says why nothing was excluded: {report:?}",
    );
    assert!(bare.path().join("mustard.json").is_file(), "the seeds landed");
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// A fresh repository with one commit — the state a host repo is in when the
/// operator runs the install.
///
/// `core.autocrlf` is pinned off for the same reason `seeded_ignore.rs` pins
/// it: line-ending translation would report a freshly committed file as
/// modified on a machine with the global setting on, turning an environment
/// preference into a failure of assertions that are about ignore rules.
fn init_repo(root: &Path) {
    git(root, &["init"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "core.autocrlf", "false"]);
    write(&root.join("README.md"), "host repo\n");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

/// The clone-local exclude file, resolved the way the code resolves it — by
/// asking git, never by joining `.git/info/exclude` (which does not exist in a
/// submodule or a linked worktree, where `.git` is a pointer file). A relative
/// answer, which is what an ordinary repository returns, is joined against
/// `root`.
fn exclude_file(root: &Path) -> PathBuf {
    let answer = git_out(root, &["rev-parse", "--git-path", "info/exclude"]);
    let path = PathBuf::from(answer.trim());
    assert!(!path.as_os_str().is_empty(), "git did not resolve an exclude path");
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

/// The repository's dirt, trimmed. `--untracked-files=all` because the default
/// collapses a wholly untracked directory into ONE line, which would let an
/// unhidden footprint file hide behind its parent's name.
fn git_status(root: &Path) -> String {
    git_out(root, &["status", "--porcelain", "--untracked-files=all"])
}

/// Run a git command in `root`, asserting success — test scaffolding only.
fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// A git command's trimmed stdout. A failure returns a sentinel rather than an
/// empty string, so a measurement that did not happen fails the assertion it
/// feeds instead of satisfying it.
fn git_out(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(root).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "<git unavailable>".to_string(),
    }
}

/// `Some(contents)` when the file exists and is readable, `None` when it is
/// absent — the distinction AC-4 needs to say "the exclude file was not
/// touched" whether or not one existed.
fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Write `body` at `path`, creating the parent directories.
fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        assert!(std::fs::create_dir_all(parent).is_ok(), "create {}", parent.display());
    }
    assert!(std::fs::write(path, body).is_ok(), "write {}", path.display());
}
