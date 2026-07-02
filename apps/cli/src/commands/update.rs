//! `mustard update` — refresh Mustard core files, preserving user edits.
//!
//! Ported from `commands/update.ts`. The flow:
//!
//! 1. require an existing `.claude/` (error out otherwise);
//! 2. confirm interactively (unless `--force`);
//! 3. take a timestamped backup — **unconditionally**, `--force` only skips
//!    the prompt, never the safety net;
//! 4. delete the core, Mustard-owned folders (`commands/mustard`, `skills`,
//!    `scripts`, `refs`);
//! 5. re-copy those folders plus `settings.json` and the root reference
//!    `pipeline-config.md` from `templates/`, and overwrite the Mustard agent
//!    definitions in the shared `agents/` folder (copied, never deleted, so a
//!    user's own agents survive);
//! 6. re-run the RTK and global-permissions guarantees;
//! 7. re-stamp the `version` field in the project-root `mustard.json`.
//!
//! **What is preserved.** Only the Mustard-owned folders are deleted, so
//! everything else under `.claude/` survives untouched: `CLAUDE.md`,
//! `grain.model.json`, `docs/`, `spec/`, `memory/`, and any user-authored
//! command outside `commands/mustard/`. The project-root `mustard.json` is
//! loaded and rewritten with only `version` bumped — git flow, commands,
//! language/tone, `runtime` and any user keys are preserved.
//!
//! **Why `pipeline-config.md` is re-copied, not preserved.** It is a static,
//! Mustard-owned orchestrator reference with no per-project customization
//! (unlike `CLAUDE.md`, whose `## Guards` the scan personalizes). It lives at
//! the template ROOT, not under a core folder, so the folder re-copy loop never
//! touched it: a project `init`'d before the file shipped and only ever
//! `update`d never received it — every agent following a `Read pipeline-config.md`
//! instruction then hit a missing file — and edits to the reference never
//! propagated to deployed projects. This is the same backfill gap the `agents/`
//! copy closed. The unconditional backup preserves any local edit.
//!
//! **The `version` re-stamp** lets the dashboard (B6) see the freshly installed
//! version; the rest of the config is left as `init`/the user set it.

use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result, bail};
use dialoguer::Confirm;
use dialoguer::theme::ColorfulTheme;
use mustard_core::io::fs as mfs;
use mustard_core::ProjectConfig;

use crate::commands::init::{
    ensure_global_permissions, ensure_ripgrep, ensure_rtk, install_mcp_json,
    resolve_templates_dir, rewrite_hooks_to_absolute,
};
use crate::fs_ops::copy_dir;

/// Flags accepted by `mustard update`.
#[derive(Debug, Default, Clone)]
pub struct UpdateOptions {
    /// Skip the confirmation prompt (never skips the backup).
    pub force: bool,
}

/// The Mustard-owned folders `update` deletes and re-copies. Everything else
/// under `.claude/` is left in place — that is how user customisations and
/// pipeline state survive an update.
const CORE_FOLDERS: &[&str] = &["commands/mustard", "skills", "scripts", "refs"];

/// Run `mustard update` against `project_path`.
///
/// The library entry point the Tauri backend (Wave 3) calls. Locates the
/// bundled `templates/` directory; callers that already know it use
/// [`update_with_templates`].
pub fn update(project_path: &Path, options: &UpdateOptions) -> Result<()> {
    let templates_dir = resolve_templates_dir()?;
    update_with_templates(project_path, &templates_dir, options)
}

/// [`update`] with the `templates/` directory supplied explicitly.
///
/// Split out for the same reason as `init_with_templates`: it keeps template
/// resolution out of the update logic so tests can drive a fixture tree.
pub fn update_with_templates(
    project_path: &Path,
    templates_dir: &Path,
    options: &UpdateOptions,
) -> Result<()> {
    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("resolving project path {}", project_path.display()))?;
    let claude_path = project_path.join(".claude");

    println!("\nMustard - Update\n");

    if !claude_path.exists() {
        bail!("no .claude/ directory found - run `mustard init` first");
    }

    println!("  Will recreate: commands/mustard/  skills/  scripts/  refs/  settings.json  pipeline-config.md");
    println!(
        "  Will preserve: CLAUDE.md  grain.model.json  mustard.json  docs/  spec/  memory/"
    );

    // Confirm — interactive only. `--force` skips the prompt; a non-TTY stdin
    // (CI, tests, Tauri) proceeds without blocking, mirroring `init`.
    if !options.force && std::io::stdin().is_terminal() {
        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Backup and update?")
            .default(true)
            .interact()
            .context("reading the update confirmation")?;
        if !proceed {
            println!("\n  Cancelled.\n");
            return Ok(());
        }
    }

    // Backup runs unconditionally — the safety net is never skipped.
    let backup = backup_claude_dir(&claude_path)?;
    println!("  Backup: {}", backup.display());

    // Delete the Mustard-owned core folders.
    for folder in CORE_FOLDERS {
        let target = claude_path.join(folder);
        if target.exists() {
            mfs::remove_dir_all(&target)
                .with_context(|| format!("removing {}", target.display()))?;
        }
    }
    println!("  Cleaned core folders");

    // Re-copy the core folders + settings.json from templates/.
    let mut total = 0usize;
    total += copy_core_folder(templates_dir, &claude_path, "commands/mustard")?;
    total += copy_core_folder(templates_dir, &claude_path, "skills")?;
    total += copy_core_folder(templates_dir, &claude_path, "scripts")?;
    total += copy_core_folder(templates_dir, &claude_path, "refs")?;
    // `agents/` is a flat, SHARED namespace (a user may drop their own agent
    // there), so — unlike the CORE_FOLDERS above — it is NOT deleted first: the
    // copy overwrites Mustard's `mustard-review` / `mustard-guards` definitions
    // and leaves any user agent untouched. Backfills a project that was `init`'d
    // before the agents shipped and has only ever been `update`d — the gap that
    // left `mustard-review` unregistered, forcing dispatch to fall back to
    // `general-purpose` and lose the read-only tool restriction.
    total += copy_core_folder(templates_dir, &claude_path, "agents")?;
    total += copy_core_file(templates_dir, &claude_path, "settings.json")?;
    // Backfill / refresh the root-level orchestrator reference (see module doc):
    // absent in projects init'd before it shipped, stale in the rest.
    total += copy_core_file(templates_dir, &claude_path, "pipeline-config.md")?;
    println!("  Updated {total} files");

    // The settings.json we just re-copied carries the template's bare
    // `rtk mustard-rt on <Event>` hooks again — re-assert the absolute,
    // PATH-independent commands (same rewrite `init` applies).
    rewrite_hooks_to_absolute(&claude_path);

    ensure_global_permissions().unwrap_or_else(|err| {
        eprintln!("[mustard] warning: could not update global permissions: {err}");
    });
    ensure_rtk();
    ensure_ripgrep();

    // Re-stamp the version into the project-root mustard.json: load the existing
    // config and rewrite only `version`, preserving git flow, commands,
    // language/tone, runtime and any user keys.
    let mut config = ProjectConfig::load(&project_path);
    config.version = Some(crate::VERSION.to_string());
    config.write(&project_path)?;

    // Ensure the project-root .mcp.json carries the mustard-memory server (a
    // project predating the settings.json → .mcp.json split picks it up here).
    install_mcp_json(&project_path)?;

    println!("\nUpdate complete!\n");
    Ok(())
}

/// Copy a single core folder from `templates/<rel>` into `.claude/<rel>`,
/// overwriting (the folder was just deleted, so this is a clean re-copy).
/// A folder absent from the payload (e.g. `refs/` in an older template set)
/// is silently skipped, matching the JS `existsSync` guard.
fn copy_core_folder(templates_dir: &Path, claude_path: &Path, rel: &str) -> Result<usize> {
    let src = templates_dir.join(rel);
    if !src.is_dir() {
        return Ok(0);
    }
    copy_dir(&src, &claude_path.join(rel), true, &[])
}

/// Re-copy a single Mustard-owned file living at the template ROOT (not under a
/// core folder) into `.claude/<rel>`. Returns `1` when copied, `0` when the
/// payload lacks it (matching the JS `existsSync` guard). Used for
/// `settings.json` and the `pipeline-config.md` reference — both backfilled when
/// absent and refreshed when present, since neither is per-project customized.
fn copy_core_file(templates_dir: &Path, claude_path: &Path, rel: &str) -> Result<usize> {
    let src = templates_dir.join(rel);
    if !src.is_file() {
        return Ok(0);
    }
    let bytes = mfs::read(&src).with_context(|| format!("reading {}", src.display()))?;
    mfs::write_atomic(claude_path.join(rel), &bytes)
        .with_context(|| format!("copying {}", src.display()))?;
    Ok(1)
}

/// Copy `.claude/` to a timestamped `.backup.` sibling and return its path.
fn backup_claude_dir(claude_path: &Path) -> Result<std::path::PathBuf> {
    let stamp = mustard_core::time::filename_safe_now();
    let name = claude_path
        .file_name()
        .map_or_else(|| ".claude".to_string(), |n| n.to_string_lossy().into_owned());
    let backup = claude_path.with_file_name(format!("{name}.backup.{stamp}"));
    copy_dir(claude_path, &backup, true, &[])?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a minimal fake `templates/` tree with the core folders `update`
    /// re-copies.
    fn fake_templates(root: &Path) -> std::path::PathBuf {
        let templates = root.join("templates");
        fs::create_dir_all(templates.join("commands/mustard")).unwrap();
        fs::create_dir_all(templates.join("skills")).unwrap();
        fs::create_dir_all(templates.join("scripts")).unwrap();
        fs::create_dir_all(templates.join("refs")).unwrap();
        fs::create_dir_all(templates.join("agents")).unwrap();
        fs::write(templates.join("commands/mustard/feature.md"), "v2").unwrap();
        fs::write(templates.join("skills/guard.js"), "v2").unwrap();
        fs::write(templates.join("agents/mustard-review.md"), "review-v2").unwrap();
        fs::write(templates.join("settings.json"), r#"{"v":2}"#).unwrap();
        // Root-level reference doc — backfilled/refreshed by update.
        fs::write(templates.join("pipeline-config.md"), "PIPELINE-CONFIG-V2").unwrap();
        templates
    }

    /// Build a `.claude/` tree as `init` would have left it, plus user files.
    fn existing_claude(project: &Path) {
        let claude = project.join(".claude");
        fs::create_dir_all(claude.join("commands/mustard")).unwrap();
        fs::create_dir_all(claude.join("skills")).unwrap();
        fs::create_dir_all(claude.join("docs")).unwrap();
        fs::create_dir_all(claude.join("spec")).unwrap();
        // Stale Mustard-owned files (should be replaced).
        fs::write(claude.join("commands/mustard/feature.md"), "v1-stale").unwrap();
        fs::write(claude.join("skills/guard.js"), "v1-stale").unwrap();
        // A project that predates the shipped agents: only a user's own agent
        // is present, and it must survive the backfill.
        fs::create_dir_all(claude.join("agents")).unwrap();
        fs::write(claude.join("agents/my-custom.md"), "USER AGENT").unwrap();
        // User files (must survive untouched).
        fs::write(claude.join("CLAUDE.md"), "USER RULES").unwrap();
        fs::write(claude.join("docs/notes.md"), "USER NOTES").unwrap();
        fs::write(claude.join("spec/feat.md"), "USER SPEC").unwrap();
        fs::write(
            project.join("mustard.json"),
            r#"{"version":"0.0.1","runtime":{"kind":"native"}}"#,
        )
        .unwrap();
    }

    #[test]
    fn update_errors_without_claude_dir() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("empty");
        fs::create_dir_all(&project).unwrap();

        let err =
            update_with_templates(&project, &templates, &UpdateOptions { force: true })
                .unwrap_err();
        assert!(err.to_string().contains("no .claude/ directory"));
    }

    #[test]
    fn update_preserves_user_files_and_refreshes_core() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        existing_claude(&project);

        update_with_templates(&project, &templates, &UpdateOptions { force: true }).unwrap();

        let claude = project.join(".claude");
        // User files survive untouched.
        assert_eq!(fs::read_to_string(claude.join("CLAUDE.md")).unwrap(), "USER RULES");
        assert_eq!(fs::read_to_string(claude.join("docs/notes.md")).unwrap(), "USER NOTES");
        assert_eq!(
            fs::read_to_string(claude.join("spec/feat.md")).unwrap(),
            "USER SPEC"
        );
        // Mustard-owned files are refreshed from the payload.
        assert_eq!(
            fs::read_to_string(claude.join("commands/mustard/feature.md")).unwrap(),
            "v2"
        );
        assert_eq!(fs::read_to_string(claude.join("skills/guard.js")).unwrap(), "v2");
        // The Mustard agent definitions are backfilled (a project that never had
        // them now gets `mustard-review`, so the subagent type registers)...
        assert_eq!(
            fs::read_to_string(claude.join("agents/mustard-review.md")).unwrap(),
            "review-v2"
        );
        // ...while a user's own agent in the shared `agents/` folder survives.
        assert_eq!(
            fs::read_to_string(claude.join("agents/my-custom.md")).unwrap(),
            "USER AGENT"
        );
        // The root reference `pipeline-config.md` is backfilled: `existing_claude`
        // never created it (the sialia gap — init'd before it shipped, only ever
        // `update`d), yet after update the deployed copy exists and matches the
        // template. Every `Read pipeline-config.md` instruction now resolves.
        assert_eq!(
            fs::read_to_string(claude.join("pipeline-config.md")).unwrap(),
            "PIPELINE-CONFIG-V2",
            "pipeline-config.md must be backfilled from the template"
        );
    }

    /// A project that already has a *stale* `pipeline-config.md` gets it refreshed
    /// to the template version — the reference doc is Mustard-owned, not a
    /// user-editable file, so edits to it propagate to deployed projects.
    #[test]
    fn update_refreshes_stale_pipeline_config() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        existing_claude(&project);
        // Simulate a deployed project stuck on an older reference doc.
        fs::write(
            project.join(".claude").join("pipeline-config.md"),
            "PIPELINE-CONFIG-V1-STALE",
        )
        .unwrap();

        update_with_templates(&project, &templates, &UpdateOptions { force: true }).unwrap();

        assert_eq!(
            fs::read_to_string(project.join(".claude").join("pipeline-config.md")).unwrap(),
            "PIPELINE-CONFIG-V2",
            "stale pipeline-config.md must be refreshed to the template version"
        );
    }

    #[test]
    fn update_restamps_version_and_keeps_runtime() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        existing_claude(&project);

        update_with_templates(&project, &templates, &UpdateOptions { force: true }).unwrap();

        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        // version is re-stamped to this build.
        assert_eq!(cfg.get("version").and_then(|v| v.as_str()), Some(crate::VERSION));
        // runtime is preserved verbatim — update does not own it.
        assert!(cfg.get("runtime").is_some(), "runtime block preserved");
    }

    #[test]
    fn update_writes_a_backup() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        existing_claude(&project);

        update_with_templates(&project, &templates, &UpdateOptions { force: true }).unwrap();

        // A `.claude.backup.<slug>` sibling exists and carries the pre-update
        // user content.
        let backup = fs::read_dir(&project)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n.starts_with(".claude.backup."));
        assert!(backup.is_some(), "a backup directory was created");
    }
}
