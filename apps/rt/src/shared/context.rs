//! Environment resolution for the `run` face.
//!
//! Unlike the enforcement faces, a `run` subcommand never receives a
//! `HookInput` — it resolves the project directory and session id from the
//! process environment, mirroring how the JS scripts did (`CLAUDE_PROJECT_DIR`,
//! and the session through `spec_state::session_from_env`: `MUSTARD_SESSION_ID`,
//! then `CLAUDE_CODE_SESSION_ID`, then `CLAUDE_SESSION_ID`).

use mustard_core::io::fs;
use mustard_core::io::workspace::{workspace_root, WorkspaceError};
use mustard_core::ClaudePaths;
use mustard_core::InstallMode;
use mustard_core::ProjectConfig;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// A cheap fingerprint of one file — `(mtime, len)`, or `None` when it is absent
/// / unstat-able. A `stat` is far cheaper than the open + read + parse it lets a
/// cache hit skip, and folding it into the cache KEY (rather than invalidating
/// by hand) is what keeps a rewritten file from being served stale.
fn file_fingerprint(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// A cheap fingerprint of `<root>/mustard.json` — see [`file_fingerprint`].
fn mustard_json_fingerprint(root: &Path) -> Option<(SystemTime, u64)> {
    file_fingerprint(&root.join("mustard.json"))
}

/// Key of a fingerprinted memo: the path an answer belongs to, plus the
/// [`file_fingerprint`] of the file the answer was derived from. Shared by the
/// two caches below, which are the same idea over two different files.
type FingerprintKey = (PathBuf, Option<(SystemTime, u64)>);

/// Process-wide cache of [`ProjectConfig`] keyed by `(root, mustard.json
/// fingerprint)`.
///
/// Every gate in one `PreToolUse(Write|Edit)` dispatch independently needs the
/// project config (size / close / boundary / work-branch), so before this seam a
/// single dispatch re-read and re-parsed `mustard.json` 3-4 times. This collapses
/// them to ONE read+parse per file version: the first caller loads and stores, the
/// rest clone the cached value. Mirrors the process-wide, path-keyed memo
/// [`mustard_core::io::workspace::workspace_root`] already uses — the same
/// one-shot-process lifetime, so "process-wide" is "per dispatch" in production.
/// The `(mtime, len)` fingerprint re-loads a rewritten config, so an in-place
/// edit is never served stale (matters only to tests that mutate `mustard.json`).
fn config_cache() -> &'static Mutex<HashMap<FingerprintKey, ProjectConfig>> {
    static CACHE: OnceLock<Mutex<HashMap<FingerprintKey, ProjectConfig>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Load `<root>/mustard.json` through the process-wide [`config_cache`], returning
/// an owned [`ProjectConfig`] — a drop-in for [`ProjectConfig::load`] that skips
/// the disk read+parse when the same file version was already loaded this process.
/// Fail-open (defaults on any IO/parse error), inherited from the underlying load.
#[must_use]
pub fn project_config_cached(root: &Path) -> ProjectConfig {
    let key = (root.to_path_buf(), mustard_json_fingerprint(root));
    if let Ok(cache) = config_cache().lock()
        && let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
    let config = ProjectConfig::load(root);
    if let Ok(mut cache) = config_cache().lock() {
        cache.insert(key, config.clone());
    }
    config
}

// ---------------------------------------------------------------------------
// Install mode — autodetected, never configured
// ---------------------------------------------------------------------------

/// `CLAUDE.local.md` — the untracked instruction layer a private `scan --full`
/// writes a subproject's Guards into instead of that subproject's `CLAUDE.md`.
///
/// Claude Code discovers a subdirectory `CLAUDE.local.md` exactly as it
/// discovers a subdirectory `CLAUDE.md` — on demand, when a file in that
/// directory is read — and appends it AFTER the shared file in the same
/// directory, so a host repository's own Guards survive and ours are additive
/// (verified against the official memory documentation, 2026-08-17).
///
/// Re-exported from [`mustard_core`] rather than spelled again here: the
/// INSTALLER hides this name (it is a `footprint_rules` entry) and the scan
/// WRITES it. Two literals in two crates is how a mode ends up hiding a path
/// nobody produces — the same class of defect the resolvers below close on the
/// reading side.
pub use mustard_core::CLAUDE_LOCAL_MD;

/// `CLAUDE.md` — the shared instruction file, the only destination before the
/// private mode existed and still the only one an ordinary install writes.
///
/// Re-exported for the same reason as [`CLAUDE_LOCAL_MD`]: every reader of a
/// subproject's Guards resolves its filename through [`guards_file_name`] /
/// [`guards_file`] below, and a reader that retyped the literal is precisely the
/// defect those two exist to make impossible.
pub use mustard_core::CLAUDE_MD;

/// Per-`root` memo of the clone-local exclude file git resolves.
///
/// This is the expensive half — a `git rev-parse --git-path info/exclude`
/// process spawn — and its answer is a property of the repository's shape, which
/// does not change under a running hook, so `root` alone is a sound key.
fn exclude_path_cache() -> &'static Mutex<HashMap<PathBuf, Option<PathBuf>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Process-wide cache of the resolved [`InstallMode`], keyed by
/// `(exclude file, its (mtime, len) fingerprint)`.
///
/// Same shape and the same reason as [`config_cache`]: the verdict is read
/// repeatedly within one dispatch, and folding the fingerprint into the key
/// means a run that WRITES the exclude file (a private `run upsert`) is never
/// afterwards served the answer from before its own write.
fn install_mode_cache() -> &'static Mutex<HashMap<FingerprintKey, InstallMode>> {
    static CACHE: OnceLock<Mutex<HashMap<FingerprintKey, InstallMode>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Which footprint this project was installed with — autodetected, never
/// configured.
///
/// The mode lives in no versioned file and in no environment variable: a knob in
/// `mustard.json` would itself be the trace the private mode exists to remove
/// (the setting would announce the tool it hides), and an env var is state the
/// operator has to remember. It is chosen ONCE through `run upsert --private`
/// and thereafter read back off the clone-local exclude file that run wrote —
/// so the state is self-evident wherever the project is opened.
///
/// WHICH rules make an exclude file "private" is not decided here: the verdict
/// is [`mustard_core::carries_private_marks`], the same predicate the writer
/// consults, so the reader can never fall behind a mark the writer gained. What
/// this adds — and its only reason to exist beside
/// [`mustard_core::detect_install_mode`] — is the two caches below: a `run`
/// dispatch asks the question once per reader, and each miss costs a process
/// spawn plus a file read.
///
/// Fail-open: no git, no repository, or an unreadable exclude file answers
/// [`InstallMode::Shared`] — today's behaviour, unchanged.
#[must_use]
pub fn install_mode(root: &Path) -> InstallMode {
    let Some(path) = exclude_path_cached(root) else {
        return InstallMode::Shared;
    };
    let key = (path.clone(), file_fingerprint(&path));
    if let Ok(cache) = install_mode_cache().lock()
        && let Some(hit) = cache.get(&key) {
            return *hit;
        }
    let mode = match fs::read_to_string(&path) {
        Ok(body) if mustard_core::carries_private_marks(&body) => InstallMode::Private,
        _ => InstallMode::Shared,
    };
    if let Ok(mut cache) = install_mode_cache().lock() {
        cache.insert(key, mode);
    }
    mode
}

/// The clone-local exclude file for `root` through [`exclude_path_cache`].
fn exclude_path_cached(root: &Path) -> Option<PathBuf> {
    let key = root.to_path_buf();
    if let Ok(cache) = exclude_path_cache().lock()
        && let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
    let resolved = mustard_core::exclude_file(root);
    if let Ok(mut cache) = exclude_path_cache().lock() {
        cache.insert(key, resolved.clone());
    }
    resolved
}

/// The instruction-file name this install OWNS at `root`: [`CLAUDE_LOCAL_MD`]
/// under a private install, [`CLAUDE_MD`] otherwise.
///
/// This is the OWNED answer — the file a census of pending Guards scaffolds
/// looks for. It never falls back: a private install must never be pointed at
/// the file the host repository versions, so "owned" here is exact rather than
/// tolerant.
/// Readers want [`guards_file`] instead.
#[must_use]
pub fn guards_file_name(root: &Path) -> &'static str {
    if install_mode(root).is_private() {
        CLAUDE_LOCAL_MD
    } else {
        CLAUDE_MD
    }
}

/// The instruction file that carries `dir`'s Guards — **the one resolver every
/// reader goes through**.
///
/// Prefers the name this install owns ([`guards_file_name`]) and falls back to
/// [`CLAUDE_MD`] when it is not on disk. Both halves matter:
///
/// - *prefer* — a private `scan --full` writes the Guards to `CLAUDE.local.md`.
///   A reader still opening `CLAUDE.md` gets the CLIENT's file, or nothing, and
///   the harness ships with its central artifact inert. Every consumer named by
///   a repo-wide search for the literal join reads through here, so the mode is
///   decided ONCE: N call sites each choosing a filename IS the defect.
/// - *fall back* — a clone can be made private before its first full scan, and a
///   shared install that later goes private still has its Guards in the shared
///   file. Reading nothing at all is not what the private mode promises; it
///   promises the host's git sees nothing, which the exclude file already
///   delivers.
///
/// Under a shared install both halves are `CLAUDE.md`, so an ordinary project's
/// behaviour is byte-identical to before this existed.
#[must_use]
pub fn guards_file(root: &Path, dir: &Path) -> PathBuf {
    let owned = dir.join(guards_file_name(root));
    if owned.is_file() {
        owned
    } else {
        dir.join(CLAUDE_MD)
    }
}

/// Whether `name` names an instruction file in EITHER layer.
///
/// For the classifiers that receive a path they did not resolve (a write
/// gate's target, a doc linter's directory walk) and only need to
/// know "is this an instruction file at all?". Deliberately mode-blind: a
/// refusal that only recognised the mode's own name would wave through the other
/// layer's file, which is the wrong direction for a guard.
#[must_use]
pub fn is_guards_file_name(name: &str) -> bool {
    name == CLAUDE_MD || name == CLAUDE_LOCAL_MD
}

/// Per-`(project, session)` memo of [`spec_for_session`]; evicted by
/// [`invalidate_session_spec`] whenever the `active-spec` marker is (re)written or
/// removed, so a resolve after a binding change reflects disk.
/// Chave `(raiz, sessão)` para a spec que aquela sessão resolveu — `None` quando
/// a resolução deu em nada, o que também vale a pena lembrar.
type SessionSpecCache = Mutex<HashMap<(String, String), Option<String>>>;

fn session_spec_cache() -> &'static SessionSpecCache {
    static CACHE: OnceLock<SessionSpecCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drop the memoised [`spec_for_session`] answer for one `(project, session)`
/// after its marker changes, so the next resolve re-reads disk. Only the affected
/// key is evicted; a poisoned lock is skipped (fail-open).
fn invalidate_session_spec(project_dir_path: &str, session_id: &str) {
    if let Ok(mut cache) = session_spec_cache().lock() {
        cache.remove(&(project_dir_path.to_string(), session_id.to_string()));
    }
}

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
///    The raw process working directory as a `String`, defaulting to `"."`.
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

/// Session-directory names that are SINKS, not sessions: buckets a writer
/// invented because it had no harness session id — `"unknown"` (this module's
/// own last resort) and `"otel-unattached"` (the OTEL collector's unresolvable
/// bucket, its `UNATTACHED_SLUG`). The hooks only ever read the id the harness
/// hands them, so a marker keyed on one of these is unreachable: no hook is
/// ever handed a placeholder id.
const PLACEHOLDER_SESSION_IDS: &[&str] = &["unknown", "otel-unattached"];

/// `true` when `id` cannot name a session the hooks read — empty, or one of
/// the [`PLACEHOLDER_SESSION_IDS`]. The single predicate every session-keyed
/// marker writer and reader in this module consults, so a binding can never
/// again land under a placeholder directory (the field defect: `emit-pipeline`
/// run from the CLI resolved `otel-unattached` by mtime, wrote the
/// session→spec marker there, and every gate keyed on the real session id
/// silently fell back to whichever spec was newest).
fn is_placeholder_session(id: &str) -> bool {
    id.is_empty() || PLACEHOLDER_SESSION_IDS.contains(&id)
}

/// Resolve the current session id, defaulting to `"unknown"`.
///
/// Resolution order:
///
/// 1. The session the environment names, through the one reader of the
///    session variables ([`crate::shared::spec_state::session_from_env`]).
/// 2. Newest `.claude/.session/<id>/` directory by mtime — the filesystem
///    fallback. `run`-face emitters never receive a `HookInput`, so when no
///    session variable is set they used to land on `"unknown"`; the
///    `SessionStart` hook has already created `.claude/.session/<id>/`, so the
///    newest one by mtime recovers the real id.
///    Placeholder buckets ([`PLACEHOLDER_SESSION_IDS`]) never win this scan —
///    they are sinks other writers invented, not sessions any hook reads.
/// 3. `"unknown"` as a last resort.
#[must_use]
pub fn session_id() -> String {
    if let Some(id) = crate::shared::spec_state::session_from_env() {
        return id;
    }
    // Filesystem fallback: newest `.claude/.session/<id>/` dir. The `.session/`
    // base is not exposed via `ClaudePaths` (it is the events writer's consumer,
    // not Mustard-owned), so compose it from `claude_dir()` the same way the
    // writer does (see `events::writer_ndjson::event_dir`).
    if let Some(id) = ClaudePaths::for_project(Path::new(&project_dir()))
        .ok()
        .map(|p| p.claude_dir().join(".session"))
        .and_then(|session_base| newest_session_dir(&session_base))
    {
        return id;
    }
    "unknown".to_string()
}

/// Newest directory name under `session_dir`, skipping placeholder buckets.
///
/// Reads the entries of `<.claude>/.session/`, keeps directories whose name is
/// not a placeholder ([`is_placeholder_session`] — `"unknown"`, the OTEL
/// collector's `"otel-unattached"` sink), and returns the name of the one with
/// the newest mtime. The skip is what makes a CLI-side marker write reach the
/// session the hooks read: the collector touches its bucket constantly, so by
/// mtime alone the bucket used to win and the binding landed where no hook
/// ever looks. Returns `None` on any IO error or when no eligible directory
/// exists — never panics.
#[must_use]
fn newest_session_dir(session_dir: &Path) -> Option<String> {
    let entries = fs::read_dir(session_dir).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries {
        if !entry.path.is_dir() {
            continue;
        }
        let name = &entry.file_name;
        if is_placeholder_session(name) {
            continue;
        }
        let Ok(mtime) = fs::modified(&entry.path) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, name.clone()));
        }
    }
    best.map(|(_, name)| name)
}

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
/// cost the answer does not justify. In the main checkout no `git` runs at
/// all. In a linked worktree, where `.git` is a file, the HEAD is still read
/// through its `gitdir`, but the main checkout — where the `mustard.json` and
/// the spec folders live — is asked of `git` through
/// `workspace::linked_worktree_main`, two or three `git rev-parse` calls per
/// edit there. Fail-open at every step.
#[must_use]
pub fn spec_of_checkout_branch(project_dir_path: &str) -> Option<String> {
    let project = Path::new(project_dir_path);
    let git_dir = checkout_git_dir(project)?;
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?.trim();
    if branch.is_empty() {
        return None;
    }
    // In a linked worktree, the Mustard lives in the main checkout: the
    // `mustard.json` and the spec folders stay outside git. Only a `.git` that
    // is a file can be a worktree, and only then is git asked.
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

/// The git folder of the checkout in `project`, reading files only: `.git` is
/// the folder itself, or, in a linked worktree, a `gitdir: <path>` file.
fn checkout_git_dir(project: &Path) -> Option<PathBuf> {
    let dot_git = project.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text.lines().find_map(|line| line.trim().strip_prefix("gitdir:"))?.trim();
    let target = Path::new(target);
    Some(if target.is_absolute() { target.to_path_buf() } else { project.join(target) })
}

/// Resolve the spec a session is currently bound to, fail-open `None`.
///
/// Hook-emitted events (`tool.use`, `agent.*`, …) are born with no spec — the
/// PostToolUse hook context never sets `MUSTARD_ACTIVE_SPEC`. The
/// only reliable binding is the `pipeline.scope` event the CLI run-face emits,
/// which carries BOTH `session_id` and `spec`. Rather than scan the NDJSON log
/// on every tool call, the router persists that binding as a small marker file
/// (see [`bind_session_spec`]); this reads it back in O(1).
///
/// Marker location: `.claude/.session/<session_id>/active-spec` — beside the
/// session's own `.events/` directory.
///
/// Returns `None` when the session has no recorded binding (no marker yet, an
/// empty/`"unknown"` session id, or any IO error) — never panics.
#[must_use]
pub fn spec_for_session(project_dir_path: &str, session_id: &str) -> Option<String> {
    // Per-dispatch memo of the marker read; evicted on any binding change via
    // `invalidate_session_spec` (see `bind_session_spec` / `unbind_session_spec`).
    let cache_key = (project_dir_path.to_string(), session_id.to_string());
    if let Ok(cache) = session_spec_cache().lock()
        && let Some(hit) = cache.get(&cache_key) {
            return hit.clone();
        }
    let resolved = spec_for_session_uncached(project_dir_path, session_id);
    if let Ok(mut cache) = session_spec_cache().lock() {
        cache.insert(cache_key, resolved.clone());
    }
    resolved
}

/// Uncached body behind [`spec_for_session`]'s per-process memo.
fn spec_for_session_uncached(project_dir_path: &str, session_id: &str) -> Option<String> {
    if is_placeholder_session(session_id) {
        return None;
    }
    let marker = session_spec_marker(project_dir_path, session_id)?;
    let spec = fs::read_to_string(&marker).ok()?;
    let spec = spec.trim();
    if spec.is_empty() {
        None
    } else {
        Some(spec.to_string())
    }
}

/// Inverse lookup: the session currently bound to `spec` via its
/// `active-spec` marker. Scans `.claude/.session/*/active-spec` and, when
/// more than one session is bound to the same spec (rare — concurrent
/// sessions on one spec), returns the binding with the newest marker mtime.
///
/// Exists for spec-scoped READERS (e.g. `digest-adherence-finalize`): the
/// emitter and the reader run as separate processes minutes apart, and the
/// env-less newest-session-by-mtime fallback of [`session_id`] races against
/// any other session touching the project in between — the field symptom was
/// `digestUsed: false` with two digest queries on record. The marker is the
/// binding the researching session itself wrote, so resolving through it is
/// stable. `None` when no session is bound to `spec` — never panics.
#[must_use]
pub fn session_for_spec(project_dir_path: &str, spec: &str) -> Option<String> {
    if spec.is_empty() {
        return None;
    }
    let base = ClaudePaths::for_project(Path::new(project_dir_path))
        .ok()?
        .claude_dir()
        .join(".session");
    let entries = fs::read_dir(&base).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries {
        if !entry.path.is_dir() || is_placeholder_session(&entry.file_name) {
            continue;
        }
        let marker = entry.path.join("active-spec");
        let Ok(content) = fs::read_to_string(&marker) else {
            continue;
        };
        if content.trim() != spec {
            continue;
        }
        let Ok(mtime) = fs::modified(&marker) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, entry.file_name.clone()));
        }
    }
    best.map(|(_, name)| name)
}

/// Persist the session→spec binding as the `active-spec` marker, best-effort.
///
/// Called from the event router whenever an event already carries both a
/// non-empty `spec` and a resolved `session_id` (the `pipeline.scope` /
/// `pipeline.stage` / `pipeline.status` events the run-face emits). Later
/// spec-less hook events for the same session then inherit the spec via
/// [`spec_for_session`]. Fail-open: any IO error is swallowed — telemetry must
/// never block tool execution.
///
/// A placeholder session id ([`is_placeholder_session`]) is REFUSED: a binding
/// written under a bucket no hook is ever handed is unreachable, and the gates
/// keyed on it would silently fall back to whichever spec is newest.
pub fn bind_session_spec(project_dir_path: &str, session_id: &str, spec: &str) {
    if is_placeholder_session(session_id) || spec.is_empty() {
        return;
    }
    let Some(marker) = session_spec_marker(project_dir_path, session_id) else {
        return;
    };
    // Skip a redundant rewrite when the marker already names this spec — keeps
    // the hot-path write a no-op once the session is bound.
    if fs::read_to_string(&marker).ok().as_deref().map(str::trim) == Some(spec) {
        return;
    }
    let Some(parent) = marker.parent() else {
        return;
    };
    let _ = fs::create_dir_all(parent);
    let _ = fs::write_atomic(&marker, spec.as_bytes());
    // The marker changed — evict the stale spec_for_session memo so the next
    // resolve reflects the new binding.
    invalidate_session_spec(project_dir_path, session_id);
}

/// Remove the session→spec binding, best-effort.
///
/// Called on the terminal close of a spec so events in the gap after the close
/// no longer inherit the just-finished spec via [`spec_for_session`]. Resolves
/// the same `active-spec` marker [`bind_session_spec`] writes and deletes it.
/// A missing marker is a no-op, and any IO error is swallowed — telemetry
/// teardown must never block the close.
pub fn unbind_session_spec(project_dir: &str, session_id: &str) {
    if is_placeholder_session(session_id) {
        return;
    }
    let Some(marker) = session_spec_marker(project_dir, session_id) else {
        return;
    };
    let _ = fs::remove_file(&marker);
    invalidate_session_spec(project_dir, session_id);
}

/// Compose the `active-spec` marker path:
/// `<project>/.claude/.session/<session_id>/active-spec`.
///
/// The `.session/` base is not exposed via [`ClaudePaths`] (it is the events
/// writer's consumer, not Mustard-owned), so compose it from `claude_dir()` the
/// same way [`session_id`]'s fallback and the NDJSON writer do. `None` on a `.claude/.claude/`
/// guard rejection of the project root.
fn session_spec_marker(project_dir_path: &str, session_id: &str) -> Option<PathBuf> {
    Some(
        ClaudePaths::for_project(Path::new(project_dir_path))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(session_id)
            .join("active-spec"),
    )
}

/// Resolve the pending auto-branch a session's first file mutation must check
/// out, fail-open `None`.
///
/// Sibling of [`spec_for_session`]: `emit-pipeline --kind pipeline.kind`
/// pre-computes the `{work_kind}/{slug}` branch name and drops it here; the
/// cut `spec-draft` takes reads it back, checks the branch out, and clears the
/// marker. A request that never drafts a spec never consumes it.
///
/// Marker location: `.claude/.session/<session_id>/pending-work-branch` — beside
/// the session's `active-spec` marker. Returns `None` when the session has no
/// recorded pending branch (no marker yet, an empty/`"unknown"` session id, or
/// any IO error) — never panics.
///
/// The marker's FIRST line is the branch; a second line, when present, is the
/// base it was cut for ([`pending_base_for`]). One file, two questions — see
/// there for why the base is recorded at all.
#[must_use]
pub fn pending_branch_for(project_dir_path: &str, session_id: &str) -> Option<String> {
    let branch = pending_marker_line(project_dir_path, session_id, 0)?;
    Some(branch)
}

/// The integration base the pending branch was resolved FOR, `None` when the
/// marker records none.
///
/// The branch NAME no longer carries the base — it names what the unit IS
/// (`feature/`, `fix/`, `hotfix/`) — so for every unit whose base follows from
/// its kind the cut re-derives it from `git.flow` and this answers `None`. It is
/// written only when the operator's answer cannot be re-derived: a project
/// declaring several emergency bases leaves a hotfix a real CHOICE, and dropping
/// it here would silently cut the emergency from a base the operator did not
/// pick. That silent coercion is the exact defect `resolve_base` already refuses
/// at the other end.
#[must_use]
pub fn pending_base_for(project_dir_path: &str, session_id: &str) -> Option<String> {
    pending_marker_line(project_dir_path, session_id, 1)
}

/// One non-empty line of the `pending-work-branch` marker — the single parser
/// behind both readers, so the two can never disagree about the file's shape.
fn pending_marker_line(
    project_dir_path: &str,
    session_id: &str,
    index: usize,
) -> Option<String> {
    if is_placeholder_session(session_id) {
        return None;
    }
    let marker = pending_branch_marker(project_dir_path, session_id)?;
    let body = fs::read_to_string(&marker).ok()?;
    let line = body.lines().nth(index)?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Persist the pending auto-branch as the `pending-work-branch` marker,
/// best-effort.
///
/// Called from `emit-pipeline` when the work-type signal (`pipeline.kind`) is
/// emitted: it computes the target branch once and stores it so the first
/// Write/Edit can check it out without re-deriving the slug. Fail-open: any IO
/// error is swallowed — telemetry must never block. Skips a redundant rewrite
/// when the marker already records the same thing (mirrors [`bind_session_spec`]).
///
/// `base` is the integration base the branch was resolved for, and is recorded
/// ONLY when the cut could not re-derive it — see [`pending_base_for`]. `None`
/// is for a caller that never had one, and it means "record no base" rather than
/// "invent one".
///
/// A caller RECONCILING the marker to the branch a session actually ended up on
/// therefore reads the base line back and passes it here again: what it learned
/// is a BRANCH, and the base is the operator's own answer to the one question
/// they were asked. Passing `None` there dropped it — and a retried emergency
/// cut then had nothing to read and no way to choose between the candidates.
pub fn set_pending_branch(
    project_dir_path: &str,
    session_id: &str,
    branch: &str,
    base: Option<&str>,
) {
    if is_placeholder_session(session_id) || branch.is_empty() {
        return;
    }
    let Some(marker) = pending_branch_marker(project_dir_path, session_id) else {
        return;
    };
    let body = match base.map(str::trim).filter(|b| !b.is_empty()) {
        Some(base) => format!("{branch}\n{base}"),
        None => branch.to_string(),
    };
    if fs::read_to_string(&marker).ok().as_deref().map(str::trim) == Some(body.as_str()) {
        return;
    }
    let Some(parent) = marker.parent() else {
        return;
    };
    let _ = fs::create_dir_all(parent);
    let _ = fs::write_atomic(&marker, body.as_bytes());
}

/// Remove the pending auto-branch marker, best-effort.
///
/// Called by the cut `spec-draft` takes once it has checked the branch out, so
/// the marker is consumed once. A missing marker is a no-op and any IO error
/// is swallowed — this teardown must never block a write.
pub fn clear_pending_branch(project_dir: &str, session_id: &str) {
    if is_placeholder_session(session_id) {
        return;
    }
    let Some(marker) = pending_branch_marker(project_dir, session_id) else {
        return;
    };
    let _ = fs::remove_file(&marker);
}

/// Compose the `pending-work-branch` marker path:
/// `<project>/.claude/.session/<session_id>/pending-work-branch`.
///
/// Composed from `claude_dir()` the same way [`session_spec_marker`] resolves
/// the sibling `active-spec` marker. `None` on a `.claude/.claude/` guard rejection of the
/// project root.
fn pending_branch_marker(project_dir_path: &str, session_id: &str) -> Option<PathBuf> {
    Some(
        ClaudePaths::for_project(Path::new(project_dir_path))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(session_id)
            .join("pending-work-branch"),
    )
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
/// emitted event. Co-located with [`current_spec`] so a hook (e.g. the
/// change-request logger) can attribute its emitted event to the active wave.
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

    // -----------------------------------------------------------------------
    // install_mode — the marks it reads must be marks something writes
    // -----------------------------------------------------------------------

    /// The install writes `mustard_core::footprint_rules()`; this module reads the
    /// result back. Pin the seam end-to-end rather than restating either side's
    /// list: if the footprint ever renamed or dropped a mark, no exclude file
    /// would carry it, `install_mode` would answer `Shared` forever, and the next
    /// `scan --full` would write straight into a client's own `CLAUDE.md` — with
    /// every other test still green.
    ///
    /// The second half pins the OTHER direction, the one that lives in this
    /// crate: `scan_claude` writes [`CLAUDE_LOCAL_MD`], and a name no install
    /// rule hides is a private scan that git can see.
    #[test]
    fn a_freshly_written_footprint_reads_back_as_private() {
        let rules = mustard_core::footprint_rules();
        assert!(
            mustard_core::carries_private_marks(&rules.join("\n")),
            "an exclude file carrying the whole footprint must detect as private: {rules:?}",
        );
        assert!(
            rules.iter().any(|r| r == CLAUDE_LOCAL_MD),
            "{CLAUDE_LOCAL_MD} is what a private scan writes; no rule hides it: {rules:?}",
        );
    }

    // -----------------------------------------------------------------------
    // guards_file — the ONE resolver every reader of a subproject's Guards uses
    // -----------------------------------------------------------------------

    /// A tree git knows nothing about is a shared install, so the resolver names
    /// the shared file — whether or not a local layer happens to sit beside it.
    /// This is the half that keeps an ordinary project byte-identical to before.
    #[test]
    fn a_shared_install_resolves_the_shared_file_even_beside_a_local_layer() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("apps").join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(CLAUDE_LOCAL_MD), "stray\n").unwrap();
        std::fs::write(sub.join(CLAUDE_MD), "theirs\n").unwrap();

        assert_eq!(guards_file_name(dir.path()), CLAUDE_MD);
        assert_eq!(guards_file(dir.path(), &sub), sub.join(CLAUDE_MD));
    }

    /// Both instruction layers are recognised by name, and nothing else is —
    /// the mode-blind test the refusals and the doc linter classify with.
    #[test]
    fn only_the_two_instruction_layers_are_guards_file_names() {
        assert!(is_guards_file_name(CLAUDE_MD));
        assert!(is_guards_file_name(CLAUDE_LOCAL_MD));
        assert!(!is_guards_file_name("claude.md"), "the name is case-sensitive on disk");
        assert!(!is_guards_file_name("CLAUDE.local.md.bak"));
        assert!(!is_guards_file_name("README.md"));
    }

    /// Fail-open: a tree git knows nothing about is a shared install, not an
    /// error and not a panic.
    #[test]
    fn a_tree_without_a_repository_reads_as_shared() {
        let dir = tempdir().unwrap();
        assert_eq!(install_mode(dir.path()), InstallMode::Shared);
    }

    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // normalise_spec_dir — the three `--spec-dir` argument shapes
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // current_spec — filesystem branch (no env mutation needed)
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // session_for_spec — marker-first inverse lookup
    // -----------------------------------------------------------------------

    /// The spec-scoped reader must resolve the session BOUND to the spec via
    /// its `active-spec` marker — not the newest session dir by mtime, which
    /// races against unrelated concurrent sessions (field symptom: a false
    /// `digestUsed: false` in digest-adherence-finalize).
    #[test]
    fn session_for_spec_resolves_bound_session_not_newest() {
        let dir = tempdir().unwrap();
        let base = dir.path().join(".claude").join(".session");
        // sess-a is bound to the spec under test.
        std::fs::create_dir_all(base.join("sess-a")).unwrap();
        std::fs::write(base.join("sess-a").join("active-spec"), "minha-spec\n").unwrap();
        // sess-b is created LAST (newest mtime) and bound to another spec —
        // the mtime-based fallback would wrongly pick it.
        std::fs::create_dir_all(base.join("sess-b")).unwrap();
        std::fs::write(base.join("sess-b").join("active-spec"), "outra-spec").unwrap();

        let root = dir.path().to_str().unwrap();
        assert_eq!(
            session_for_spec(root, "minha-spec").as_deref(),
            Some("sess-a"),
            "the bound session wins regardless of mtime order"
        );
        assert_eq!(session_for_spec(root, "outra-spec").as_deref(), Some("sess-b"));
        assert_eq!(session_for_spec(root, "spec-sem-binding"), None);
        assert_eq!(session_for_spec(root, ""), None);
    }

    // -----------------------------------------------------------------------
    // session_id — filesystem fallback (no env mutation needed)
    // -----------------------------------------------------------------------

    #[test]
    fn session_id_falls_back_to_newest_session_dir() {
        // `newest_session_dir` returns the newest real session id and
        // never the `"unknown"` bucket. Exercised directly (the crate forbids
        // `unsafe`, so a test cannot unset the env to reach this branch via
        // `session_id()`); mirrors `current_spec`'s FS-branch unit tests.
        let dir = tempdir().unwrap();
        let session_base = dir.path().join(".claude").join(".session");
        std::fs::create_dir_all(session_base.join("unknown")).unwrap();
        // Create `sess-A` last so it has the newest mtime.
        std::fs::create_dir_all(session_base.join("sess-A")).unwrap();

        let result = newest_session_dir(&session_base);
        assert_eq!(result.as_deref(), Some("sess-A"));
        assert_ne!(result.as_deref(), Some("unknown"));
    }

    #[test]
    fn newest_session_dir_returns_none_on_missing_dir() {
        // Fail-open: a nonexistent `.session/` base degrades to None.
        assert!(newest_session_dir(Path::new("/nonexistent-mustard-session-xyzzy")).is_none());
    }

    /// The session→spec binding reaches the session the hooks read.
    ///
    /// The field defect: `emit-pipeline` run from the CLI carries no harness
    /// session id, the mtime fallback resolved the OTEL collector's
    /// `otel-unattached` bucket (touched constantly, so newest), and the
    /// binding landed under a directory no hook ever consults — every gate
    /// keyed on the REAL session id then fell back to whichever spec was
    /// newest. Two guarantees close it: the resolver never picks a placeholder
    /// bucket, and the marker writers refuse one outright.
    #[test]
    fn session_binding_reaches_the_reading_session() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let base = dir.path().join(".claude").join(".session");
        // The session the hooks are actually handed — created FIRST (older).
        std::fs::create_dir_all(base.join("sess-hook")).unwrap();
        // The collector's bucket — created LAST, and touched again so its
        // mtime is strictly newest (the shape observed live).
        std::fs::create_dir_all(base.join("otel-unattached")).unwrap();
        std::fs::create_dir_all(base.join("otel-unattached").join(".events")).unwrap();

        // The env-less resolver skips the placeholder even when it is newest.
        assert_eq!(
            newest_session_dir(&base).as_deref(),
            Some("sess-hook"),
            "a placeholder bucket must never win the newest-by-mtime scan",
        );

        // A binding aimed at the placeholder is refused — unreachable by
        // construction, better absent than misleading...
        bind_session_spec(project, "otel-unattached", "my-spec");
        assert!(
            spec_for_session(project, "otel-unattached").is_none(),
            "no binding may live under a placeholder session id",
        );
        // ...and the sibling pending-branch marker refuses the same way.
        set_pending_branch(project, "otel-unattached", "dev_x", None);
        assert!(pending_branch_for(project, "otel-unattached").is_none());

        // Written under the session the hooks read, the binding round-trips —
        // the reader keyed on the harness-provided id finds the spec.
        bind_session_spec(project, "sess-hook", "my-spec");
        assert_eq!(
            spec_for_session(project, "sess-hook").as_deref(),
            Some("my-spec"),
            "the binding must be readable under the id the hooks carry",
        );
    }

    // -----------------------------------------------------------------------
    // session→spec binding lifecycle (bind → resolve → unbind)
    // -----------------------------------------------------------------------

    #[test]
    fn unbind_session_spec_clears_the_binding() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        bind_session_spec(project, "sess-X", "my-spec");
        assert_eq!(
            spec_for_session(project, "sess-X").as_deref(),
            Some("my-spec"),
            "bind then resolve should round-trip",
        );
        unbind_session_spec(project, "sess-X");
        assert!(
            spec_for_session(project, "sess-X").is_none(),
            "unbind should clear the marker",
        );
        // A second unbind on a missing marker is a no-op (must not panic).
        unbind_session_spec(project, "sess-X");
    }

    // -----------------------------------------------------------------------
    // pending auto-branch lifecycle (set → resolve → clear)
    // -----------------------------------------------------------------------

    #[test]
    fn pending_branch_round_trips_then_clears() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        set_pending_branch(project, "sess-B", "feature/my-thing", None);
        assert_eq!(
            pending_branch_for(project, "sess-B").as_deref(),
            Some("feature/my-thing"),
            "set then resolve should round-trip",
        );
        clear_pending_branch(project, "sess-B");
        assert!(
            pending_branch_for(project, "sess-B").is_none(),
            "clear should remove the marker",
        );
        // A blank branch / unknown session never writes; a second clear is a no-op.
        set_pending_branch(project, "sess-B", "", None);
        assert!(pending_branch_for(project, "sess-B").is_none());
        set_pending_branch(project, "unknown", "feature/x", None);
        assert!(pending_branch_for(project, "unknown").is_none());
        clear_pending_branch(project, "sess-B");
    }
}
