//! Resume-mode decision + refresh signal.
//!
//! Three answers, all degrading to the conservative path when the evidence is
//! missing:
//! - [`decide_mode`] — `continued` / `reanalyzed` / `ask` from the pipeline
//!   state view + dispatch-failure presence.
//! - [`compute_needs_refresh`] — whether a `pipeline.wave.complete` landed
//!   after the last `pipeline.resume_mode`, plus that resume event's age (used
//!   by [`super::run`] to debounce re-emission).
//! - [`inside_own_work_branch`] — whether the checkout ALREADY is this spec's
//!   own `{base}_{slug}` branch. A FACT, not a routing decision: what to skip
//!   because of it (the table, the header, the *implement now* confirm) is the
//!   picker's judgement and stays in the picker.

use super::{AUTO_CONTINUE_TTL_MS, PipelineDispatchFailurePayload, PipelineStateView};
use crate::commands::event::work_branch::{compute_work_branch, current_branch};
use mustard_core::domain::model::event::{
    EVENT_PIPELINE_RESUME_MODE, EVENT_PIPELINE_WAVE_COMPLETE,
};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;
use mustard_core::EventReader;
use std::path::{Path, PathBuf};

/// Returns `(needs_refresh, last_resume_mode_age_ms)`.
///
/// `needs_refresh` is `true` when at least one `pipeline.wave.complete` event
/// landed since the most recent `pipeline.resume_mode` event for this spec.
///
/// Reads from the per-spec NDJSON events dir (`.claude/spec/{spec}/.events/`).
/// Fail-open: an unreadable dir returns `(false, None)`.
pub(super) fn compute_needs_refresh(project: &Path, spec: &str) -> (bool, Option<i64>) {
    let events_dir = match ClaudePaths::for_project(project)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
        .map(|sp| sp.events_dir())
    {
        Some(d) => d,
        None => return (false, None),
    };

    let now_ms = i64::try_from(mustard_core::time::now_unix_millis() as u128).unwrap_or(i64::MAX);

    // Collect all NDJSON events from the dir.
    let ndjson_files: Vec<PathBuf> = mfs::read_dir(&events_dir)
        .ok()
        .into_iter()
        .flatten()
        .map(|e| e.path)
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ndjson"))
        .collect();

    let mut last_resume_ts: Option<String> = None;
    let mut last_wave_complete_ts: Option<String> = None;

    for path in &ndjson_files {
        for ev in EventReader::stream(path) {
            let ts = ev
                .raw
                .get("ts")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            match ev.kind.as_str() {
                k if k == EVENT_PIPELINE_RESUME_MODE => {
                    if ts.as_deref() > last_resume_ts.as_deref() {
                        last_resume_ts = ts;
                    }
                }
                k if k == EVENT_PIPELINE_WAVE_COMPLETE => {
                    if ts.as_deref() > last_wave_complete_ts.as_deref() {
                        last_wave_complete_ts = ts;
                    }
                }
                _ => {}
            }
        }
    }

    let last_resume_ms = last_resume_ts
        .as_deref()
        .and_then(mustard_core::time::parse_iso_millis);
    let last_resume_age = last_resume_ms.map(|ms| now_ms - ms);

    // Needs refresh when there is a wave.complete that is newer than the last resume_mode.
    let needs = match (last_resume_ts.as_deref(), last_wave_complete_ts.as_deref()) {
        (Some(resume_ts), Some(wave_ts)) => wave_ts > resume_ts,
        (None, Some(_)) => true,
        _ => false,
    };

    (needs, last_resume_age)
}

/// Decide the resume mode from the view + dispatch failure state.
///
/// - `continued` — recent events, no dispatch failure, status is in-progress.
/// - `reanalyzed` — pipeline was abandoned (no events for a while) AND no
///   dispatch failure.
/// - `ask` — dispatch failure present OR no state at all.
pub(super) fn decide_mode(
    view: Option<&PipelineStateView>,
    dispatch_failure: Option<&PipelineDispatchFailurePayload>,
) -> String {
    if dispatch_failure.is_some() {
        return "ask".to_string();
    }
    let Some(v) = view else {
        return "ask".to_string();
    };
    let last_ts = v
        .tasks
        .iter()
        .filter_map(|t| t.dispatched_at.clone())
        .max();
    let now_ms = i64::try_from(mustard_core::time::now_unix_millis() as u128).unwrap_or(i64::MAX);
    let age_ms = last_ts
        .as_deref()
        .and_then(mustard_core::time::parse_iso_millis)
        .map(|at| now_ms - at);
    match age_ms {
        Some(ms) if ms <= AUTO_CONTINUE_TTL_MS => "continued".to_string(),
        Some(_) => "reanalyzed".to_string(),
        // No task dispatch yet — orchestrator decides.
        None => "ask".to_string(),
    }
}

/// `true` when the checkout IS this spec's own work branch — the
/// `{base}_{slug}` name for one of the project's `git.flow` integration bases.
///
/// The work unit is the branch plus everything the work produced: the spec, its
/// waves, its ceremony and the code. So a caller standing on that branch is
/// already inside the unit it is asking about — there is no spec to pick, no
/// stage to introduce and nothing to confirm. The picker reads this to resume
/// with no ceremony; the DECISION of what ceremony to drop stays there, this
/// only reports where the tree is.
///
/// The name is not re-derived: [`compute_work_branch`] is the same function
/// that minted the pending marker, so the two spellings cannot drift.
///
/// `false` for every uncertainty — an empty spec, a VCS opt-out, a directory
/// that is not a repository, a detached HEAD. A resume that cannot SHOW it is
/// inside the unit takes the ceremony, which is what it always did.
pub(super) fn inside_own_work_branch(project: &Path, spec: &str) -> bool {
    if spec.trim().is_empty() {
        return false;
    }
    let config = mustard_core::ProjectConfig::load(project);
    let Some(vcs) = config.vcs() else {
        return false;
    };
    let root = project.to_string_lossy().into_owned();
    let Some(current) = current_branch(&vcs, &root) else {
        return false;
    };
    config
        .git
        .integration_bases()
        .iter()
        .any(|base| compute_work_branch(base, spec, None, "", "", &root) == current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Run a git command in `root`, asserting success — test scaffolding only.
    fn git(root: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    /// A repo whose single commit lives on `base`, with `git.flow` declared so
    /// the base set is DERIVED (nothing here assumes `dev`/`main`).
    fn repo_on(root: &Path, base: &str, flow_json: &str) {
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"git":{{"flow":{flow_json}}}}}"#),
        )
        .unwrap();
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", base]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);
    }

    /// AC-3 — a resume asked for from inside the unit's own branch is
    /// RECOGNISED as such, which is what lets the picker drop the ceremony.
    ///
    /// The spec, its waves and its code all live on `{base}_{slug}`, so a caller
    /// standing there is already inside the unit it is naming: nothing to pick,
    /// nothing to introduce, nothing to confirm. Every other position — the
    /// bare base, another unit's branch, a repo-less directory — keeps the
    /// ceremony, because none of them SHOWS the caller is inside this unit.
    #[test]
    fn resume_inside_own_branch_is_recognised_from_the_branch_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repo_on(root, "dev", r#"{"*":"dev","dev":"main"}"#);

        assert!(
            !inside_own_work_branch(root, "my-spec"),
            "the integration base is not the unit's branch",
        );

        git(root, &["checkout", "-b", "dev_my-spec"]);
        assert!(
            inside_own_work_branch(root, "my-spec"),
            "`{{base}}_{{slug}}` for this spec IS the unit's branch",
        );

        // Any declared base counts — the unit could have been cut off `main`.
        git(root, &["checkout", "-b", "main_my-spec"]);
        assert!(
            inside_own_work_branch(root, "my-spec"),
            "the base half is derived from git.flow, not hardcoded to the primary",
        );

        // Another unit's branch is not this one, however similar the name.
        git(root, &["checkout", "-b", "dev_my-spec-two"]);
        assert!(
            !inside_own_work_branch(root, "my-spec"),
            "a neighbouring unit's branch must not answer for this spec",
        );

        // Agnostic: a develop/master project answers against ITS bases.
        let other = tempfile::tempdir().unwrap();
        repo_on(other.path(), "develop_my-spec", r#"{"*":"develop","develop":"master"}"#);
        assert!(inside_own_work_branch(other.path(), "my-spec"));

        // Unmeasurable ⇒ false: a directory that is not a repository keeps the
        // ceremony rather than claiming a position nobody observed.
        let bare = tempfile::tempdir().unwrap();
        assert!(!inside_own_work_branch(bare.path(), "my-spec"));
        assert!(!inside_own_work_branch(root, "  "), "an empty spec names no branch");
    }
}
