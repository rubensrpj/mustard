//! Laying the files down: the location guard, the private footprint, the
//! narration of each seed, the `templates/` payload beside the binary and the
//! `.github/` scaffolding.
//!
//! The seeding itself lives in the core (`mustard_core::platform::project_seed`);
//! this part only guards, locates and narrates.

use std::path::{Path, PathBuf};

use anyhow::Result;
use mustard_core::platform::project_seed::cleanup::{Action, CleanupPlan};
use mustard_core::SeedOutcome;

use crate::fs_ops::copy_dir;

/// Pre-flight location guard: refuse to init a directory that sits INSIDE a
/// git repository without being that repository's root.
///
/// Why: the workspace resolver (`mustard_core::io::workspace`) anchors on git
/// repository roots. A `mustard.json` + `.claude/` planted in a non-root
/// subdirectory would never win the resolution — it would only sit there as a
/// confusing phantom (the historical monorepo defect this guard closes).
///
/// Rules (filesystem probes only — fail-open, no `git` subprocess):
/// - the target IS a git repository root (`.git` as a directory, or as a file
///   for a submodule / linked worktree) → allow;
/// - the target lies inside a git repository but is not its root → refuse,
///   naming the repository root as the right place to init;
/// - no `.git` anywhere up the tree → allow with a note (projects without git
///   are supported through the resolver's loose fallback).
pub(super) fn guard_init_location(project_path: &Path) -> Result<()> {
    use mustard_core::io::workspace::is_git_repo_root;

    if is_git_repo_root(project_path) {
        return Ok(());
    }
    let enclosing_root = project_path
        .ancestors()
        .skip(1)
        .find(|dir| is_git_repo_root(dir));
    let Some(repo_root) = enclosing_root else {
        println!(
            "  note: no git repository found here or above - proceeding (projects without git are supported)"
        );
        return Ok(());
    };
    anyhow::bail!(
        "this folder is inside a git repository, but it is not the repository's root.\n\
         Mustard anchors its workspace at the root of a git repository, so initializing here\n\
         would leave the harness state in the wrong place.\n\
         \n\
           repository root: {}\n\
         \n\
         Either run `mustard init` from that repository root, or - if this subfolder is meant\n\
         to be its own Mustard project - make it its own git repository (or a git submodule)\n\
         first, then re-run `mustard init` here.",
        repo_root.display()
    )
}

/// Private mode, step 0: hide the Mustard footprint from THIS clone's git, and
/// name whatever the host repository already tracks.
///
/// Mirrors the private step of `mustard_core::upsert_project` so the two
/// install faces never drift: the rules are [`mustard_core::footprint_rules`],
/// the residue question is asked with [`mustard_core::footprint_pathspecs`] (the
/// two are NOT the same list — a rule is a pattern, a pathspec is a path), the
/// write goes through the clone-local exclude layer (a path git resolves — never
/// the literal `.git/info/exclude`, which does not exist in a submodule or a
/// linked worktree), and an already-tracked path is REPORTED, never unlinked:
/// `git rm --cached` rewrites the host's index, and that is the operator's
/// decision, not an install-time cosmetic.
///
/// The residue report is SPLIT, and that split is the difference between advice
/// and damage. `git rm --cached` clears a file the install put there; aimed at
/// the client's own `CLAUDE.md` — which a private install never writes, because
/// the Guards go to `CLAUDE.local.md` beside it — the same command untracks
/// THEIR work, and their next commit deletes it. So only a path
/// [`mustard_core::is_written_footprint`] recognises is offered the command; the
/// host's own file is named for what it is and left alone.
///
/// One failure here is NOT narrated away, and it is the reason this function
/// returns a `Result` at all: when git resolved an exclude file in a real
/// repository and the write still did not land, the install refuses. Everything
/// after this point would then be written VISIBLY into a repository the operator
/// believes cannot see it — the one outcome this mode exists to prevent, and the
/// one an operator cannot notice for themselves. A tree with no repository is a
/// different thing entirely (there is nobody for a footprint to be visible to)
/// and still degrades to a printed line.
///
/// # Errors
///
/// [`mustard_core::ExcludeFailure::is_blocking`] — the exclude file could not be
/// read or written inside a repository that exists.
pub(super) fn hide_footprint(project_path: &Path) -> Result<()> {
    let outcome = mustard_core::ensure_excluded(project_path, &mustard_core::footprint_rules());
    match (outcome.unavailable, outcome.appended.len()) {
        (Some(failure), _) if failure.is_blocking() => anyhow::bail!(
            "a private install must not write anything it cannot hide.\n\
             \n\
               {}\n\
             \n\
             Nothing was written. This clone's exclude file is where the footprint is hidden;\n\
             until it can be read and written, every file `mustard init` seeds would be visible\n\
             in this repository's `git status` while the install reported itself private.\n\
             Fix the file's permissions (or its type — it must be a FILE) and re-run.",
            failure.reason(),
        ),
        (Some(failure), _) => println!("  private install: {}", failure.reason()),
        (None, 0) => {
            println!("  private install: this clone's exclude file already carries every rule");
        }
        (None, count) => println!(
            "  private install: hid {count} path(s) from this clone's git (exclude file, never committed)"
        ),
    }

    let tracked =
        mustard_core::tracked_paths(project_path, &mustard_core::footprint_pathspecs());
    let (ours, theirs): (Vec<String>, Vec<String>) = tracked
        .into_iter()
        .partition(|path| mustard_core::is_written_footprint(path));
    if !ours.is_empty() {
        println!(
            "  note: this repository ALREADY tracks {} — a git ignore rule cannot hide a tracked path,",
            ours.join(", ")
        );
        println!("        so those stay visible. Nothing was unlinked; clear them yourself with:");
        println!("          git rm --cached {}", ours.join(" "));
    }
    for path in theirs {
        println!(
            "  note: {path} is the repository's OWN versioned file — a private install never \
             writes it, so it is left exactly as it is and no rule of ours hides it."
        );
    }
    Ok(())
}

/// Print one didactic line per seeded file. The seeding itself lives in the
/// core (`mustard_core::platform::project_seed`) — the CLI only narrates:
/// `Created`/`Updated` announce a write, `Preserved` confirms the user's file
/// survived the merge untouched.
pub(super) fn report_seed(name: &str, outcome: SeedOutcome) {
    match outcome {
        SeedOutcome::Created | SeedOutcome::Updated => println!("  wrote {name}"),
        SeedOutcome::Preserved => println!("  kept {name} (yours, unchanged)"),
    }
}

/// Print one line per `mustard.json#inject` migration the core performed
/// (`mustard_core::migrate_inject_declarations`).
pub(super) fn report_migration(migrated: &[String]) {
    for entry in migrated {
        println!("  migrated {entry}");
    }
}

/// Print what an older Mustard left in files that are not its own, and how to
/// take it out. Nothing is taken out here: the list goes through the plugin's
/// door, where the person says yes to exactly that list.
pub(super) fn report_cleanup(plan: &CleanupPlan) {
    if plan.is_empty() {
        return;
    }
    if !plan.files.is_empty() {
        println!("  an older Mustard left lines in files that are not its own (nothing was removed):");
        for change in &plan.files {
            let what = match change.action {
                Action::Edit => "remove",
                Action::Delete => "delete",
            };
            println!("    {what} {}: {}", change.path, change.removes.join("; "));
        }
    }
    for path in &plan.unmarked {
        println!("  note: {path} carries traces of an older scan without its marks — left for you to decide");
    }
    if plan.has_changes() {
        println!("  to take them out, run /mustard:upsert inside Claude Code and confirm the list;");
        println!("  the Guards that leave become project-rule lessons, and the commit is yours.");
    }
}

/// The `templates/` payload shipped beside `exe`, if there is one.
///
/// Covers both installed layouts: the payload in the binary's OWN directory
/// (the macOS `.pkg`, which puts CLI + payload inside `.app/Contents/MacOS`)
/// and one level up (the `.deb`, whose binaries live in `/usr/lib/mustard/bin`
/// next to `/usr/lib/mustard/templates`).
///
/// `exe` must be the CANONICAL executable path — a symlink's own directory
/// holds no payload; see [`resolve_templates_dir`]. Kept pure (nothing but
/// `is_dir` probes, no process env) so a test can drive it with a real symlink
/// instead of reasoning about the platform — see
/// `templates_resolve_through_a_symlinked_exe`.
fn templates_beside_exe(exe: &Path) -> Option<PathBuf> {
    let exe_dir = exe.parent()?;
    [exe_dir.join("templates"), exe_dir.join("../templates")]
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

/// Resolve the bundled `templates/` directory.
///
/// Resolution order:
/// 1. the `MUSTARD_TEMPLATES_DIR` environment variable (explicit override —
///    used by tests and by any caller that knows its own layout);
/// 2. `<exe-dir>/templates` and `<exe-dir>/../templates` (installed layout),
///    resolved from the CANONICALIZED executable path;
/// 3. `<CARGO_MANIFEST_DIR>/templates` (the in-repo layout, for `cargo run`).
///
/// Step 2 canonicalizes because `current_exe` promises nothing about symlinks:
/// the std docs state that some platforms return the path of the symlink and
/// others the path of its target, and on macOS the underlying
/// `_NSGetExecutablePath` is documented (dyld(3)) to return "a path", not "a
/// real path". Every installed layout ships the payload beside the REAL binary
/// and exposes symlinks on `PATH` — `/usr/local/bin` → inside the `.app`
/// (macOS), `/usr/bin` → `/usr/lib/mustard/bin` (Linux). Without
/// canonicalizing, `mustard init` invoked by name probes the LINK's directory
/// and never reaches the payload: that is precisely how macOS broke while
/// Linux, whose `/proc/self/exe` is pre-resolved by the kernel, did not.
pub(super) fn resolve_templates_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("MUSTARD_TEMPLATES_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Ok(path);
        }
    }

    let exe = std::env::current_exe().ok();
    if let Some(exe) = exe.as_deref() {
        // Fail-open: an unresolvable path degrades to the original, never to an
        // error — resolution must not become more brittle than it was.
        let real = match std::fs::canonicalize(exe) {
            Ok(real) => real,
            Err(_) => exe.to_path_buf(),
        };
        if let Some(found) = templates_beside_exe(&real) {
            return Ok(found);
        }
        // Safety net: the pre-canonical path is still probed, so a canonical
        // form that points somewhere unhelpful can never resolve LESS than the
        // previous behaviour did.
        if real.as_path() != exe
            && let Some(found) = templates_beside_exe(exe) {
                return Ok(found);
            }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
    if manifest.is_dir() {
        return Ok(manifest);
    }

    // Name what was probed: the bare "set the env var" hint left the reader with
    // no way to tell an unpackaged binary from a payload that IS installed but
    // sits beside the symlink's target rather than the symlink.
    let exe_display = exe
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    anyhow::bail!(
        "could not locate the Mustard `templates/` directory.\n\
         Probed next to the running binary ({exe_display}) and at the \
         compile-time path ({}).\n\
         An installed Mustard ships the payload beside the REAL binary, not \
         beside the symlink on PATH; set MUSTARD_TEMPLATES_DIR to override.",
        manifest.display(),
    )
}

/// Copy `templates/.github/` → `<project>/.github/` when the project has a
/// GitHub remote. Never overwrites — user customisations win. Returns the
/// number of files copied (0 when there is no `.github` payload or no remote).
pub(super) fn install_github_templates(templates_dir: &Path, project_path: &Path) -> Result<usize> {
    let src = templates_dir.join(".github");
    if !src.is_dir() || !has_github_remote(project_path) {
        return Ok(0);
    }
    copy_dir(&src, &project_path.join(".github"), false, &[])
}

/// Whether `origin`'s URL points at github.com.
fn has_github_remote(project_path: &Path) -> bool {
    mustard_core::platform::git::run(project_path, &["config", "--get", "remote.origin.url"])
        .out()
        .is_some_and(|url| url.to_lowercase().contains("github.com"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Regression guard (2026-06-03): the legacy per-subproject guards file
    /// `.claude/commands/guards.md` (and its `patterns.md` companion) is
    /// OBSOLETE. No shipped template may point an agent at those non-existent
    /// paths. Walks the REAL bundled `templates/` payloads — the CLI's own
    /// tree AND the core seed tree (`packages/core/templates/`, where the
    /// harness seeds moved) — and fails if the obsolete path is reintroduced.
    #[test]
    fn templates_never_reference_obsolete_guards_file() {
        let templates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        assert!(
            templates.is_dir(),
            "templates payload missing at {}",
            templates.display()
        );
        let core_templates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/core/templates");
        assert!(
            core_templates.is_dir(),
            "core seed payload missing at {}",
            core_templates.display()
        );

        const FORBIDDEN: [&str; 2] = ["commands/guards.md", "commands/patterns.md"];
        let mut offenders: Vec<String> = Vec::new();

        // Iterative directory walk — no external crate.
        let mut stack = vec![templates.clone(), core_templates];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(bytes) = fs::read(&path) else {
                    continue;
                };
                let text = String::from_utf8_lossy(&bytes);
                for needle in FORBIDDEN {
                    if text.contains(needle) {
                        offenders.push(format!("{} → {needle}", path.display()));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "templates must not reference the obsolete standalone guards file:\n{}",
            offenders.join("\n")
        );
    }

    /// Both installed layouts resolve, and an unpackaged binary resolves to
    /// nothing. Runs everywhere — the symlink half of the contract needs a
    /// privilege Windows may withhold, so it lives in the `#[cfg(unix)]` test
    /// below; this one keeps the candidate list itself covered on every host.
    #[test]
    fn templates_beside_exe_covers_both_installed_layouts() {
        let dir = tempdir().unwrap();

        // `.pkg` layout: payload in the binary's OWN directory.
        let pkg = dir.path().join("pkg");
        fs::create_dir_all(pkg.join("templates")).unwrap();
        assert!(templates_beside_exe(&pkg.join("mustard")).is_some());

        // `.deb` layout: payload one level up from the binary's directory.
        let deb = dir.path().join("deb");
        fs::create_dir_all(deb.join("bin")).unwrap();
        fs::create_dir_all(deb.join("templates")).unwrap();
        assert!(templates_beside_exe(&deb.join("bin/mustard")).is_some());

        // Neither: a bare binary with no payload anywhere near it.
        let bare = dir.path().join("bare/bin");
        fs::create_dir_all(&bare).unwrap();
        assert!(templates_beside_exe(&bare.join("mustard")).is_none());
    }

    /// Regression guard (2026-07-29): `mustard init` on macOS died with
    /// "could not locate the Mustard `templates/` directory". The `.pkg`
    /// installs the real binary + payload inside the `.app` and exposes
    /// `/usr/local/bin/mustard` as a SYMLINK — and `current_exe` is not
    /// required to resolve symlinks (on macOS `_NSGetExecutablePath` returns
    /// "a path", not "a real path", per dyld(3)). Probing the LINK's own
    /// directory therefore finds nothing, which is why the resolution
    /// canonicalizes first.
    ///
    /// Unix-only: creating a symlink on Windows needs a privilege the test
    /// host may not hold. The candidate list itself is covered on every host
    /// by `templates_beside_exe_covers_both_installed_layouts`.
    #[cfg(unix)]
    #[test]
    fn templates_resolve_through_a_symlinked_exe() {
        let dir = tempdir().unwrap();
        let real_bin = dir.path().join("real/bin");
        let link_dir = dir.path().join("link");
        fs::create_dir_all(real_bin.join("templates")).unwrap();
        fs::create_dir_all(&link_dir).unwrap();

        let real_exe = real_bin.join("mustard");
        fs::write(&real_exe, "").unwrap();
        let link_exe = link_dir.join("mustard");
        std::os::unix::fs::symlink(&real_exe, &link_exe).unwrap();

        // The defect, stated as an assertion: the symlink's own directory
        // holds no payload, so probing it (the pre-fix behaviour) finds
        // nothing. If this ever passes, the fixture stopped reproducing.
        assert!(
            templates_beside_exe(&link_exe).is_none(),
            "fixture broken: the symlink's directory must not hold a payload",
        );

        // The fix: canonicalize, then probe — the payload beside the TARGET.
        let canonical = fs::canonicalize(&link_exe).unwrap();
        let found = templates_beside_exe(&canonical)
            .expect("templates/ beside the symlink target must resolve");
        assert_eq!(
            fs::canonicalize(found).unwrap(),
            fs::canonicalize(real_bin.join("templates")).unwrap(),
        );
    }
}
