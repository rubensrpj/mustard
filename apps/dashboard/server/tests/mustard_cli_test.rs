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
/// Mustard 2.0 init reads `settings.json`, `.gitignore`, and the `mustard/`
/// injectable instruction files (the orchestrator is injected by the session
/// hooks now — no `CLAUDE.md` is planted); the content payload
/// (commands/skills/agents/refs) ships in the plugin.
fn fake_templates(root: &std::path::Path) -> std::path::PathBuf {
    let templates = root.join("templates");
    fs::create_dir_all(templates.join("mustard")).expect("mkdir");
    fs::write(templates.join("mustard/orchestrator.md"), "# Orchestrator Rules
").expect("write");
    fs::write(templates.join("settings.json"), r#"{"env":{"MUSTARD_TEST":"1"}}"#).expect("write");
    fs::write(templates.join(".gitignore"), "spec/*/.events/
").expect("write");
    templates
}

#[test]
fn init_runs_non_interactively_and_is_idempotent() {
    let work = tempfile::tempdir().expect("tempdir");
    let templates = fake_templates(work.path());
    let project = work.path().join("project");
    fs::create_dir_all(&project).expect("mkdir");

    // Non-interactive init: seed a fresh project the way the dashboard would.
    init_with_templates(
        &project,
        &templates,
        &InitOptions { yes: true, ..InitOptions::default() },
    )
    .expect("init should run without a terminal");

    let claude = project.join(".claude");
    assert!(
        claude.join("mustard").join("orchestrator.md").exists(),
        ".claude/mustard/ injectables scaffolded"
    );
    assert!(
        !claude.join("CLAUDE.md").exists(),
        "no .claude/CLAUDE.md planted — the orchestrator is injected now"
    );
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
        claude.join("mustard").join("orchestrator.md").exists(),
        "core seed still present after the idempotent re-seed",
    );

    // …and neither run may have registered this project with the dashboard.
    //
    // This is the ONE test in the four that drives `init` in-process, so it is
    // the one that cannot redirect `$HOME`: `std::env::set_var` is `unsafe` in
    // edition 2024 and process-global, which would corrupt whatever else the
    // test binary is running in parallel. What it CAN do is look where the leak
    // would land. `register_with_dashboard` is `pub(crate)` and lives in
    // `cli::dispatch` precisely so a library caller never takes that act; if it
    // ever moves back down, this call would append a row for a temporary
    // directory to the developer's real `~/.claude/dashboard-projects.json`,
    // and the tempdir's random path segment makes that row unmistakable.
    //
    // The matching POSITIVE assertion — that a real `mustard init` DOES write
    // the row, into a home the test owns — belongs to the tests that drive the
    // binary (`apps/cli/tests/private_init.rs`, `apps/cli/tests/rtk_gate.rs`),
    // because dispatch is the only layer allowed to take the act at all.
    let canonical = project.canonicalize().unwrap_or_else(|_| project.clone());
    let leaked: Vec<String> = mustard_core::dashboard_registry::read()
        .into_iter()
        .map(|e| e.path)
        .filter(|p| std::path::Path::new(p) == canonical)
        .collect();
    assert!(
        leaked.is_empty(),
        "a library `init` registered a temporary project on the real machine: {leaked:?} — \
         registering is an environment act and belongs to cli::dispatch",
    );
}
