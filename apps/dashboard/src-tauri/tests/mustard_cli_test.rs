//! Smoke test for the dashboard `mustard-cli` path dependency (Mustard 2.0).
//!
//! The dashboard links `mustard-cli` to reuse its library API natively (no
//! sidecar process). This test drives `init` from the dashboard crate test
//! context — proving the path dependency is linked and the non-interactive
//! bootstrap runs to completion without a terminal. Because `init` is now
//! idempotent (it subsumes the retired `mustard update`), a second run must
//! re-seed an already-initialized project without error.

use std::fs;

use mustard_cli::commands::init::{InitOptions, init_with_templates};

/// Build a minimal fake `templates/` payload `init` can seed from. The thin
/// Mustard 2.0 init reads only `CLAUDE.md`, `settings.json`, and `.gitignore`;
/// the content payload (commands/skills/agents/refs) ships in the plugin now.
fn fake_templates(root: &std::path::Path) -> std::path::PathBuf {
    let templates = root.join("templates");
    fs::create_dir_all(&templates).unwrap();
    fs::write(templates.join("CLAUDE.md"), "# rules").unwrap();
    fs::write(templates.join("settings.json"), r#"{"env":{"MUSTARD_TEST":"1"}}"#).unwrap();
    fs::write(templates.join(".gitignore"), "spec/*/.events/
").unwrap();
    templates
}

#[test]
fn init_runs_non_interactively_and_is_idempotent() {
    let work = tempfile::tempdir().unwrap();
    let templates = fake_templates(work.path());
    let project = work.path().join("project");
    fs::create_dir_all(&project).unwrap();

    // Non-interactive init: seed a fresh project the way the dashboard would.
    init_with_templates(
        &project,
        &templates,
        &InitOptions { yes: true, ..InitOptions::default() },
    )
    .expect("init should run without a terminal");

    let claude = project.join(".claude");
    assert!(claude.join("CLAUDE.md").exists(), ".claude/ scaffolded");
    assert!(project.join("mustard.json").exists(), "version stamp written at project root");
    assert!(!claude.join("mustard.json").exists(), "no .claude/mustard.json");

    // Idempotent re-run — the job the retired `mustard update` used to do.
    // Re-seeding an already-initialized project must succeed non-interactively.
    init_with_templates(
        &project,
        &templates,
        &InitOptions { force: true, yes: true, ..InitOptions::default() },
    )
    .expect("re-running init should re-seed without a terminal");

    assert!(
        claude.join("CLAUDE.md").exists(),
        "core seed still present after the idempotent re-seed",
    );
}
