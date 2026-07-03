//! `mustard update` — refresh Mustard core files, preserving user edits.
//!
//! `update` MIRRORS the entire bundled `templates/` payload into an existing
//! `.claude/`, exactly the tree `init` lays down — so ANY new template file
//! reaches an already-installed project automatically. It replaces the former
//! hardcoded allowlist of folders (`commands/mustard`, `skills`, `scripts`,
//! `refs`) plus a couple of named files, which silently went stale whenever a
//! new template path shipped: `agents/`, `pipeline-config.md`, `context/` and
//! `grammars-suggestions.json` were each missed and hand-backfilled. The mirror
//! is driven by ITERATING the template's top-level entries — nothing is
//! enumerated by name beyond the three routing sets below.
//!
//! The flow:
//!
//! 1. require an existing `.claude/` (error out otherwise);
//! 2. confirm interactively (unless `--force`);
//! 3. take a timestamped backup — **unconditionally**, `--force` only skips
//!    the prompt, never the safety net;
//! 4. mirror each top-level `templates/` entry into `.claude/`, routed by:
//!    - **PRESERVE** ([`PRESERVE`]) — never deleted, never overwritten: the
//!      user/runtime files `CLAUDE.md`, `mustard.json`, `grain.model.json` and
//!      the dirs `spec/`, `memory/`, `docs/`. `CLAUDE.md` ships in the payload
//!      but its refresh is a separate, deliberate concern (the scan personalizes
//!      its `## Guards`), so update leaves it alone; the rest are not in the
//!      payload at all — listed so a future template could never clobber them;
//!    - **EXCLUDE** ([`EXCLUDE`]) — never deployed: `.github` (installed at the
//!      project root by `init`) and `.claude` (the `.claude/.claude/` nesting
//!      guard). Matches `init`'s `copy_dir` exclude exactly, so init and update
//!      produce the SAME `.claude/`;
//!    - **MERGE** ([`MERGE_DIRS`]) — copied OVER without deleting first:
//!      `agents/`, a flat SHARED namespace where a user may drop their own agent
//!      alongside Mustard's `mustard-review` / `mustard-guards` definitions;
//!    - **else** — a Mustard-owned FOLDER is deleted then recopied (pruning
//!      files dropped from the template + refreshing the rest); a Mustard-owned
//!      FILE (e.g. `settings.json`, `pipeline-config.md`, `grammars-suggestions.json`)
//!      is overwrite-copied;
//! 5. re-assert the absolute hook commands in the copied `settings.json`, then
//!    re-run the RTK / ripgrep / global-permissions guarantees;
//! 6. re-stamp the `version` field in the project-root `mustard.json`.
//!
//! **What is preserved.** The PRESERVE set above, plus every user-added file
//! inside a MERGE dir (a user's own agent survives the `agents/` refresh). The
//! project-root `mustard.json` is loaded and rewritten with only `version`
//! bumped — git flow, commands, language/tone, `runtime` and any user keys
//! survive. The unconditional backup preserves any local edit to a
//! Mustard-owned file.
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

/// Top-level `.claude/` entries `update` must NEVER delete or overwrite — the
/// user- and runtime-owned files. `mustard.json` and `grain.model.json` are not
/// in the template payload at all (they are listed so a future template shipping
/// one could still never clobber the deployed copy); `CLAUDE.md` IS in the
/// payload but its refresh is a separate, deliberate concern — the scan
/// personalizes its `## Guards`, so update leaves the user's orchestrator file
/// untouched. `spec/`, `memory/`, `docs/` are the pipeline state and user notes.
const PRESERVE: &[&str] =
    &["CLAUDE.md", "mustard.json", "grain.model.json", "spec", "memory", "docs"];

/// Top-level template entries never deployed into a client `.claude/`. Matches
/// `init`'s `copy_dir` exclude EXACTLY so init and update produce the same tree:
/// `.github` is installed at the project ROOT (by `init`), and `.claude` guards
/// against the `.claude/.claude/` nesting bug (I1).
const EXCLUDE: &[&str] = &[".github", ".claude"];

/// Mustard-owned folders copied OVER the target without deleting it first, so a
/// user's own file dropped alongside Mustard's survives. `agents/` is a flat,
/// SHARED namespace: the copy overwrites Mustard's `mustard-review` /
/// `mustard-guards` definitions and leaves any user agent in place. (Backfills a
/// project `init`'d before the agents shipped — the gap that left
/// `mustard-review` unregistered, forcing dispatch to fall back to
/// `general-purpose` and lose the read-only tool restriction.)
const MERGE_DIRS: &[&str] = &["agents"];

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

    println!(
        "  Will mirror the entire templates payload into .claude/ (folders pruned + refreshed, files overwritten)"
    );
    println!(
        "  Will preserve: CLAUDE.md  mustard.json  grain.model.json  spec/  memory/  docs/  (+ your own agents/)"
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

    // Mirror the whole templates payload into `.claude/`, driven by ITERATING
    // the template's top-level entries — no hardcoded allowlist, so a new
    // template path can never go stale. Each entry is routed by the
    // PRESERVE / EXCLUDE / MERGE_DIRS sets; everything else is a Mustard-owned
    // FOLDER (deleted then recopied, pruning files dropped from the template) or
    // FILE (overwrite-copied). Names are sorted for deterministic output.
    let mut names: Vec<String> = std::fs::read_dir(templates_dir)
        .with_context(|| format!("reading templates {}", templates_dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();

    let mut total = 0usize;
    for name in &names {
        let key = name.as_str();
        if EXCLUDE.contains(&key) || PRESERVE.contains(&key) {
            continue;
        }
        let src = templates_dir.join(name);
        let dest = claude_path.join(name);
        if MERGE_DIRS.contains(&key) {
            // Copy OVER without deleting first — user-added entries survive.
            total += copy_dir(&src, &dest, true, &[])?;
        } else if src.is_dir() {
            // Mustard-owned folder: delete then recopy so files removed from the
            // template are pruned from the deployed tree.
            if dest.exists() {
                mfs::remove_dir_all(&dest)
                    .with_context(|| format!("removing {}", dest.display()))?;
            }
            total += copy_dir(&src, &dest, true, &[])?;
        } else {
            // Mustard-owned file at the template root: overwrite-copy.
            let bytes = mfs::read(&src).with_context(|| format!("reading {}", src.display()))?;
            mfs::write_atomic(&dest, &bytes)
                .with_context(|| format!("copying {}", src.display()))?;
            total += 1;
        }
    }
    println!("  Mirrored {total} files from templates");

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
        fs::create_dir_all(templates.join("refs")).unwrap();
        fs::create_dir_all(templates.join("agents")).unwrap();
        // A nested folder OUTSIDE the old CORE_FOLDERS allowlist — the exact
        // shape (`context/`) that silently went stale on every update.
        fs::create_dir_all(templates.join("context/qa")).unwrap();
        fs::write(templates.join("commands/mustard/feature.md"), "v2").unwrap();
        fs::write(templates.join("skills/guard.js"), "v2").unwrap();
        fs::write(templates.join("agents/mustard-review.md"), "review-v2").unwrap();
        fs::write(templates.join("context/qa/qa.core.md"), "QA-CORE-V2").unwrap();
        fs::write(templates.join("settings.json"), r#"{"v":2}"#).unwrap();
        // Root-level reference doc — backfilled/refreshed by update.
        fs::write(templates.join("pipeline-config.md"), "PIPELINE-CONFIG-V2").unwrap();
        // A top-level file OUTSIDE the old allowlist — also went stale before.
        fs::write(templates.join("grammars-suggestions.json"), "GRAMMARS-V2").unwrap();
        templates
    }

    /// Build a `.claude/` tree as `init` would have left it, plus user files.
    fn existing_claude(project: &Path) {
        let claude = project.join(".claude");
        fs::create_dir_all(claude.join("commands/mustard")).unwrap();
        fs::create_dir_all(claude.join("skills")).unwrap();
        fs::create_dir_all(claude.join("docs")).unwrap();
        fs::create_dir_all(claude.join("spec")).unwrap();
        fs::create_dir_all(claude.join("memory")).unwrap();
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
        fs::write(claude.join("memory/note.md"), "USER MEMORY").unwrap();
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
        assert_eq!(
            fs::read_to_string(claude.join("memory/note.md")).unwrap(),
            "USER MEMORY"
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

    /// Regression for the hardcoded-allowlist bug: a template path that was NEVER
    /// in the old `CORE_FOLDERS` list (`context/`) — and a top-level file outside
    /// it (`grammars-suggestions.json`) — must now be mirrored into `.claude/` by
    /// update. Before the generic mirror these silently went stale on every
    /// update and had to be hand-backfilled.
    #[test]
    fn update_deploys_template_files_outside_the_old_allowlist() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        existing_claude(&project);

        update_with_templates(&project, &templates, &UpdateOptions { force: true }).unwrap();

        let claude = project.join(".claude");
        // A nested folder never in the old allowlist is mirrored, content fresh.
        assert_eq!(
            fs::read_to_string(claude.join("context/qa/qa.core.md")).unwrap(),
            "QA-CORE-V2",
            "context/ must be deployed by the generic mirror"
        );
        // A top-level template file outside the allowlist is deployed too.
        assert_eq!(
            fs::read_to_string(claude.join("grammars-suggestions.json")).unwrap(),
            "GRAMMARS-V2",
            "a top-level file outside the old allowlist must be deployed"
        );
    }

    /// A file removed from the template payload is pruned from a Mustard-owned
    /// folder on update (the folder is deleted then recopied), so stale files
    /// never accumulate in a deployed project.
    #[test]
    fn update_prunes_files_removed_from_templates() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        existing_claude(&project);
        // A stale Mustard-owned file with no counterpart in the template payload
        // (`templates/refs/` is empty), sitting in an owned (non-MERGE) folder.
        let claude = project.join(".claude");
        fs::create_dir_all(claude.join("refs")).unwrap();
        fs::write(claude.join("refs/removed.md"), "OLD REF").unwrap();

        update_with_templates(&project, &templates, &UpdateOptions { force: true }).unwrap();

        assert!(
            !claude.join("refs/removed.md").exists(),
            "a file dropped from templates must be pruned from a Mustard-owned folder"
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
