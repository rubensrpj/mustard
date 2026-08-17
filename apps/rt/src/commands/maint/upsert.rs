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
//! `--private` selects the footprint that stays invisible to the host
//! repository's git. It is needed only once: every later run reads the mode
//! back off the clone-local exclude file that run wrote (see
//! `shared::context::install_mode`), so the report grows its private half
//! without the operator having to remember a flag.

use std::path::PathBuf;

use mustard_core::InstallMode;

/// Execute `mustard-rt run upsert`.
///
/// The `mustard.json#version` stamp is [`mustard_core::harness_version`] —
/// the installed plugin's manifest version when launched by the plugin
/// (`CLAUDE_PLUGIN_ROOT`), the core crate's own version otherwise. The field
/// records "which harness last set this project up"; a legacy 3.1.x CLI stamp
/// reads as drift once and this very command realigns it.
pub fn run(private: bool) {
    // Workspace-root walk first (an already-installed project resolves to its
    // anchor even from a subdirectory), then `CLAUDE_PROJECT_DIR`, then the
    // process cwd — the fresh-install path, where no anchor exists yet.
    let root = PathBuf::from(crate::shared::context::project_dir());

    // The flag CHOOSES the mode; absent, the project's own exclude file answers.
    // Never the other way round: a `--private` on an already-shared install must
    // switch it, and a re-run without the flag must not silently undo one.
    let mode = if private {
        InstallMode::Private
    } else {
        crate::shared::context::install_mode(&root)
    };

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
        }
    }
}
