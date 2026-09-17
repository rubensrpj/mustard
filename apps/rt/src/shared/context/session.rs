//! A sessão: qual é a atual, para um comando `run`, e a spec a que cada
//! sessão está ligada (a marca `active-spec`).

use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::env::project_dir;

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
pub(super) fn is_placeholder_session(id: &str) -> bool {
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

/// Resolve the spec a session is currently bound to, fail-open `None`.
///
/// Hook-emitted events (`tool.use`, `agent.*`, …) are born with no spec — the
/// PostToolUse hook context never sets `MUSTARD_ACTIVE_SPEC`. The
/// only reliable binding is the `pipeline.scope` event the CLI run-face emits,
/// which carries BOTH `session_id` and `spec`. Rather than scan the NDJSON log
/// on every tool call, the router persists that binding as a small marker file
/// (o marcador `active-spec`); this reads it back in O(1).
///
/// Marker location: `.claude/.session/<session_id>/active-spec` — beside the
/// session's own `.events/` directory.
///
/// Returns `None` when the session has no recorded binding (no marker yet, an
/// empty/`"unknown"` session id, or any IO error) — never panics.
#[must_use]
pub fn spec_for_session(project_dir_path: &str, session_id: &str) -> Option<String> {
    // Per-dispatch memo of the marker read; evicted on any binding change via
    // `invalidate_session_spec` (see `unbind_session_spec`).
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

/// Persist the session→spec binding as the `active-spec` marker, best-effort.
///
/// Sem chamador na produção desde a refatoração que enxugou o runtime: quem
/// escreve a ligação hoje são os testes dos portões que a LEEM por
/// [`spec_for_session`], e por isso ela mora sob `cfg(test)` em vez de sair.
/// Fail-open: qualquer erro de IO é engolido.
///
/// A placeholder session id ([`is_placeholder_session`]) is REFUSED: a binding
/// written under a bucket no hook is ever handed is unreachable, and the gates
/// keyed on it would silently fall back to whichever spec is newest.
#[cfg(test)]
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
/// o mesmo marcador `active-spec` da ligação e o apaga.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::context::pending_branch::{pending_branch_for, set_pending_branch};
    use tempfile::tempdir;

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
}
