//! O que um comando `run` resolve do ambiente do processo: a raiz do
//! projeto, a pasta atual, a onda em curso e a pasta de spec que um argumento
//! nomeia. Um comando `run` não recebe a entrada de um gancho.

use mustard_core::io::workspace::{workspace_root, WorkspaceError};
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

/// Resolve the Mustard workspace root by ancestor walk, **failing strictly**
/// on missing anchor.
///
/// This is the strict entry point for run subcommands — unlike enforcement hooks
/// (which fail open via `dispatch::build_ctx`), a `run` subcommand has no
/// useful behaviour without a workspace and must surface the error to the
/// caller. The returned [`PathBuf`] is the directory containing both
/// `mustard.json` and `.claude/`.
///
/// # Errors
///
/// Propagates [`WorkspaceError`] from [`workspace_root`] when no ancestor
/// satisfies the anchor predicate, when the resolved path violates the `.claude/.claude/`
/// `.claude/.claude/` guard, or when `MUSTARD_WORKSPACE_ROOT` is set to an
/// invalid path.
pub fn workspace_root_strict() -> Result<PathBuf, WorkspaceError> {
    let start = if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !dir.is_empty() {
            PathBuf::from(dir)
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    workspace_root(&start)
}

/// The raw process working directory as a `String`, defaulting to `"."`.
///
/// This is the plain `std::env::current_dir()` idiom (NOT the workspace-root
/// walk of [`project_dir`]) — the single home for the `current_dir → String`
/// snippet that the per-command economy emitters used verbatim.
#[must_use]
pub fn cwd() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string())
}

/// Resolve the project directory.
///
/// The single-source move of `.claude` paths made the canonical resolver
/// [`workspace_root_strict`], which fails strictly on a missing anchor.
/// `project_dir` keeps its legacy `String` return shape so the many existing
/// call-sites that bake the value into `current_dir(...)` of a `Command`
/// continue to work, but it now consults [`workspace_root_strict`] first.
///
/// Resolution order:
///
/// 1. [`workspace_root_strict`] — `mustard.json + .claude/` ancestor walk.
/// 2. `CLAUDE_PROJECT_DIR` env var.
/// 3. `std::env::current_dir()`.
/// 4. `"."` as a last resort.
#[must_use]
pub fn project_dir() -> String {
    if let Ok(root) = workspace_root_strict() {
        return root.to_string_lossy().into_owned();
    }
    if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR")
        && !dir.is_empty() {
            return dir;
        }
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string())
}

/// Normalise a `--spec-dir` argument onto the spec DIRECTORY it names.
///
/// The four `--spec-dir` commands (`plan-materialize`, `pipeline-summary`,
/// `wave-tree`, `wave-size-check`) are driven both by hand and by orchestrator
/// prompts, where the argument arrives in three shapes. This is the single home
/// for resolving them, so the four can never drift.
///
/// Precedence (first match wins):
///
/// 1. An existing DIRECTORY — as given (absolute / cwd-relative) or under
///    `project`. Returned unchanged: today's behaviour, preserved exactly.
/// 2. A path that names a FILE (`…/spec.md`) — resolves to the directory
///    holding it, so pointing at the spec markdown works like pointing at the
///    spec.
/// 3. A bare slug whose `.claude/spec/{slug}` exists — resolves to that
///    directory.
///
/// Fail-open: nothing matched → the raw argument as a [`PathBuf`], so each
/// command's own absolutize/join and its existing "not found" message still
/// apply.
#[must_use]
pub fn normalise_spec_dir(project: &Path, raw: &str) -> PathBuf {
    let as_given = PathBuf::from(raw);
    // `join` on an absolute `raw` yields `raw`, so the two candidates collapse
    // to one for an absolute argument.
    let candidates = [as_given.clone(), project.join(raw)];

    // 1. An existing directory wins as-is.
    if let Some(dir) = candidates.iter().find(|c| c.is_dir()) {
        return dir.clone();
    }
    // 2. A path that names a file resolves to its parent. Only an EXISTING file
    //    or an explicit `.md` document qualifies — a bare slug (which has no
    //    extension) must never be mistaken for a filename.
    for candidate in &candidates {
        let names_file = candidate.is_file()
            || candidate
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"));
        if names_file
            && let Some(parent) = candidate.parent().filter(|p| p.is_dir()) {
                return parent.to_path_buf();
            }
    }
    // 3. A bare slug under `.claude/spec/`. `for_spec` rejects any name
    //    carrying a separator or a traversal, so a real path never lands here
    //    by accident.
    if let Some(dir) = ClaudePaths::for_project(project)
        .and_then(|p| p.for_spec(raw))
        .ok()
        .map(|s| s.dir().to_path_buf())
        .filter(|d| d.is_dir())
    {
        return dir;
    }
    as_given
}

/// Resolve the active wave number from `MUSTARD_ACTIVE_WAVE` — the convention the
/// harness sets on every wave dispatch and that `route` already stamps on each
/// emitted event. O pedido do subagente lê a onda por aqui.
/// `None` when unset / not numeric (a spec-less or non-wave dispatch).
#[must_use]
pub fn current_wave() -> Option<i64> {
    std::env::var("MUSTARD_ACTIVE_WAVE")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// The precedence contract: an existing directory resolves UNCHANGED, a
    /// path ending in a file resolves to its parent, and a bare slug resolves
    /// through `.claude/spec/{slug}`. Anything unresolvable falls back to the
    /// raw argument so each command's own "not found" path still fires.
    #[test]
    fn normalise_spec_dir_resolves_directory_file_and_slug() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("my-spec");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# demo\n").unwrap();

        // 1. An existing directory is returned unchanged (today's behaviour).
        let as_dir = normalise_spec_dir(project, &spec_dir.to_string_lossy());
        assert_eq!(as_dir, spec_dir, "an existing directory resolves unchanged");

        // 2. A path ending in a file resolves to the directory holding it.
        let as_file = normalise_spec_dir(project, &spec_dir.join("spec.md").to_string_lossy());
        assert_eq!(as_file, spec_dir, "`…/spec.md` resolves to its parent");
        // Even when the `.md` does not exist yet (pre-draft), the parent wins.
        let as_missing_md =
            normalise_spec_dir(project, &spec_dir.join("wave-plan.md").to_string_lossy());
        assert_eq!(as_missing_md, spec_dir, "an absent `.md` still resolves to its parent");

        // 3. A bare slug resolves through `.claude/spec/{slug}`.
        assert_eq!(
            normalise_spec_dir(project, "my-spec"),
            spec_dir,
            "a bare slug resolves through .claude/spec/"
        );

        // Fail-open: an unknown slug comes back verbatim.
        assert_eq!(
            normalise_spec_dir(project, "ghost-spec"),
            PathBuf::from("ghost-spec"),
            "an unresolvable argument is returned untouched"
        );
    }
}
