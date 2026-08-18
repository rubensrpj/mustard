//! `mustard-rt run upsert` — install or update Mustard in the current project.
//!
//! The plugin's bootstrap door: everything the harness needs in a project —
//! `.claude/settings.json`, the injectable instruction files under
//! `.claude/mustard/`, `.claude/.gitignore`, and the project-root
//! `mustard.json` — is seeded by `mustard_core::upsert_project`, idempotent
//! and always merge-mode (an existing user file is preserved; only what is
//! missing is created or backfilled). The legacy planted-orchestrator
//! footprint is migrated away in the same pass.
//!
//! Output: the serialized `UpsertReport` as pretty JSON — deterministic
//! (fixed field order, no timestamps, project-root-relative names only), per
//! the `run`-face byte-stability contract. Fail-open: an engine error is
//! reported as a JSON `{"error": …}` object and the process still exits 0.
//!
//! The footprint is ALWAYS the one that stays invisible to the host
//! repository's git. There is no flag and no mode to choose: an install that
//! versions the harness into a repository is not something this door can be
//! asked for, so no argv, no config and no forgotten default can produce one.
//!
//! The one loud failure: when a private install cannot hide itself in a
//! repository that exists, the engine writes NOTHING and the error is narrated
//! on stderr as well as reported in the JSON. Every other degradation here costs
//! a feature; that one costs the operator's belief that a client's git cannot
//! see the harness, and it is the failure they cannot notice for themselves.

use std::path::PathBuf;

use mustard_core::InstallMode;

/// Execute `mustard-rt run upsert`.
///
/// The `mustard.json#version` stamp is [`mustard_core::harness_version`] —
/// the installed plugin's manifest version when launched by the plugin
/// (`CLAUDE_PLUGIN_ROOT`), the core crate's own version otherwise. The field
/// records "which harness last set this project up"; a legacy 3.1.x CLI stamp
/// reads as drift once and this very command realigns it.
pub fn run() {
    // Workspace-root walk first (an already-installed project resolves to its
    // anchor even from a subdirectory), then `CLAUDE_PROJECT_DIR`, then the
    // process cwd — the fresh-install path, where no anchor exists yet.
    let root = PathBuf::from(crate::shared::context::project_dir());

    // Unconditional. The mode is not read from anywhere and not asked for
    // anywhere: a harness that installs itself into someone else's repository
    // is the failure this door exists to make unreachable, and a knob that can
    // reach it is the same failure with an extra step.
    let mode = InstallMode::Private;

    let version = mustard_core::harness_version();
    match mustard_core::upsert_project(&root, Some(&version), mode) {
        Ok(report) => {
            let json = serde_json::to_string_pretty(&report)
                .unwrap_or_else(|e| format!("{{\"error\": \"serializing report: {e}\"}}"));
            println!("{json}");
        }
        Err(err) => {
            // Fail-open: report the failure as JSON, exit 0 (the run face
            // never signals through the exit code).
            let json = serde_json::json!({ "error": err.to_string() });
            println!("{json}");
            // One failure is not a machine's problem alone. A private install
            // that could not hide leaves the operator believing a client's git
            // cannot see the harness — the one thing they cannot check for
            // themselves — so it is narrated on stderr as well, where a person
            // reads. stdout stays the byte-stable JSON the run face contracts.
            if matches!(err, mustard_core::platform::error::Error::NotHidden(_)) {
                eprintln!(
                    "\n  NOTHING WAS INSTALLED.\n\
                     \n\
                     A private install hides its footprint in this clone's exclude file, and that\n\
                     file could not be used. Every file the install would seed — including the one\n\
                     naming the harness — would have been visible in this repository's `git status`\n\
                     while the report called itself private.\n\
                     \n\
                     Make the exclude file readable and writable (it must be a FILE), then re-run.\n"
                );
            }
        }
    }
}
