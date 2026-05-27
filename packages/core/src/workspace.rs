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
//! ancestors looking for an **anchor** — a directory that contains **both**
//! `mustard.json` (file) and `.claude/` (directory). The first ancestor that
//! satisfies the predicate is the workspace root.
//!
//! ## Override
//!
//! `MUSTARD_WORKSPACE_ROOT` short-circuits the walker. The value is the path
//! to use directly; it is validated against the same anchor predicate and the
//! I1 `.claude/.claude/` guard before being accepted.
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
//!   `(start_dir_canonical, override_value)` pair return the cached
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

/// Memoisation key — the canonicalised `start_dir` plus the literal value of
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
    // Build the cache key. `canonicalize()` may fail (start_dir absent or
    // permission-denied); fall back to the path as-given so we still cache
    // by something stable.
    let canonical_key =
        std::fs::canonicalize(start_dir).unwrap_or_else(|_| start_dir.to_path_buf());
    let key: CacheKey = (canonical_key, override_value.map(str::to_string));

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
    walk_ancestors(start_dir)
}

/// Validate an override value: the path must exist, satisfy the anchor
/// predicate, and not violate the I1 guard.
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

/// Walk ancestors of `start_dir`, returning the first that is an anchor.
fn walk_ancestors(start_dir: &Path) -> Result<PathBuf, WorkspaceError> {
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

/// I1 guard mirrored from [`crate::claude_paths`] — kept private so the two
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

    /// Build a minimal anchor: a directory containing `mustard.json` + `.claude/`.
    fn make_anchor(at: &Path) {
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
