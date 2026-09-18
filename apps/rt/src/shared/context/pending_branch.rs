//! A branch de trabalho pendente de uma sessão (a marca
//! `pending-work-branch`): a branch que a primeira escrita deve abrir e a base
//! para a qual ela foi escolhida.

use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

use super::session::is_placeholder_session;

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
