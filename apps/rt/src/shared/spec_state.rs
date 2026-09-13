//! `spec_state` — the ONE ladder that names the current spec, read from disk.
//!
//! The ladder itself is pure and lives in the core
//! (`mustard_core::domain::spec_state::resolve`): the `MUSTARD_ACTIVE_SPEC`
//! override, then the spec of the branch the checkout stands on, then the spec
//! the session is bound to. This module only reads each rung — the environment,
//! `.git/HEAD` and the session's `active-spec` marker — and hands them over.
//!
//! Every door that asks "which spec is this" goes through [`active_spec`], so
//! two doors can never name different specs for the same session. A leftover
//! `.pipeline-states/` file names nothing.
//!
//! [`DiskSpecState`] is the disk side of the core's `SpecState` port: that
//! ladder, and each spec's state folded from its `spec.ndjson`.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{resolve, SpecState, State};
use mustard_core::io::spec_events as store;

use crate::shared::context;

/// The session id the `run` face was handed through the environment:
/// `MUSTARD_SESSION_ID`, then `CLAUDE_SESSION_ID`. Never a guess from the
/// newest session folder, which could name another session's spec.
#[must_use]
pub(crate) fn session_from_env() -> Option<String> {
    ["MUSTARD_SESSION_ID", "CLAUDE_SESSION_ID"].iter().find_map(|key| {
        std::env::var(key).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    })
}

/// The current spec for `session` in the project at `root`, fail-open `None`:
/// the environment override, then the spec of the checkout's branch, then the
/// spec the session is bound to.
#[must_use]
pub(crate) fn active_spec(root: &str, session: Option<&str>) -> Option<String> {
    let env = std::env::var("MUSTARD_ACTIVE_SPEC").ok();
    let branch = context::spec_of_checkout_branch(root);
    let bound = session.and_then(|sid| context::spec_for_session(root, sid));
    resolve(env.as_deref(), branch, bound)
}

/// The disk side of the core's [`SpecState`] port: the ladder of
/// [`active_spec`] over the checkout at `root`, and each spec's state folded
/// from its `spec.ndjson` (the main checkout's, when `root` is a linked
/// worktree).
pub(crate) struct DiskSpecState {
    root: PathBuf,
}

impl DiskSpecState {
    /// The port over the checkout at `root`.
    #[must_use]
    pub(crate) fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }
}

impl SpecState for DiskSpecState {
    fn active(&self, session: Option<&str>) -> Option<String> {
        active_spec(&self.root.to_string_lossy(), session)
    }

    fn state(&self, spec: &str) -> State {
        self.log(spec).map_or_else(State::absent, |log| State::from_log(&log))
    }

    fn log(&self, spec: &str) -> Option<SpecLog> {
        let path = store::spec_file(&store::spec_root(&self.root), spec).ok()?;
        store::read(&path).ok().flatten()
    }
}

/// Stand the checkout at `root` on the branch of `spec` — a `.git/HEAD` naming
/// `feature/<spec>` and the spec's folder — so the branch rung answers `spec`
/// without touching the process environment.
#[cfg(test)]
pub(crate) fn stand_on_spec_branch(root: &std::path::Path, spec: &str) {
    let git = root.join(".git");
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(git.join("HEAD"), format!("ref: refs/heads/feature/{spec}\n")).unwrap();
    std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::HookInput;
    use tempfile::tempdir;

    const SESSION: &str = "s-lado-a-lado";

    /// Every door's own resolver, called the way the door calls it.
    fn every_door(root: &str) -> Vec<(&'static str, Option<String>)> {
        let input = HookInput { session_id: Some(SESSION.to_string()), ..HookInput::default() };
        vec![
            (
                "clarification_observer",
                crate::hooks::observe::clarification_observer::active_unit(root, &input),
            ),
            (
                "change_request",
                crate::commands::spec::change_request::resolve_spec(root, None, Some(SESSION)),
            ),
            (
                "change_request_log",
                crate::hooks::observe::change_request_log::resolve_spec(root, Some(SESSION)),
            ),
            (
                "boundary_gate",
                crate::hooks::write::boundary_gate::resolve_boundary_spec(root, Some(SESSION)),
            ),
            ("subagent_inject", crate::hooks::task::subagent_inject::capture_spec(root, SESSION)),
            ("pr_detect", crate::hooks::bash::pr_detect::detect_recent_spec(root, Some(SESSION))),
            ("grill_capture", crate::commands::grill_capture::finalize_spec(root, "", Some(SESSION))),
            ("route", crate::shared::events::route::spec_of_event(None, root, Some(SESSION))),
            (
                "post_edit",
                crate::hooks::write::post_edit::find_active_spec(root, Some(SESSION))
                    .map(|(_, name)| name),
            ),
        ]
    }

    /// A spec folder with a `spec.md`, which the checklist door also needs.
    fn spec_with_md(root: &std::path::Path, spec: &str) {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), "# Spec\n").unwrap();
    }

    fn assert_all_name(root: &str, expected: &str) {
        for (door, got) in every_door(root) {
            assert_eq!(got.as_deref(), Some(expected), "the {door} door named another spec");
        }
    }

    #[test]
    fn every_door_names_the_same_spec_for_the_same_session() {
        // An inherited override answers first at every door by design; the
        // rungs below it are what is under test.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        // The checkout stands on one spec's branch while the session is bound
        // to another: the branch wins at every door.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        spec_with_md(root, "da-branch");
        spec_with_md(root, "da-sessao");
        stand_on_spec_branch(root, "da-branch");
        context::bind_session_spec(root_str, SESSION, "da-sessao");
        assert_all_name(root_str, "da-branch");

        // Off any spec branch, the session binding answers at every door.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        spec_with_md(root, "da-sessao");
        context::bind_session_spec(root_str, SESSION, "da-sessao");
        assert_all_name(root_str, "da-sessao");
    }

    #[test]
    fn the_session_rung_answers_only_for_the_session_it_is_given() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        context::bind_session_spec(root, SESSION, "da-sessao");
        assert_eq!(active_spec(root, Some(SESSION)).as_deref(), Some("da-sessao"));
        assert_eq!(active_spec(root, Some("outra-sessao")), None);
        assert_eq!(active_spec(root, None), None);
    }

    #[test]
    fn a_spec_without_its_event_file_has_no_state() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join("sem-arquivo")).unwrap();
        let disk = DiskSpecState::new(dir.path());
        assert!(disk.state("sem-arquivo").is_absent(), "no event file, no state");
        assert!(disk.log("sem-arquivo").is_none());
    }

    #[test]
    fn the_disk_state_folds_the_state_events_of_the_spec_file() {
        let dir = tempdir().unwrap();
        let path = store::spec_file(dir.path(), "com-arquivo").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let draft = |v: serde_json::Value| v.as_object().cloned().unwrap();
        store::write(
            &path,
            "state",
            draft(serde_json::json!({"phase": "plan", "branch": "feature/com-arquivo", "base": "dev"})),
            &[],
        )
        .unwrap();
        store::write(
            &path,
            "state",
            draft(serde_json::json!({
                "phase": "approved",
                "witness": {"question": "Aprova?", "answer": "Aprovar"}
            })),
            &[],
        )
        .unwrap();

        let state = DiskSpecState::new(dir.path()).state("com-arquivo");
        assert_eq!(state.phase, Some("approved"));
        assert!(state.approved);
        assert_eq!(state.branch.as_deref(), Some("feature/com-arquivo"), "the branch is inherited");
        assert_eq!(state.base.as_deref(), Some("dev"));
    }
}
