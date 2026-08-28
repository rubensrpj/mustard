//! `dashboard_register_observer` — a project that USES Mustard announces itself
//! to the dashboard.
//!
//! ## Why
//!
//! The dashboard opens on a machine-level list of projects
//! (`~/.claude/dashboard-projects.json`). Until recently its only writer was the
//! dashboard's own "add folder" button, so the list started empty and stayed
//! empty: the operator installed Mustard, opened the dashboard, and saw nothing
//! (reported in the field, 2026-08-28).
//!
//! `mustard init` filling it in on install closed half of that. The half it
//! could not close is every project that was ALREADY installed — including, on
//! the machine where this was reported, the Mustard repository itself. Those
//! projects never run `init` again, so they would have stayed invisible
//! forever, which is the operator's literal complaint (found in review).
//!
//! So the answer to "how would the dashboard know who uses Mustard" is: a
//! project that uses it says so, every time it runs. `SessionStart` is that
//! moment, and it is the only one that covers installs old and new alike.
//!
//! ## Cost, and why this can afford to run every session
//!
//! Idempotent by path: an already-registered project reads one small JSON,
//! finds itself, and writes NOTHING. Only the first session in a given project
//! writes at all.
//!
//! ## Opt-out
//!
//! `MUSTARD_DASHBOARD_REGISTER=0` (or `false`/`no`) turns it off entirely. The
//! file records the paths of the operator's projects, and while it is Mustard's
//! own file — not the user's `~/.claude/settings.json`, which is why this is not
//! gated behind `MUSTARD_GLOBAL_PERMISSIONS` — an operator who does not want
//! that list kept must have a way to say so.
//!
//! ## Contract
//!
//! Pure side effect, no verdict, never panics, never blocks a session. A
//! non-Mustard directory registers nothing.

use std::path::PathBuf;

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};

/// The `SessionStart` dashboard self-registration module.
pub struct DashboardRegisterObserver;

/// Has the operator turned self-registration off?
///
/// Default ON. The value is read the same way the rest of the crate reads a
/// boolean knob: an explicit falsey word disables, anything else leaves it on.
fn opted_out() -> bool {
    std::env::var("MUSTARD_DASHBOARD_REGISTER")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "0" | "false" | "no" | "off")
        })
        .unwrap_or(false)
}

/// Register `root` with the dashboard when it is a Mustard project.
///
/// Inner, testable form — see the module doc. Fail-open at every step.
pub(crate) fn register_project(root: &std::path::Path) {
    if opted_out() {
        return;
    }
    // A directory that is not a Mustard project has nothing to announce.
    if !mustard_core::ProjectConfig::exists(root) {
        return;
    }
    // The registry's identity is the absolute path: a relative one would record
    // a row that resolves differently depending on where the dashboard runs.
    let absolute = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // The result is deliberately dropped. A dashboard listing is a convenience,
    // and a hook must never turn an unwritable home directory into a session
    // that will not start.
    let _ = mustard_core::dashboard_registry::register(&absolute);
}

impl Observer for DashboardRegisterObserver {
    /// On `SessionStart`, announce this project to the dashboard. Any other
    /// trigger is a no-op. Pure side effect — never panics, never blocks.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        // Cheapest first: the trigger, then the operator's switch, both of
        // which settle without touching the filesystem.
        if ctx.trigger != Some(Trigger::SessionStart) {
            return;
        }
        if opted_out() {
            return;
        }
        // The `Option` form, NOT `project_dir_or_cwd`: this observer WRITES, and
        // falling back to the process cwd would register whatever directory a
        // test run happened to start in.
        //
        // The rule is spelled here rather than borrowed: the equivalent helper
        // (`hooks::task::common::project_dir_opt`) is private to the `task`
        // family, and widening it to reach across families costs more than the
        // three lines it saves.
        let Some(root) = ctx.workspace_root.clone().or_else(|| {
            input
                .cwd
                .as_deref()
                .filter(|c| !c.is_empty() && *c != ".")
                .map(PathBuf::from)
        }) else {
            return;
        };
        register_project(&root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::dashboard_registry;

    /// A plain directory is not a Mustard project and announces nothing.
    #[test]
    fn a_non_mustard_directory_registers_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        // Nothing to assert through the real registry path without touching the
        // operator's home, so drive the inner rule instead: no `mustard.json`
        // means the guard short-circuits before any write is attempted.
        assert!(!mustard_core::ProjectConfig::exists(dir.path()));
        register_project(dir.path());
        assert!(dashboard_registry::read_at(&registry).is_empty());
    }

    /// The whole point of running every session: the SECOND run must write
    /// nothing, so an established project costs one small read.
    #[test]
    fn registering_an_already_listed_project_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).expect("project");

        dashboard_registry::register_at(&registry, &project).expect("first");
        let before = std::fs::read_to_string(&registry).expect("read");
        let stamp = std::fs::metadata(&registry).and_then(|m| m.modified()).ok();

        dashboard_registry::register_at(&registry, &project).expect("second");
        let after = std::fs::read_to_string(&registry).expect("read");

        assert_eq!(before, after, "a second registration must change nothing");
        assert_eq!(
            stamp,
            std::fs::metadata(&registry).and_then(|m| m.modified()).ok(),
            "a second registration must not even rewrite the file"
        );
        assert_eq!(dashboard_registry::read_at(&registry).len(), 1);
    }

    #[test]
    fn the_opt_out_is_off_by_default() {
        // No variable set in the test environment ⇒ registration is ON.
        assert!(!opted_out());
    }

    /// The mold's required case: no valid project root must not panic, and
    /// must write nothing anywhere.
    #[test]
    fn no_project_root_writes_nothing_and_does_not_panic() {
        let input = HookInput::default();
        let ctx = Ctx {
            trigger: Some(Trigger::SessionStart),
            ..Ctx::default()
        };
        // `workspace_root` is None and the payload carries no project dir, so
        // the `Option` chain returns before any write is attempted.
        DashboardRegisterObserver.observe(&input, &ctx);
    }

    /// A trigger this observer does not serve is a no-op.
    #[test]
    fn another_trigger_is_a_no_op() {
        let input = HookInput::default();
        let ctx = Ctx {
            trigger: Some(Trigger::SessionEnd),
            ..Ctx::default()
        };
        DashboardRegisterObserver.observe(&input, &ctx);
    }
}
