//! `workspace` — single source of truth for "the Mustard workspace root".
//!
//! ## Why
//!
//! The rt + cli call-sites historically discovered the project root four
//! different ways: `current_dir()`, walking `start_dir.ancestors()`, reading
//! an undocumented env var, or stopping at the first `.git/`. Each gave a
//! subtly different answer in monorepos with nested submodules.
//!
//! [`workspace_root`] replaces all four. Given a `start_dir`, it walks
//! ancestors looking for an **anchor** in two passes:
//!
//! 1. **Strict pass** — a directory that contains `mustard.json` (file),
//!    `.claude/` (directory) **and** is a git repository root (`.git` exists —
//!    as a *directory* for a normal repo, or as a *file* for a submodule /
//!    linked worktree, which carry a `gitdir:` pointer file). The nearest
//!    ancestor satisfying all three is the workspace root.
//! 2. **Loose fallback** — when NO ancestor satisfies the strict predicate
//!    (e.g. a project that uses no git at all), the walk repeats with the
//!    historical rule: `mustard.json` + `.claude/` only.
//!
//! The strict pass exists because the loose rule alone made any directory with
//! a stray committed `mustard.json` + `.claude/` a *phantom anchor*: in a
//! monorepo, harness runtime state was scaffolded inside a subproject's
//! `.claude/` instead of the repo root's. Requiring the git root pins the
//! anchor to the repository boundary while the fallback keeps git-less
//! projects working exactly as before.
//!
//! ## Override
//!
//! `MUSTARD_WORKSPACE_ROOT` short-circuits the walker. The value is the path
//! to use directly; it is validated against the same anchor predicate and the
//! I1 `.claude/.claude/` guard before being accepted.
//!
//! ## Worktree redirect
//!
//! When the resolved anchor sits inside a LINKED git worktree — `git rev-parse
//! --git-dir` differs from `--git-common-dir` — the root is remapped to the
//! MAIN checkout (the parent of the shared `…/.git` common dir). All Mustard
//! state (specs, events, active-spec markers, telemetry) then lands under the
//! primary checkout's `.claude/`, never the worktree's: the worktree carries
//! only code. The redirect is SURGICAL and fail-open — the main checkout,
//! every non-git tree, and any git failure keep the un-redirected walk result,
//! so only a proven linked worktree changes. It applies to the ancestor-walk
//! path only; the `MUSTARD_WORKSPACE_ROOT` override is honoured verbatim.
//!
//! ## Inviolable safety contract
//!
//! - **No cwd fallback.** If no ancestor satisfies the predicate, the function
//!   returns [`WorkspaceError::AnchorNotFound`]. It never silently picks
//!   `start_dir` itself.
//! - **Crosses `.git/` submodule boundaries.** Mustard monorepos commonly
//!   embed `apps/dashboard/src-tauri` with its own `.git/`; the walker steps
//!   straight past such intermediate `.git/` markers.
//! - **No `.claude/.claude/`.** Resolved paths are rejected with
//!   [`WorkspaceError::ForbiddenDotClaudeDotClaude`] if the final segment is
//!   `.claude` or the path contains the sub-sequence `.claude/.claude/`. This
//!   keeps the I1 guard close to the boundary where the path is minted.
//! - **Memoised per process.** Repeated calls with the same
//!   `(start_dir, override_value)` pair return the cached
//!   [`PathBuf`] without re-walking — the canonical resolver lives on the hot
//!   path of every harness event.
//!
//! ## Testing
//!
//! `cargo test` runs in parallel and `std::env::set_var` is **process-global
//! and `unsafe` under Rust 2024** (this crate is `#![forbid(unsafe_code)]`).
//! To keep tests free of `unsafe`, the override is threaded through an
//! internal [`resolve_with_override`] helper that takes the value explicitly;
//! the public [`workspace_root`] reads `MUSTARD_WORKSPACE_ROOT` and delegates.
//! Tests call the helper directly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Errors returned by [`workspace_root`].
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// The walker exhausted all ancestors of `start_dir` without finding a
    /// directory containing both `mustard.json` and `.claude/`.
    #[error("workspace anchor not found searching from {searched_from:?}")]
    AnchorNotFound {
        /// The original `start_dir` the walker began at.
        searched_from: PathBuf,
    },

    /// The resolved path contains the forbidden sub-sequence
    /// `.claude/.claude/` (or terminates in `.claude`). The path is supplied
    /// for diagnostic logging.
    #[error("resolved path contains forbidden .claude/.claude/ sequence: {resolved:?}")]
    ForbiddenDotClaudeDotClaude {
        /// The path that triggered the guard.
        resolved: PathBuf,
    },

    /// The `MUSTARD_WORKSPACE_ROOT` override was set but failed validation.
    #[error("MUSTARD_WORKSPACE_ROOT override invalid ({reason}): {path:?}")]
    OverrideInvalid {
        /// The value of the override env var.
        path: PathBuf,
        /// A short reason ("path does not exist", "anchor not found", …).
        reason: String,
    },
}

/// The env var name that overrides the ancestor walker.
const OVERRIDE_ENV: &str = "MUSTARD_WORKSPACE_ROOT";

/// Memoisation key — the raw `start_dir` (NOT canonicalised) plus the literal value of
/// `MUSTARD_WORKSPACE_ROOT` (or `None` when unset).
type CacheKey = (PathBuf, Option<String>);

/// Process-wide memoisation cache. `OnceLock<Mutex<HashMap<_, _>>>` keeps the
/// implementation std-only and lazily initialises the map on first call.
fn cache() -> &'static Mutex<HashMap<CacheKey, PathBuf>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, PathBuf>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve the Mustard workspace root by ancestor walk from `start_dir`.
///
/// # Errors
///
/// - [`WorkspaceError::ForbiddenDotClaudeDotClaude`] when the resolved root
///   would re-nest `.claude/`.
/// - [`WorkspaceError::AnchorNotFound`] when no ancestor of `start_dir`
///   contains both `mustard.json` and `.claude/`.
/// - [`WorkspaceError::OverrideInvalid`] when `MUSTARD_WORKSPACE_ROOT` is set
///   but the value fails validation.
pub fn workspace_root(start_dir: &Path) -> Result<PathBuf, WorkspaceError> {
    let override_value = std::env::var(OVERRIDE_ENV).ok();
    resolve_with_override(start_dir, override_value.as_deref())
}

/// Like [`workspace_root`] but takes the override value as an explicit
/// argument instead of reading `MUSTARD_WORKSPACE_ROOT`.
///
/// This is the seam tests use to exercise the override path without mutating
/// the process environment (which would require `unsafe`).
///
/// # Errors
///
/// Same as [`workspace_root`].
pub fn resolve_with_override(
    start_dir: &Path,
    override_value: Option<&str>,
) -> Result<PathBuf, WorkspaceError> {
    // Build the cache key from the raw `start_dir` - deliberately NOT
    // canonicalised. Canonicalising here cost a `stat` syscall on EVERY call,
    // cache hits included; the slow-path walker resolves correctly from any
    // spelling, so the key only has to be stable per caller (the raw path is).
    let key: CacheKey = (start_dir.to_path_buf(), override_value.map(str::to_string));

    // Fast path — already cached. Re-validate the I1 guard against the
    // cached value so a stale `.claude/.claude/` answer can never sneak
    // through.
    if let Ok(guard) = cache().lock() {
        if let Some(hit) = guard.get(&key) {
            if violates_dot_claude_guard(hit) {
                return Err(WorkspaceError::ForbiddenDotClaudeDotClaude {
                    resolved: hit.clone(),
                });
            }
            return Ok(hit.clone());
        }
    }

    // Slow path — resolve, validate, memoise.
    let resolved = resolve_uncached(start_dir, override_value)?;
    if violates_dot_claude_guard(&resolved) {
        return Err(WorkspaceError::ForbiddenDotClaudeDotClaude { resolved });
    }
    if let Ok(mut guard) = cache().lock() {
        guard.insert(key, resolved.clone());
    }
    Ok(resolved)
}

/// Inner resolver — handles override + ancestor walk; does *not* touch the
/// cache so callers can compose retries.
fn resolve_uncached(
    start_dir: &Path,
    override_value: Option<&str>,
) -> Result<PathBuf, WorkspaceError> {
    if let Some(override_raw) = override_value {
        let override_path = PathBuf::from(override_raw);
        return validate_override(override_path);
    }
    let resolved = walk_ancestors(start_dir)?;
    // Worktree redirect (the ONE behavioural change): a resolved anchor sitting
    // inside a LINKED git worktree is remapped to its MAIN checkout so specs,
    // events, markers, and telemetry land under the primary `.claude/`. The main
    // checkout and every non-git tree return `None` here and keep `resolved`.
    Ok(main_checkout_if_linked(&resolved).unwrap_or(resolved))
}

/// Validate an override value: the path must exist, satisfy the anchor
/// predicate, and not violate the I1 guard.
///
/// Deliberately validates against the **loose** anchor rule (`mustard.json` +
/// `.claude/` only), NOT the strict git-root rule the ancestor walk prefers:
/// `MUSTARD_WORKSPACE_ROOT` is an explicit, deliberate user choice — if the
/// user points Mustard at a directory that is not a git repository root, we
/// honour it rather than second-guess them. The strict rule only exists to
/// disambiguate the *automatic* walk in monorepos.
fn validate_override(path: PathBuf) -> Result<PathBuf, WorkspaceError> {
    if !path.exists() {
        return Err(WorkspaceError::OverrideInvalid {
            path,
            reason: "path does not exist".to_string(),
        });
    }
    if violates_dot_claude_guard(&path) {
        return Err(WorkspaceError::ForbiddenDotClaudeDotClaude { resolved: path });
    }
    if !is_anchor(&path) {
        return Err(WorkspaceError::OverrideInvalid {
            path,
            reason: "missing mustard.json and/or .claude/".to_string(),
        });
    }
    Ok(path)
}

/// Walk ancestors of `start_dir` in two passes.
///
/// Pass 1 (strict) returns the nearest ancestor that is an anchor **and** a
/// git repository root — this is what pins the workspace to the repository
/// boundary in monorepos, so a stray `mustard.json` + `.claude/` inside a
/// subproject can never become a phantom anchor. Pass 2 (loose fallback) only
/// runs when pass 1 found nothing anywhere up the tree: it re-walks with the
/// historical anchor-only rule so projects with no git at all keep resolving
/// exactly as before (fail-open).
fn walk_ancestors(start_dir: &Path) -> Result<PathBuf, WorkspaceError> {
    for candidate in start_dir.ancestors() {
        if is_anchor(candidate) && is_git_repo_root(candidate) {
            return Ok(candidate.to_path_buf());
        }
    }
    for candidate in start_dir.ancestors() {
        if is_anchor(candidate) {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(WorkspaceError::AnchorNotFound {
        searched_from: start_dir.to_path_buf(),
    })
}

/// True iff `dir` contains both `mustard.json` (file) and `.claude/` (dir).
fn is_anchor(dir: &Path) -> bool {
    let mustard_json = dir.join("mustard.json");
    let claude_dir = dir.join(".claude");
    mustard_json.is_file() && claude_dir.is_dir()
}

/// True iff `dir` is the root of a git repository: `dir/.git` exists as
/// **either** a directory (normal checkout) **or** a file (a submodule or a
/// linked worktree, where `.git` is a `gitdir:` pointer file). Purely a
/// filesystem probe — no `git` subprocess — so it is cheap enough for the
/// ancestor walk and never fails: an unreadable / absent path is simply
/// "not a git root".
pub fn is_git_repo_root(dir: &Path) -> bool {
    let dot_git = dir.join(".git");
    dot_git.is_dir() || dot_git.is_file()
}

/// Canonicalise `p`, falling back to the path as-given on error — so a relative
/// and an absolute reading of the same directory compare equal without panicking.
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Run `git rev-parse <args>` in `dir`, returning the trimmed stdout as a
/// [`PathBuf`] on success. `None` on any failure — git absent, not a repo, a
/// non-zero exit, or empty output. Never panics: this is the fail-open seam of
/// the worktree redirect.
fn git_rev_parse(dir: &Path, args: &[&str]) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("rev-parse")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// When `dir` is inside a LINKED git worktree, return the MAIN checkout root;
/// otherwise `None` (⇒ the caller keeps today's walk result unchanged).
///
/// Detection mirrors `work_branch_gate::is_isolated_worktree`: a linked worktree
/// reports a per-worktree `--git-dir` distinct from the shared `--git-common-dir`,
/// while the MAIN checkout reports the same path for both. Derivation mirrors
/// `git_settle::main_checkout_root`: the parent of the absolute `…/.git` common
/// dir IS the main checkout. Fully fail-open — git absent, not a repo, the main
/// checkout (dirs equal), or a derived root that is not a valid Mustard anchor
/// all yield `None`, so only a proven linked worktree is ever redirected.
fn main_checkout_if_linked(dir: &Path) -> Option<PathBuf> {
    let git_dir = git_rev_parse(dir, &["--path-format=absolute", "--git-dir"])?;
    let common = git_rev_parse(dir, &["--path-format=absolute", "--git-common-dir"])?;
    // Main checkout ⇒ identical dirs ⇒ nothing to redirect (behaviour identical).
    if canonical(&git_dir) == canonical(&common) {
        return None;
    }
    // Linked worktree: the parent of the shared `…/.git` common dir is the main
    // checkout root; `--show-toplevel` is the fallback for an unusual common dir.
    let main = if common.file_name().and_then(|n| n.to_str()) == Some(".git") {
        common.parent()?.to_path_buf()
    } else {
        git_rev_parse(dir, &["--path-format=absolute", "--show-toplevel"])?
    };
    // Redirect only to a genuine, uncontaminated Mustard anchor — otherwise keep
    // today's resolution rather than invent a root.
    if is_anchor(&main) && !violates_dot_claude_guard(&main) {
        Some(main)
    } else {
        None
    }
}

/// I1 guard mirrored from [`crate::io::claude_paths`] — kept private so the two
/// modules cannot drift apart accidentally.
fn violates_dot_claude_guard(path: &Path) -> bool {
    let last_is_dot_claude =
        path.file_name().and_then(|s| s.to_str()) == Some(".claude");
    if last_is_dot_claude {
        return true;
    }
    let as_string = path.to_string_lossy().replace('\\', "/");
    as_string.contains(".claude/.claude/") || as_string.ends_with(".claude/.claude")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Build a minimal STRICT anchor: `mustard.json` + `.claude/` + a `.git/`
    /// directory (the fixture is a git repository root, satisfying the strict
    /// pass — the shape of every real Mustard project under git).
    fn make_anchor(at: &Path) {
        make_loose_anchor(at);
        std::fs::create_dir_all(at.join(".git")).unwrap();
    }

    /// Build a LOOSE anchor only: `mustard.json` + `.claude/`, NO `.git`.
    /// Resolvable solely through the fallback pass (git-less projects) — or
    /// not at all when a strict anchor exists above it.
    fn make_loose_anchor(at: &Path) {
        std::fs::write(at.join("mustard.json"), b"{}").unwrap();
        std::fs::create_dir_all(at.join(".claude")).unwrap();
    }

    /// Serialise tests that touch the process-wide memo cache and clear it
    /// before the test body runs. The returned guard pins the lock for the
    /// caller's scope, so sibling tests cannot race on the cache state mid-run.
    #[must_use]
    fn serialize_test() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(mut cache_guard) = cache().lock() {
            cache_guard.clear();
        }
        guard
    }

    #[test]
    fn workspace_root_resolves_from_root_when_anchor_present() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        let resolved = resolve_with_override(dir.path(), None).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn workspace_root_resolves_from_subproject_ancestor_walk() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        // 3 levels deep: <root>/apps/foo/src
        let deep = dir.path().join("apps").join("foo").join("src");
        std::fs::create_dir_all(&deep).unwrap();
        let resolved = resolve_with_override(&deep, None).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn workspace_root_fails_without_anchor() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        // No mustard.json / .claude planted.
        let err = resolve_with_override(dir.path(), None).unwrap_err();
        assert!(matches!(err, WorkspaceError::AnchorNotFound { .. }));
    }

    #[test]
    fn workspace_root_fails_with_only_mustard_json() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let err = resolve_with_override(dir.path(), None).unwrap_err();
        assert!(matches!(err, WorkspaceError::AnchorNotFound { .. }));
    }

    #[test]
    fn workspace_root_fails_with_only_claude_dir() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let err = resolve_with_override(dir.path(), None).unwrap_err();
        assert!(matches!(err, WorkspaceError::AnchorNotFound { .. }));
    }

    #[test]
    fn workspace_root_traverses_git_submodule() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        // Plant an intermediate `.git/` (simulated submodule). The walker
        // must not stop here — it has no `mustard.json + .claude/`.
        let sub = dir.path().join("apps").join("dashboard").join("src-tauri");
        std::fs::create_dir_all(sub.join(".git")).unwrap();
        let resolved = resolve_with_override(&sub, None).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn workspace_root_skips_phantom_subproject_anchor_inside_git_repo() {
        let _guard = serialize_test();
        // The real monorepo defect: a stray committed `mustard.json` +
        // `.claude/` inside apps/dashboard (which has NO `.git` of its own)
        // made the subproject a phantom anchor and harness state landed there.
        // The strict pass must walk past it to the git repository root.
        let dir = tempdir().unwrap();
        make_anchor(dir.path()); // git root + anchor
        let sub = dir.path().join("apps").join("dashboard");
        std::fs::create_dir_all(&sub).unwrap();
        make_loose_anchor(&sub); // phantom: anchor files, no .git
        let start = sub.join("src");
        std::fs::create_dir_all(&start).unwrap();
        let resolved = resolve_with_override(&start, None).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap(),
            "phantom subproject anchor must lose to the git repository root"
        );
    }

    #[test]
    fn workspace_root_accepts_submodule_git_file_as_own_anchor() {
        let _guard = serialize_test();
        // sialia case: a git SUBMODULE has `.git` as a FILE carrying a
        // `gitdir:` pointer. A user who ran `mustard init` inside it made it a
        // deliberate anchor — the subproject must win over the outer root.
        let dir = tempdir().unwrap();
        make_anchor(dir.path()); // outer repo root, also an anchor
        let sub = dir.path().join("backend").join("Sialia.Backend");
        std::fs::create_dir_all(&sub).unwrap();
        make_loose_anchor(&sub);
        // The pointer target is deliberately bogus: `is_git_repo_root` is a
        // pure filesystem probe and the worktree redirect is fail-open, so an
        // unresolvable gitdir must not disturb the resolution.
        std::fs::write(
            sub.join(".git"),
            b"gitdir: ../../.git/modules/Sialia.Backend\n",
        )
        .unwrap();
        let resolved = resolve_with_override(&sub, None).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(&sub).unwrap(),
            "a submodule (.git file) that is an anchor wins over the outer root"
        );
    }

    #[test]
    fn workspace_root_loose_fallback_resolves_git_less_project() {
        let _guard = serialize_test();
        // No `.git` anywhere up the tree: the strict pass finds nothing and
        // the loose fallback must keep today's behaviour (fail-open).
        let dir = tempdir().unwrap();
        make_loose_anchor(dir.path());
        let deep = dir.path().join("src").join("lib");
        std::fs::create_dir_all(&deep).unwrap();
        let resolved = resolve_with_override(&deep, None).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap(),
            "git-less projects must keep resolving through the loose fallback"
        );
    }

    #[test]
    fn workspace_root_rejects_resolved_dot_claude_dot_claude() {
        let _guard = serialize_test();
        // Construct a contaminated start_dir: <root>/.claude/.claude. We
        // plant the anchor at <root>/.claude/ so the walker resolves to it
        // and the I1 guard fires.
        let dir = tempdir().unwrap();
        let contaminated_root = dir.path().join(".claude");
        std::fs::create_dir_all(&contaminated_root).unwrap();
        make_anchor(&contaminated_root);
        let start = contaminated_root.join(".claude");
        std::fs::create_dir_all(&start).unwrap();
        let err = resolve_with_override(&start, None).unwrap_err();
        assert!(matches!(
            err,
            WorkspaceError::ForbiddenDotClaudeDotClaude { .. }
        ));
    }

    #[test]
    fn workspace_root_honors_env_override() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        let other = tempdir().unwrap();
        // `other` has no anchor; `dir` does. With the override pointing at
        // `dir`, calling from `other` must still resolve to `dir`.
        let override_path = dir.path().to_string_lossy().into_owned();
        let resolved = resolve_with_override(other.path(), Some(&override_path)).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn env_override_stays_loose_no_git_required() {
        let _guard = serialize_test();
        // MUSTARD_WORKSPACE_ROOT is a deliberate user choice: it must accept a
        // loose anchor (no .git) — the strict rule only disambiguates the
        // automatic ancestor walk, never an explicit override.
        let dir = tempdir().unwrap();
        make_loose_anchor(dir.path());
        let other = tempdir().unwrap();
        let override_path = dir.path().to_string_lossy().into_owned();
        let resolved = resolve_with_override(other.path(), Some(&override_path)).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn workspace_root_rejects_invalid_env_override() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        // Override points at a directory that exists but has no anchor.
        let override_path = dir.path().to_string_lossy().into_owned();
        let other = tempdir().unwrap();
        let err = resolve_with_override(other.path(), Some(&override_path)).unwrap_err();
        assert!(matches!(err, WorkspaceError::OverrideInvalid { .. }));
    }

    #[test]
    fn workspace_root_memoizes_same_input() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        let first = resolve_with_override(dir.path(), None).unwrap();
        // Delete the anchor — a fresh resolver would fail. The cache must
        // return the original value.
        std::fs::remove_file(dir.path().join("mustard.json")).unwrap();
        std::fs::remove_dir_all(dir.path().join(".claude")).unwrap();
        let second = resolve_with_override(dir.path(), None).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn public_workspace_root_reads_env_when_present() {
        // Sanity check: calling the public API with no env var set behaves
        // the same as the helper. This does NOT mutate the env (forbidden by
        // the `unsafe_code` lint), so it only exercises the "var absent"
        // branch — the `Some(_)` branch is fully covered by
        // `workspace_root_honors_env_override` via the helper seam.
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        if std::env::var(OVERRIDE_ENV).is_ok() {
            // CI or sibling test set the override — skip rather than
            // contaminate the assertion.
            return;
        }
        let resolved = workspace_root(dir.path()).unwrap();
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }
}
