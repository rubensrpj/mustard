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
//! `.claude/.claude/` guard before being accepted.
//!
//! ## Worktree redirect
//!
//! When the resolved anchor sits inside a LINKED git worktree — its `.git` file
//! points at an admin folder that carries a `commondir` — the root is remapped
//! to the MAIN checkout ([`linked_worktree_main`]). All Mustard state (specs,
//! events, active-spec markers, telemetry) then lands under the primary
//! checkout's `.claude/`, never the worktree's: the worktree carries only code.
//! The redirect reads the files git leaves behind and never runs git, so a
//! harness event spawns no process for it. It is SURGICAL and fail-open — the
//! main checkout, every non-git tree, and any file that does not read keep the
//! un-redirected walk result, so only a proven linked worktree changes. It
//! applies to the ancestor-walk path only; the `MUSTARD_WORKSPACE_ROOT`
//! override is honoured verbatim.
//!
//! A linked worktree that lives OUTSIDE the project — the separate copy of a
//! wave, in the user's cache folder — has no anchor anywhere above it. When
//! the walk finds nothing, the main checkout of that worktree becomes the
//! root, provided it is a genuine anchor; otherwise the walk's
//! [`WorkspaceError::AnchorNotFound`] stands.
//!
//! ## Inviolable safety contract
//!
//! - **No cwd fallback.** If no ancestor satisfies the predicate, the function
//!   returns [`WorkspaceError::AnchorNotFound`]. It never silently picks
//!   `start_dir` itself.
//! - **Crosses `.git/` submodule boundaries.** Mustard monorepos commonly
//!   embed `apps/dashboard/server` with its own `.git/`; the walker steps
//!   straight past such intermediate `.git/` markers.
//! - **No `.claude/.claude/`.** Resolved paths are rejected with
//!   [`WorkspaceError::ForbiddenDotClaudeDotClaude`] if the final segment is
//!   `.claude` or the path contains the sub-sequence `.claude/.claude/`. This
//!   keeps the guard close to the boundary where the path is minted.
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

/// The workspace root for `start_dir`, falling back to `start_dir` itself.
///
/// The fail-open face of [`workspace_root`], for the writers that must produce
/// a path no matter what. A writer handed a directory INSIDE the workspace —
/// a subproject that carries a `.claude/` of its own, say — must still land on
/// the ONE `.claude/` the dashboard and the reports read; a blind
/// `start_dir.join(".claude")` shards the state per subproject instead, and
/// `doctor --check workspace-leaks` names the eight directories where that has
/// already happened.
///
/// Falling back rather than erroring is deliberate: every caller here is
/// fail-silent by contract (a metric, an event line, a verdict file), so a
/// resolution failure must degrade to the old behaviour instead of dropping
/// the write. With no anchor above `start_dir` — a `tempdir` in a unit test, a
/// checkout without `mustard.json` — the answer IS `start_dir`.
#[must_use]
pub fn workspace_root_or_self(start_dir: &Path) -> PathBuf {
    workspace_root(start_dir).unwrap_or_else(|_| start_dir.to_path_buf())
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

    // Fast path — already cached. Re-validate the `.claude/.claude/` guard against the
    // cached value so a stale `.claude/.claude/` answer can never sneak
    // through.
    if let Ok(guard) = cache().lock()
        && let Some(hit) = guard.get(&key) {
            if violates_dot_claude_guard(hit) {
                return Err(WorkspaceError::ForbiddenDotClaudeDotClaude {
                    resolved: hit.clone(),
                });
            }
            return Ok(hit.clone());
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
    // A linked worktree OUTSIDE the project — the separate copy of a wave,
    // which lives in the user's cache folder — has no project above it, and
    // Mustard itself stays out of git, so the copy carries no anchor either.
    // When the walk finds nothing, the main checkout of that worktree is the
    // project, provided it is a genuine anchor.
    let resolved = match walk_ancestors(start_dir) {
        Ok(found) => found,
        Err(missing) => return main_checkout_if_linked(start_dir).ok_or(missing),
    };
    // Worktree redirect (the ONE behavioural change): a resolved anchor sitting
    // inside a LINKED git worktree is remapped to its MAIN checkout so specs,
    // events, markers, and telemetry land under the primary `.claude/`. The main
    // checkout and every non-git tree return `None` here and keep `resolved`.
    Ok(main_checkout_if_linked(&resolved).unwrap_or(resolved))
}

/// Validate an override value: the path must exist, satisfy the anchor
/// predicate, and not violate the `.claude/.claude/` guard.
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

/// The project whose directory `start_dir` sits in, by the ancestor walk ALONE
/// — no memoisation, no worktree redirect — or `None` when no ancestor is an
/// anchor.
///
/// The walk is [`walk_ancestors`], the very one [`workspace_root`] takes, so
/// this is not a second answer to "which project is this": it is the same walk,
/// stopped one step earlier. What it deliberately leaves out is the linked-
/// worktree redirect: the redirect exists so specs, events and telemetry land
/// under the ONE `.claude/`, which is a question about where STATE is written.
/// Which binary runs a command — the question of the one caller here, the git
/// executor ([`crate::platform::git::run`]) — is not that question.
#[must_use]
pub fn anchor_of(start_dir: &Path) -> Option<PathBuf> {
    walk_ancestors(start_dir).ok()
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

/// When `dir` is inside a LINKED git worktree, return the MAIN checkout root;
/// otherwise `None` (⇒ the caller keeps today's walk result unchanged).
///
/// The main checkout is [`linked_worktree_main`]'s; only a genuine,
/// uncontaminated Mustard anchor is accepted, so only a proven linked worktree
/// is ever redirected. It serves both faces of the resolver: the anchor the
/// walk found, and a start the walk placed in no project — the separate copy
/// of a wave, in the user's cache folder, which carries no Mustard file.
fn main_checkout_if_linked(dir: &Path) -> Option<PathBuf> {
    let main = linked_worktree_main(dir)?;
    // Redirect only to a genuine, uncontaminated Mustard anchor — otherwise keep
    // today's resolution rather than invent a root.
    if is_anchor(&main) && !violates_dot_claude_guard(&main) {
        Some(main)
    } else {
        None
    }
}

/// The MAIN checkout when `dir` is inside a LINKED git worktree; `None` in the
/// main checkout itself, outside git, or when the files do not lead there.
///
/// Read from the files git leaves behind, and never by running git: it sits on
/// the path of every harness event, and the git executor
/// ([`crate::platform::git::run`]) asks it which project owns a wave copy that
/// lives outside its project — asking git there would be asking git how to ask
/// git. Like git, the reading stops at the nearest `.git` above `dir`. A
/// folder there is a main checkout. A file names the admin folder, read as
/// [`checkout_git_dir`] reads it, and only a linked
/// worktree's admin folder carries the `commondir` that leads to the shared
/// folder; a submodule's has none. The folder above a shared `…/.git` is the
/// main checkout. A shared folder with another name — a submodule's, under the
/// outer project's `.git/modules` — names its checkout in `core.worktree`; a
/// bare repository names none, for it has no checkout.
///
/// Unlike the walk's redirect, it does not ask the main checkout to be a
/// Mustard anchor as seen from the worktree: once Mustard stays out of git, a
/// worktree carries no Mustard file at all, and the spec event writer still has
/// to reach the main checkout's file.
#[must_use]
pub fn linked_worktree_main(dir: &Path) -> Option<PathBuf> {
    // A relative `.` only climbs to the folders above once it is absolute.
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let top = dir.ancestors().find(|folder| is_git_repo_root(folder))?;
    // The `.git` folder of a main checkout is its own git folder, never an
    // admin folder that leads elsewhere.
    let admin = checkout_git_dir(top).filter(|admin| *admin != top.join(".git"))?;
    let common = std::fs::read_to_string(admin.join("commondir")).ok()?;
    let common = std::fs::canonicalize(admin.join(common.trim())).ok()?;
    if common.file_name().and_then(|name| name.to_str()) == Some(".git") {
        return common.parent().map(Path::to_path_buf);
    }
    std::fs::canonicalize(common.join(configured_worktree(&common)?)).ok()
}

/// The MAIN checkout of the Mustard source repository that `start` stands in,
/// or `None` when `start` is anywhere else.
///
/// From `start`, the root of the git checkout above it; inside a linked
/// worktree — the separate copy of a wave — the main checkout that
/// [`linked_worktree_main`] leads to. A checkout is Mustard's when
/// `apps/rt/Cargo.toml` declares the package `mustard-rt`: read from the file,
/// and not from the folder name, so a clone under any name counts and a project
/// that merely lives in a folder called `mustard` does not. Files only, no git
/// process: the answer sits on the path of every `mustard-rt` call.
#[must_use]
pub fn mustard_checkout(start: &Path) -> Option<PathBuf> {
    // A relative `.` only climbs to the folders above once it is absolute.
    let start = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    let top = start.ancestors().find(|folder| is_git_repo_root(folder))?;
    let main = linked_worktree_main(&start).unwrap_or_else(|| top.to_path_buf());
    declares_mustard_rt(&main).then_some(main)
}

/// The checkout builds the package `mustard-rt`: its `apps/rt/Cargo.toml`
/// names it in the `[package]` section.
fn declares_mustard_rt(checkout: &Path) -> bool {
    let Ok(manifest) = std::fs::read_to_string(checkout.join("apps").join("rt").join("Cargo.toml")) else {
        return false;
    };
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
        {
            return value.trim().trim_matches('"') == "mustard-rt";
        }
    }
    false
}

/// The git folder of the checkout rooted at `checkout`, read from files only:
/// a `.git` folder is that folder; a `.git` file — a linked worktree's or a
/// submodule's — names it on its `gitdir: <path>` line, absolute or relative
/// to the checkout, with the spaces around the line and the path ignored.
/// `None` when there is no `.git`, or the file names no folder.
///
/// It is the one reading of that pointer: the branch a checkout stands on is
/// read through it, and so is the way from a linked worktree to its main
/// checkout ([`linked_worktree_main`]).
#[must_use]
pub fn checkout_git_dir(checkout: &Path) -> Option<PathBuf> {
    let dot_git = checkout.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let pointer = std::fs::read_to_string(&dot_git).ok()?;
    let target = pointer.lines().find_map(|line| line.trim().strip_prefix("gitdir:"))?.trim();
    if target.is_empty() {
        return None;
    }
    let target = Path::new(target);
    Some(if target.is_absolute() { target.to_path_buf() } else { checkout.join(target) })
}

/// The checkout the shared git folder `common` names in `core.worktree`,
/// relative to that folder or absolute. Git reads its `config` and then its
/// `config.worktree`, so the second one wins. `None` when neither names one.
fn configured_worktree(common: &Path) -> Option<String> {
    ["config.worktree", "config"]
        .into_iter()
        .filter_map(|name| std::fs::read_to_string(common.join(name)).ok())
        .find_map(|text| core_worktree(&text))
}

/// The last `worktree` of the `[core]` section in the git configuration
/// `text` — section and key in any case, as git reads them.
fn core_worktree(text: &str) -> Option<String> {
    let mut in_core = false;
    let mut found = None;
    for line in text.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            in_core = header.split(']').next().is_some_and(|name| name.trim().eq_ignore_ascii_case("core"));
        } else if in_core
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("worktree")
        {
            found = Some(config_value(value));
        }
    }
    found.filter(|value| !value.is_empty())
}

/// A git configuration value as git writes it: quotes are dropped, a
/// backslash keeps the character after it, and a `#` or `;` outside quotes
/// starts a comment.
fn config_value(raw: &str) -> String {
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = raw.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => value.extend(chars.next()),
            '#' | ';' if !quoted => break,
            _ => value.push(c),
        }
    }
    value.trim_end().to_string()
}

/// The `.claude/.claude/` guard mirrored from [`crate::io::claude_paths`] — kept private so the two
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
        let sub = dir.path().join("apps").join("dashboard").join("server");
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
        // and the `.claude/.claude/` guard fires.
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

    /// `git` with a fixed identity, in `dir`; the test fails on a git error.
    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A hook running inside the separate copy of a wave — a linked worktree
    /// in a folder outside the project, like the user's cache — finds no
    /// project walking up: Mustard stays out of git, so the copy carries none
    /// of it. The root it resolves is the main checkout, and the spec it
    /// writes and reads back is the project's, never one inside the copy. A
    /// folder outside any project and outside git still resolves nothing.
    #[test]
    fn a_hook_inside_an_outside_copy_resolves_the_main_checkout() {
        let _guard = serialize_test();
        let dir = tempdir().unwrap();
        let main = dir.path().join("projeto");
        std::fs::create_dir_all(main.join("src")).unwrap();
        std::fs::write(main.join("src").join("lib.rs"), "fn um() {}\n").unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "seed"]);
        make_loose_anchor(&main);
        let copy = dir.path().join("cache").join("mustard").join("copias").join("projeto-0123abcd").join("x-1");
        git(&main, &["worktree", "add", "-q", "--detach", &copy.to_string_lossy(), "HEAD"]);
        assert!(!copy.join("mustard.json").exists() && !copy.join(".claude").exists(), "the copy carries no Mustard file");
        let start = copy.join("src");

        let resolved = resolve_with_override(&start, None).expect("the outside copy resolves to its main checkout");
        assert_eq!(std::fs::canonicalize(&resolved).unwrap(), std::fs::canonicalize(&main).unwrap());

        let file = crate::io::spec_events::spec_file(&resolved, "x").unwrap();
        let said = serde_json::json!({"author": "user", "text": "da cópia"});
        crate::io::spec_events::write(&file, "message", said.as_object().cloned().unwrap(), &[]).unwrap();
        assert!(main.join(".claude").join("spec").join("x").join("spec.ndjson").is_file(), "the spec lands in the project");
        let again = resolve_with_override(&start, None).unwrap();
        let log = crate::io::spec_events::read(&crate::io::spec_events::spec_file(&again, "x").unwrap())
            .unwrap()
            .expect("the hook reads the project's spec back");
        assert_eq!(log.events.len(), 1, "{:?}", log.events);
        assert!(!copy.join(".claude").exists(), "nothing is written inside the copy");

        let loose = dir.path().join("solta");
        std::fs::create_dir_all(&loose).unwrap();
        assert!(
            matches!(resolve_with_override(&loose, None), Err(WorkspaceError::AnchorNotFound { .. })),
            "a folder outside any project still resolves nothing",
        );
    }

    /// Um repositório de mentira que declara o pacote `mustard-rt`, com o git
    /// de verdade para que o `worktree` nasça.
    fn mustard_repo(at: &Path) {
        std::fs::create_dir_all(at.join("apps").join("rt")).unwrap();
        std::fs::write(at.join("apps").join("rt").join("Cargo.toml"), "[package]\nname = \"mustard-rt\"\nversion = \"0.1.0\"\n").unwrap();
        git(at, &["init", "-q"]);
        git(at, &["add", "-A"]);
        git(at, &["commit", "-q", "-m", "seed"]);
    }

    /// Dentro do repositório do Mustard, de qualquer pasta dele, a resposta é a
    /// raiz do checkout; numa cópia de onda — um `worktree` fora do projeto —,
    /// é o checkout principal; fora dele, em outro projeto, num manifesto que
    /// não é do `mustard-rt` ou numa pasta fora do git, não é nada.
    #[test]
    fn the_mustard_checkout_is_found_from_the_repository_and_from_a_wave_copy() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("qualquer-nome");
        mustard_repo(&main);
        let main_real = std::fs::canonicalize(&main).unwrap();
        let same = |found: Option<PathBuf>, want: &Path| {
            assert_eq!(found.map(|found| std::fs::canonicalize(found).unwrap()), Some(want.to_path_buf()));
        };
        same(mustard_checkout(&main), &main_real);
        same(mustard_checkout(&main.join("apps").join("rt")), &main_real);

        let copy = dir.path().join("cache").join("copias").join("projeto-0123abcd").join("x").join("a");
        git(&main, &["worktree", "add", "-q", "--detach", &copy.to_string_lossy(), "HEAD"]);
        same(mustard_checkout(&copy), &main_real);
        same(mustard_checkout(&copy.join("apps").join("rt")), &main_real);

        let other = dir.path().join("outro");
        std::fs::create_dir_all(other.join("apps").join("rt")).unwrap();
        std::fs::write(other.join("apps").join("rt").join("Cargo.toml"), "[package]\nname = \"outro-app\"\n").unwrap();
        git(&other, &["init", "-q"]);
        assert_eq!(mustard_checkout(&other), None, "a repository whose apps/rt is another package");
        let renamed = dir.path().join("mustard");
        std::fs::create_dir_all(renamed.join(".git")).unwrap();
        assert_eq!(mustard_checkout(&renamed), None, "the folder name alone proves nothing");
        let loose = dir.path().join("solta");
        std::fs::create_dir_all(loose.join("apps").join("rt")).unwrap();
        std::fs::write(loose.join("apps").join("rt").join("Cargo.toml"), "[package]\nname = \"mustard-rt\"\n").unwrap();
        assert_eq!(mustard_checkout(&loose), None, "outside git there is no checkout");
        std::fs::write(
            main.join("apps").join("rt").join("Cargo.toml"),
            "[dependencies]\nname = \"mustard-rt\"\n[package]\nname = \"outro\"\n",
        )
        .unwrap();
        assert_eq!(mustard_checkout(&main), None, "only the name in [package] counts");
    }

    /// The git folder of a checkout is its `.git` folder, or the folder a
    /// `.git` file names on its `gitdir:` line: relative to the checkout or
    /// absolute, with spaces before the line or around the path ignored. A
    /// checkout with no `.git`, or a file that names nothing, has none.
    #[test]
    fn the_git_folder_of_a_checkout_is_its_folder_or_the_one_its_pointer_names() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("principal");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        assert_eq!(checkout_git_dir(&main), Some(main.join(".git")), "a .git folder is the git folder");

        let relative = dir.path().join("relativa");
        std::fs::create_dir_all(&relative).unwrap();
        std::fs::write(relative.join(".git"), "gitdir: ../principal/.git/worktrees/relativa\n").unwrap();
        assert_eq!(
            checkout_git_dir(&relative),
            Some(relative.join("../principal/.git/worktrees/relativa")),
            "a relative pointer is read from the checkout",
        );

        let absolute = dir.path().join("absoluta");
        std::fs::create_dir_all(&absolute).unwrap();
        let admin = main.join(".git").join("worktrees").join("absoluta");
        std::fs::write(absolute.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        assert_eq!(checkout_git_dir(&absolute), Some(admin.clone()), "an absolute pointer is taken as it is");

        let spaced = dir.path().join("com-espaco");
        std::fs::create_dir_all(&spaced).unwrap();
        std::fs::write(spaced.join(".git"), format!("  gitdir:   {}  \r\n", admin.display())).unwrap();
        assert_eq!(checkout_git_dir(&spaced), Some(admin), "spaces around the line and the path are ignored");

        let none = dir.path().join("sem-git");
        std::fs::create_dir_all(&none).unwrap();
        assert_eq!(checkout_git_dir(&none), None, "no .git, no git folder");
        std::fs::write(none.join(".git"), "gitdir:\n").unwrap();
        assert_eq!(checkout_git_dir(&none), None, "a pointer that names nothing gives nothing");
    }

    /// The checkout a shared git folder names is read as git reads its
    /// configuration: only the `[core]` section, section and key in any case,
    /// the last one standing, quotes dropped, a backslash keeping the
    /// character after it and a comment cut off; a folder that names none —
    /// a bare repository's — gives none.
    #[test]
    fn the_configured_checkout_is_read_as_git_reads_it() {
        let text = "[core]\n\tbare = false\n\tworktree = ../../../velho\n[remote \"origin\"]\n\tworktree = errado\n\
                    [CORE]\n\tWorkTree = \"../../../meu #mod\" ; o módulo\n[core \"sub\"]\n\tworktree = errado\n";
        assert_eq!(core_worktree(text).as_deref(), Some("../../../meu #mod"));
        assert_eq!(core_worktree("[core]\n\tworktree = a\\\"b # nota\n").as_deref(), Some("a\"b"));
        assert_eq!(core_worktree("[core]\n\tbare = true\n"), None);
        assert_eq!(core_worktree("[remote \"x\"]\n\tworktree = fora\n"), None);
    }
}
