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
    opt_out_from(std::env::var("MUSTARD_DASHBOARD_REGISTER").ok().as_deref())
}

/// The rule alone, as a function of the value.
///
/// Split from the environment read so the accepted words can be asserted
/// without a test mutating a process-global variable — tests run in parallel,
/// and one that sets `MUSTARD_DASHBOARD_REGISTER` would decide the answer for
/// whichever sibling happened to read it at that moment.
fn opt_out_from(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    })
}

/// Register `root` into the registry at `registry`, when `root` is a Mustard
/// project.
///
/// **The registry path is a PARAMETER, and that is the whole point.** With the
/// home directory resolved inside, the only thing a test could assert was the
/// early-return branch: driving the writing branch would have written into the
/// operator's real `~/.claude/dashboard-projects.json`. So the test that named
/// a registry read a path nothing ever wrote to, and passed for that reason —
/// it would have passed just as well if this function had written to the
/// operator's home (found in review). The seam already existed one layer down
/// (`dashboard_registry::register_at`); this mirrors it, which is exactly how
/// the core made the same behaviour testable.
///
/// Fail-open at every step.
pub(crate) fn register_project_at(registry: &std::path::Path, root: &std::path::Path) {
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
    let _ = mustard_core::dashboard_registry::register_at(registry, &absolute);
}

/// Register `root` with the MACHINE registry — the production path.
///
/// Thin: it resolves where the registry lives and hands the decision to
/// [`register_project_at`]. An unresolvable home directory is a silent no-op,
/// for the same reason the write failure is.
pub(crate) fn register_project(root: &std::path::Path) {
    let Some(registry) = mustard_core::dashboard_registry::registry_path() else {
        return;
    };
    register_project_at(&registry, root);
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
        // The rule is spelled here rather than borrowed. The equivalent helper
        // is `hooks::task::common::project_dir_opt`, and the FUNCTION is
        // `pub(crate)` — it is the MODULE around it that is private, which is
        // what the compiler refuses. Widening a sibling family's module to
        // reach across families costs more than the three lines it saves, and
        // this form is a superset anyway: it prefers `ctx.workspace_root` when
        // the harness resolved one.
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

    /// A directory that carries `mustard.json`, i.e. a project the harness
    /// considers its own.
    fn mustard_project(parent: &std::path::Path, name: &str) -> std::path::PathBuf {
        let dir = parent.join(name);
        std::fs::create_dir_all(&dir).expect("project dir");
        std::fs::write(dir.join("mustard.json"), r#"{"version":"0.1.54"}"#).expect("config");
        assert!(
            mustard_core::ProjectConfig::exists(&dir),
            "the fixture must look like a Mustard project"
        );
        dir
    }

    /// THE claim this observer exists to make, and the one the mold demands be
    /// asserted: an already-installed project announces itself, and the row it
    /// writes is the row the dashboard will render.
    ///
    /// Written against an injected registry path. The previous version of this
    /// test read a file nothing wrote to, so it passed without ever driving the
    /// writing branch — it would have passed just as well had the observer
    /// written into the operator's real home (found in review).
    #[test]
    fn an_installed_project_writes_its_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = mustard_project(dir.path(), "my-app");

        register_project_at(&registry, &project);

        let rows = dashboard_registry::read_at(&registry);
        assert_eq!(rows.len(), 1, "the project must appear exactly once");
        assert_eq!(rows[0].name, "my-app", "the row carries the folder name");
        assert_eq!(
            rows[0].path,
            project.canonicalize().expect("canonicalize").to_string_lossy(),
            "the row carries the ABSOLUTE path — a relative one resolves \
             differently depending on where the dashboard runs"
        );
        assert!(!rows[0].added_at.is_empty());
    }

    /// The whole point of running every session: the SECOND run must write
    /// nothing, so an established project costs one small read.
    #[test]
    fn a_second_session_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = mustard_project(dir.path(), "my-app");

        register_project_at(&registry, &project);
        let before = std::fs::read_to_string(&registry).expect("read");
        let stamp = std::fs::metadata(&registry).and_then(|m| m.modified()).ok();

        register_project_at(&registry, &project);

        assert_eq!(
            before,
            std::fs::read_to_string(&registry).expect("read"),
            "a second session must change nothing"
        );
        assert_eq!(
            stamp,
            std::fs::metadata(&registry).and_then(|m| m.modified()).ok(),
            "a second session must not even rewrite the file"
        );
        assert_eq!(dashboard_registry::read_at(&registry).len(), 1);
    }

    /// Two different projects both land, in the order they were seen.
    #[test]
    fn each_project_gets_its_own_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let alpha = mustard_project(dir.path(), "alpha");
        let beta = mustard_project(dir.path(), "beta");

        register_project_at(&registry, &alpha);
        register_project_at(&registry, &beta);

        let names: Vec<String> = dashboard_registry::read_at(&registry)
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }

    /// A plain directory is not a Mustard project and announces nothing — now
    /// asserted against the registry the call actually targets.
    #[test]
    fn a_non_mustard_directory_registers_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let plain = dir.path().join("just-a-folder");
        std::fs::create_dir_all(&plain).expect("dir");

        register_project_at(&registry, &plain);

        assert!(!registry.exists(), "no project, no file");
        assert!(dashboard_registry::read_at(&registry).is_empty());
    }

    /// The operator's switch, read as a rule rather than through the process
    /// environment — see [`opt_out_from`].
    #[test]
    fn the_opt_out_is_off_by_default_and_reads_falsey_words() {
        assert!(!opt_out_from(None), "unset means registration is ON");
        for word in ["0", "false", "no", "off", "FALSE", " off "] {
            assert!(opt_out_from(Some(word)), "{word} should opt out");
        }
        for word in ["1", "true", "yes", "on", ""] {
            assert!(!opt_out_from(Some(word)), "{word} should NOT opt out");
        }
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
