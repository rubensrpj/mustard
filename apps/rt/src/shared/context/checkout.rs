//! A spec em que o checkout está: a da branch atual, quando existe a pasta
//! dela, e a spec atual para quem não tem sessão em mãos.

use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use std::path::Path;

/// Resolve the name of the current spec for a caller with no session in hand,
/// fail-open `None`: the `MUSTARD_ACTIVE_SPEC` override, then the spec of the
/// branch the checkout stands on ([`spec_of_checkout_branch`]). It is the one
/// ladder of [`crate::shared::spec_state::active_spec`] without its session
/// rung. A leftover `.pipeline-states/` file names nothing.
#[must_use]
pub fn current_spec(project_dir_path: &str) -> Option<String> {
    crate::shared::spec_state::active_spec(project_dir_path, None)
}

/// The unit the CHECKOUT is standing on: the slug of the current branch, when a
/// spec directory of that name exists. `None` on an integration base, a
/// hand-cut branch, or a slug with no spec directory.
///
/// **Why the branch outranks the leftover state file.** Every unit is cut as a
/// `{kind}/{slug}` branch — the branch IS the isolation. A directory under
/// `.claude/spec/` is only the residue of a unit that once existed, and a
/// `.pipeline-states/*.json` is the residue of one that once ran. Measured in
/// the field: for a whole session the boundary gate warned on every single edit
/// naming `contrato-plano-fixo-nasce-com`, and the prompt banner said the same
/// pipeline was in flight — while `active-specs` listed a different unit, and
/// the checkout was on that other unit's branch. Dozens of warnings, all
/// pointing at the wrong target. A gate that is wrong every time is worse than
/// no gate, because the operator learns to skip the one day it is right.
///
/// Reads `.git/HEAD` directly rather than spawning `git`: this runs inside a
/// PreToolUse hook, once per Write/Edit, and a subprocess per file edit is a
/// cost the answer does not justify. No `git` runs at all: in a linked
/// worktree, where `.git` is a file, the HEAD is read through its `gitdir`,
/// by the same reading `workspace::checkout_git_dir` gives every caller,
/// and the main checkout — where the `mustard.json` and the spec folders
/// live — is read from the worktree's files by
/// `workspace::linked_worktree_main`. Fail-open at every step.
#[must_use]
pub fn spec_of_checkout_branch(project_dir_path: &str) -> Option<String> {
    let project = Path::new(project_dir_path);
    let git_dir = mustard_core::io::workspace::checkout_git_dir(project)?;
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?.trim();
    if branch.is_empty() {
        return None;
    }
    // In a linked worktree, the Mustard lives in the main checkout: the
    // `mustard.json` and the spec folders stay outside git. Only a `.git` that
    // is a file can be a worktree, and only then are its files read.
    let home = (git_dir != project.join(".git"))
        .then(|| mustard_core::io::workspace::linked_worktree_main(project))
        .flatten()
        .unwrap_or_else(|| project.to_path_buf());
    let config_root = if mustard_core::ProjectConfig::exists(project) { project } else { home.as_path() };
    let config = mustard_core::ProjectConfig::load(config_root);
    let slug = crate::shared::work_kind::BaseFlow::of_at(&config.git, project).slug_of(branch)?;
    [project, home.as_path()]
        .into_iter()
        .any(|root| {
            ClaudePaths::for_project(root)
                .and_then(|p| p.for_spec(&slug))
                .is_ok_and(|sp| sp.dir().exists())
        })
        .then_some(slug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn current_spec_returns_none_when_no_states_dir() {
        // A nonexistent project path → no pipeline-states dir → None.
        let result = current_spec("/nonexistent-mustard-test-path-xyzzy");
        // Either None (env var not set in CI) or Some(...) if MUSTARD_ACTIVE_SPEC
        // happens to be set — just assert it doesn't panic.
        let _ = result;
    }

    #[test]
    fn a_leftover_pipeline_state_file_no_longer_names_the_current_spec() {
        // An inherited override answers first by design; the leftover file is
        // what is under test, so skip rather than depend on the shell.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(claude.join(".pipeline-states")).unwrap();
        std::fs::write(claude.join(".pipeline-states").join("my-feature-xyzzy.json"), "{}").unwrap();
        std::fs::create_dir_all(claude.join("spec").join("my-feature-xyzzy")).unwrap();

        let root = dir.path().to_str().unwrap();
        assert_eq!(current_spec(root), None, "the leftover file names nothing");
        assert_eq!(crate::shared::spec_state::active_spec(root, Some("s-unbound")), None);

        // The branch the checkout stands on still does.
        crate::shared::spec_state::stand_on_spec_branch(dir.path(), "on-the-branch");
        assert_eq!(current_spec(root).as_deref(), Some("on-the-branch"));
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = mustard_core::platform::git::run(dir, args);
        assert!(out.ok, "git {args:?}: {}", out.stderr);
    }

    /// The separate copy of a wave is a linked worktree in a folder outside
    /// the project, whose `.git` file points to its own git folder: the spec
    /// it stands on is the one of the copy's branch, read through that
    /// pointer, with the spec folder in the main checkout. The main checkout,
    /// on the integration base, stands on none.
    #[test]
    fn a_separate_copy_names_the_spec_of_its_own_branch() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("projeto");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(main.join("README.md"), "oi\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "init"]);
        std::fs::write(main.join("mustard.json"), "{}").unwrap();
        std::fs::create_dir_all(main.join(".claude").join("spec").join("da-copia")).unwrap();
        let copy = dir.path().join("cache").join("copias").join("projeto-0123abcd").join("da-copia").join("a");
        git(&main, &["worktree", "add", "-q", &copy.to_string_lossy(), "-b", "feature/da-copia"]);
        assert!(copy.join(".git").is_file(), "the copy is a real linked worktree");
        assert!(!copy.join(".claude").exists(), "the copy carries no spec folder");

        assert_eq!(spec_of_checkout_branch(&copy.to_string_lossy()).as_deref(), Some("da-copia"));
        assert_eq!(spec_of_checkout_branch(&main.to_string_lossy()), None, "the base names no spec");
    }
}
