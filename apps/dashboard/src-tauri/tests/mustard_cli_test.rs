//! Smoke test for the B5 Wave 3 Tauri integration.
//!
//! The `mustard_install` / `mustard_update` Tauri commands are private wrappers
//! around `mustard_cli::init` / `mustard_cli::update`. This test exercises that
//! same library API from the dashboard crate's test context — proving the
//! `mustard-cli` path dependency is linked and the non-interactive install
//! path runs to completion without a terminal.

use std::fs;

use mustard_cli::commands::init::{InitOptions, init_with_templates};
use mustard_cli::commands::update::{UpdateOptions, update_with_templates};

/// Build a minimal fake `templates/` payload `init`/`update` can copy.
fn fake_templates(root: &std::path::Path) -> std::path::PathBuf {
    let templates = root.join("templates");
    fs::create_dir_all(templates.join("commands/mustard")).unwrap();
    fs::create_dir_all(templates.join("hooks")).unwrap();
    fs::create_dir_all(templates.join("skills")).unwrap();
    fs::create_dir_all(templates.join("scripts")).unwrap();
    fs::create_dir_all(templates.join("refs")).unwrap();
    fs::write(templates.join("CLAUDE.md"), "# rules").unwrap();
    fs::write(templates.join("settings.json"), "{}").unwrap();
    fs::write(templates.join("commands/mustard/feature.md"), "feature").unwrap();
    templates
}

#[test]
fn install_then_update_runs_non_interactively() {
    let work = tempfile::tempdir().unwrap();
    let templates = fake_templates(work.path());
    let project = work.path().join("project");
    fs::create_dir_all(&project).unwrap();

    // What `mustard_install` does under the hood: non-interactive init.
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

    // What `mustard_update` does under the hood: forced, non-interactive update.
    update_with_templates(&project, &templates, &UpdateOptions { force: true })
        .expect("update should run without a terminal");

    assert!(
        claude.join("commands/mustard/feature.md").exists(),
        "core files refreshed by update",
    );
}
