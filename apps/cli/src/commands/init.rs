//! `mustard init` — thin bootstrap for a Claude Code project (Mustard 2.0).
//!
//! The heavy `.claude/` payload — commands, skills, agents, refs, hooks — now
//! ships in the **`mustard` plugin**, distributed through a private git
//! marketplace. `init` no longer copies that payload; it lays down the small
//! set of files a plugin cannot ship, then enables the plugin. The flow:
//!
//! 1. probe RTK — a hard gate (the harness prefixes every Bash call with `rtk`);
//!    then guard the location: init refuses a directory that sits inside a git
//!    repository without being its root (the workspace resolver anchors on git
//!    roots — see [`guard_init_location`]);
//! 2. handle an already-present `.claude/` (force-overwrite, merge, or
//!    backup-then-overwrite — interactively prompted when no flag decides it);
//! 3. seed the harness into `.claude/` — delegated to the core seeding engine
//!    (`mustard_core::platform::project_seed`, fed by the compiled-in
//!    `platform::seeds` constants; `mustard-rt run upsert` consumes the same
//!    engine):
//!    - `settings.json` — the reduced SEED (env / permissions / statusLine /
//!      plansDirectory …); plugin enablement is NOT planted (user-scope
//!      choice) and the broken pair an older build wrote is retired
//!      (`mustard_core::retire_planted_plugin_enablement`);
//!    - `mustard/*.md` — the injectable instruction files (orchestrator rules,
//!      response style); the session hooks splice them into the agent's window
//!      per `mustard.json#inject` — **no `CLAUDE.md` is planted anymore** (a
//!      planted orchestrator drowned in large root files; injection always
//!      lands);
//!    - `.gitignore` — covers the ephemeral harness state;
//!    - migration: a legacy Mustard-planted `.claude/CLAUDE.md` (identified by
//!      its `# Orchestrator Rules` marker) is deleted, and the Mustard import
//!      + breadcrumb lines are removed from the project-root `CLAUDE.md` —
//!      the file goes back to being fully the user's;
//! 4. copy `templates/.github/` → project-root `.github/` when a GitHub remote
//!    is detected (project-level scaffolding, not part of the plugin);
//! 5. ensure global Claude Code permissions in `~/.claude/settings.json` (opt-in);
//! 6. install RTK + ripgrep (token economy) if missing — fail-open;
//! 7. write the single project-root `mustard.json`: git-flow + agnostically
//!    detected build/test/lint/type-check commands + spec language + tone +
//!    the `runtime`/`version` stamp + the default `inject` declarations
//!    (seeded only when the user has none — a curated list is preserved).
//!
//! There is **one** config file, at the **project root** (the workspace anchor
//! `workspace_root` keys on) — never `.claude/mustard.json`. The `version`
//! stamp lets the dashboard read the installed Mustard version; because `init`
//! is idempotent, **re-running it re-stamps that version** — the job the retired
//! `mustard update` used to do.
//!
//! `.mcp.json` is deliberately **not** written: the `mustard` plugin ships its
//! own `.mcp.json`, so a project-level copy is redundant once the plugin is
//! enabled.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use mustard_core::io::fs as mfs;
use serde_json::json;

use crate::commands::git_flow;
use crate::fs_ops::copy_dir;
use mustard_core::{ProjectConfig, Runtime, SeedOutcome};

/// Flags accepted by `mustard init`.
#[derive(Debug, Default, Clone)]
pub struct InitOptions {
    /// Overwrite an existing `.claude/` without a backup.
    pub force: bool,
    /// Accept defaults without prompting.
    pub yes: bool,
    /// Print intended actions without touching disk.
    pub dry_run: bool,
}

/// What to do with an already-present `.claude/` directory.
enum ExistingAction {
    /// Overwrite (the `--force` path, or the interactive "backup" choice once
    /// the backup has been taken).
    Overwrite,
    /// Keep user edits: seed only the files that are absent, but still merge the
    /// plugin-enable keys into `settings.json`.
    Merge,
    /// Abort without writing.
    Cancel,
}

/// Run `mustard init` against `project_path`.
///
/// This is the library entry point the Tauri backend calls. The binary passes
/// the process working directory; a caller may pass any folder. The bundled
/// `templates/` directory is located via [`resolve_templates_dir`]; callers
/// that already know its location use [`init_with_templates`].
pub fn init(project_path: &Path, options: &InitOptions) -> Result<()> {
    let templates_dir = resolve_templates_dir()?;
    init_with_templates(project_path, &templates_dir, options)
}

/// [`init`] with the `templates/` directory supplied explicitly.
///
/// Splitting this out keeps template resolution (an environment concern) out
/// of the install logic, so tests can drive a fixture tree and the Tauri
/// backend can point at its own bundled payload — no process-global env var.
pub fn init_with_templates(
    project_path: &Path,
    templates_dir: &Path,
    options: &InitOptions,
) -> Result<()> {
    // RTK is a mandatory dependency of Mustard — the harness's Golden Rule
    // prefixes every Bash invocation with `rtk`. Probe before touching disk: if
    // `rtk` is missing the install would produce a `.claude/` that cannot run,
    // so we exit hard with install instructions instead. Skipped in dry-run
    // mode (no disk writes either).
    if !options.dry_run {
        probe_rtk();
    }

    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("resolving project path {}", project_path.display()))?;
    let claude_path = project_path.join(".claude");

    // Location guard — runs in dry-run too: the honest "intended action" for a
    // subdirectory of a git repository is a refusal, not a simulated install.
    guard_init_location(&project_path)?;

    println!("\nMustard\n");

    let runtime = Runtime::detect();
    println!("[mustard] runtime: {} {}/{}", runtime.kind, runtime.os, runtime.arch);

    if options.dry_run {
        println!("  (dry-run) would seed the harness into {}:", claude_path.display());
        println!("    settings.json  — reduced seed (plugin enablement stays at user scope; a planted placeholder pair is retired)");
        println!("    mustard/*.md   — injectable instruction files (orchestrator, response style); hooks inject them per mustard.json#inject");
        println!("    .gitignore     — ephemeral harness state");
        println!("  (dry-run) would migrate a legacy Mustard-planted .claude/CLAUDE.md away (and remove the Mustard import/breadcrumb lines from the root CLAUDE.md)");
        println!(
            "  (dry-run) would write git-flow + commands + runtime/version + inject declarations to {}",
            project_path.join("mustard.json").display()
        );
        println!("  (dry-run) content payload (commands/skills/agents/refs) + .mcp.json now ship in the `mustard` plugin — not written");
        return Ok(());
    }

    // Decide how to treat an existing `.claude/`. A fresh project is a plain
    // overwrite of an empty tree.
    let overwrite = if claude_path.exists() {
        match decide_existing_action(&claude_path, options)? {
            ExistingAction::Cancel => {
                println!("\n  Cancelled.\n");
                return Ok(());
            }
            ExistingAction::Merge => false,
            ExistingAction::Overwrite => true,
        }
    } else {
        true
    };

    mfs::create_dir_all(&claude_path)
        .with_context(|| format!("creating {}", claude_path.display()))?;

    // Migration (idempotent, every run over an existing project): remove the
    // footprint the pre-injectable Mustard left in the project's instruction
    // files — the planted `.claude/CLAUDE.md` orchestrator and the import +
    // breadcrumb lines in the root `CLAUDE.md`. Runs BEFORE seeding so the
    // legacy layout is gone when the new one lands. Fail-open in the core
    // engine: any IO error degrades to "not migrated", never aborts the init.
    report_migration(&mustard_core::migrate_orchestrator_footprint(&project_path, &claude_path));

    // (a)+(e) settings.json: the reduced seed + the plugin-enablement retire —
    // the core engine owns the content (compiled-in seed) and the merge rules.
    let outcome = mustard_core::seed_settings(&claude_path, overwrite)
        .context("seeding .claude/settings.json")?;
    report_seed(".claude/settings.json", outcome);
    // (b) injectable instruction files — the orchestrator is INJECTED by the
    // session hooks now (per `mustard.json#inject`), never planted as
    // `.claude/CLAUDE.md`.
    for (name, outcome) in mustard_core::seed_injectable_files(&claude_path, overwrite)
        .context("seeding .claude/mustard/ injectables")?
    {
        report_seed(&format!(".claude/mustard/{name}"), outcome);
    }
    // (c) ephemeral-state .gitignore.
    let outcome = mustard_core::seed_gitignore(&claude_path, overwrite)
        .context("seeding .claude/.gitignore")?;
    report_seed(".claude/.gitignore", outcome);

    // (d) `.mcp.json` is intentionally NOT written — the `mustard` plugin ships
    // its own, so a project-level copy is redundant once the plugin is enabled.

    // Project-root `.github/` scaffolding (PR template) — not part of the
    // plugin, seeded only when the project has a GitHub remote. Never overwrites.
    let gh = install_github_templates(templates_dir, &project_path)?;
    if gh > 0 {
        println!("  wrote {gh} GitHub template(s) at .github/");
    }

    ensure_global_permissions().unwrap_or_else(|err| {
        eprintln!("[mustard] warning: could not update global permissions: {err}");
    });
    ensure_rtk();
    ensure_ripgrep();

    // Write the single project-root mustard.json: git-flow + detected commands
    // + language/tone + runtime/version stamp. One file, one write. A re-run
    // re-stamps `version` — the idempotent replacement for `mustard update`.
    write_project_config(&project_path, &runtime, !options.yes)?;

    print_next_steps();
    Ok(())
}

/// Pre-flight location guard: refuse to init a directory that sits INSIDE a
/// git repository without being that repository's root.
///
/// Why: the workspace resolver (`mustard_core::io::workspace`) anchors on git
/// repository roots. A `mustard.json` + `.claude/` planted in a non-root
/// subdirectory would never win the resolution — it would only sit there as a
/// confusing phantom (the historical monorepo defect this guard closes).
///
/// Rules (filesystem probes only — fail-open, no `git` subprocess):
/// - the target IS a git repository root (`.git` as a directory, or as a file
///   for a submodule / linked worktree) → allow;
/// - the target lies inside a git repository but is not its root → refuse,
///   naming the repository root as the right place to init;
/// - no `.git` anywhere up the tree → allow with a note (projects without git
///   are supported through the resolver's loose fallback).
fn guard_init_location(project_path: &Path) -> Result<()> {
    use mustard_core::io::workspace::is_git_repo_root;

    if is_git_repo_root(project_path) {
        return Ok(());
    }
    let enclosing_root = project_path
        .ancestors()
        .skip(1)
        .find(|dir| is_git_repo_root(dir));
    let Some(repo_root) = enclosing_root else {
        println!(
            "  note: no git repository found here or above - proceeding (projects without git are supported)"
        );
        return Ok(());
    };
    anyhow::bail!(
        "this folder is inside a git repository, but it is not the repository's root.\n\
         Mustard anchors its workspace at the root of a git repository, so initializing here\n\
         would leave the harness state in the wrong place.\n\
         \n\
           repository root: {}\n\
         \n\
         Either run `mustard init` from that repository root, or - if this subfolder is meant\n\
         to be its own Mustard project - make it its own git repository (or a git submodule)\n\
         first, then re-run `mustard init` here.",
        repo_root.display()
    )
}

/// Print one didactic line per seeded file. The seeding itself lives in the
/// core (`mustard_core::platform::project_seed`) — the CLI only narrates:
/// `Created`/`Updated` announce a write, `Preserved` confirms the user's file
/// survived the merge untouched.
fn report_seed(name: &str, outcome: SeedOutcome) {
    match outcome {
        SeedOutcome::Created | SeedOutcome::Updated => println!("  wrote {name}"),
        SeedOutcome::Preserved => println!("  kept {name} (yours, unchanged)"),
    }
}

/// Print the didactic lines for what the core migration engine
/// (`mustard_core::migrate_orchestrator_footprint`) found and did.
fn report_migration(outcome: &mustard_core::MigrationOutcome) {
    for entry in &outcome.migrated {
        match entry.as_str() {
            ".claude/CLAUDE.md" => println!(
                "  removed legacy .claude/CLAUDE.md (the orchestrator is injected from .claude/mustard/ now)"
            ),
            "CLAUDE.md" => println!(
                "  cleaned CLAUDE.md (removed the Mustard import + breadcrumb lines — the root file is fully yours again)"
            ),
            other => println!("  migrated {other}"),
        }
    }
    if outcome.foreign_claude_md {
        println!(
            "  note: .claude/CLAUDE.md exists but is not the Mustard orchestrator — left untouched (the file is yours; Mustard injects its rules from .claude/mustard/ instead)"
        );
    }
}

/// Resolve the bundled `templates/` directory.
///
/// Resolution order:
/// 1. the `MUSTARD_TEMPLATES_DIR` environment variable (explicit override —
///    used by tests and by the Tauri backend, which knows its own layout);
/// 2. `<exe-dir>/templates` and `<exe-dir>/../templates` (installed layout);
/// 3. `<CARGO_MANIFEST_DIR>/templates` (the in-repo layout, for `cargo run`).
fn resolve_templates_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("MUSTARD_TEMPLATES_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Ok(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            for candidate in [exe_dir.join("templates"), exe_dir.join("../templates")] {
                if candidate.is_dir() {
                    return Ok(candidate);
                }
            }
        }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
    if manifest.is_dir() {
        return Ok(manifest);
    }

    anyhow::bail!(
        "could not locate the Mustard `templates/` directory \
         (set MUSTARD_TEMPLATES_DIR to override)"
    )
}

/// Decide how to treat an existing `.claude/`, prompting if no flag settles
/// it. On the interactive "backup" choice the backup is taken here, so the
/// returned action is then [`ExistingAction::Overwrite`].
fn decide_existing_action(claude_path: &Path, options: &InitOptions) -> Result<ExistingAction> {
    if options.force {
        return Ok(ExistingAction::Overwrite);
    }
    if options.yes {
        println!("  .claude/ exists - updating without overwriting user files");
        return Ok(ExistingAction::Merge);
    }
    // Non-interactive stdin (CI, tests, Tauri): default to the safe merge
    // rather than blocking on a prompt that can never be answered.
    if !std::io::stdin().is_terminal() {
        println!("  .claude/ exists - merging (non-interactive)");
        return Ok(ExistingAction::Merge);
    }

    let choices = ["Backup and overwrite", "Merge (keep my files)", "Cancel"];
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(".claude/ already exists")
        .items(choices)
        .default(1)
        .interact()
        .context("reading the .claude/ conflict choice")?;

    match choice {
        0 => {
            backup_claude_dir(claude_path)?;
            Ok(ExistingAction::Overwrite)
        }
        1 => Ok(ExistingAction::Merge),
        _ => Ok(ExistingAction::Cancel),
    }
}

/// Copy `.claude/` to a timestamped `.backup.` sibling.
fn backup_claude_dir(claude_path: &Path) -> Result<()> {
    let stamp = mustard_core::time::filename_safe_now();
    let backup = claude_path.with_file_name(format!(
        "{}.backup.{stamp}",
        claude_path
            .file_name()
            .map_or_else(|| ".claude".to_string(), |n| n.to_string_lossy().into_owned())
    ));
    copy_dir(claude_path, &backup, true, &[])?;
    println!("  Backup: {}", backup.display());
    Ok(())
}

/// Copy `templates/.github/` → `<project>/.github/` when the project has a
/// GitHub remote. Never overwrites — user customisations win. Returns the
/// number of files copied (0 when there is no `.github` payload or no remote).
fn install_github_templates(templates_dir: &Path, project_path: &Path) -> Result<usize> {
    let src = templates_dir.join(".github");
    if !src.is_dir() || !has_github_remote(project_path) {
        return Ok(0);
    }
    copy_dir(&src, &project_path.join(".github"), false, &[])
}

/// Whether `origin`'s URL points at github.com.
fn has_github_remote(project_path: &Path) -> bool {
    Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(project_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
        .is_some_and(|url| url.contains("github.com"))
}

/// Build and write the single project-root `mustard.json`.
///
/// Loads any existing config (so a re-run preserves user edits), folds in the
/// git-flow + locale choices and agnostically-detected commands — only when
/// interactive or on a fresh project; otherwise the existing git-flow is left
/// untouched — then stamps `runtime` + `version` and writes **once**. There is
/// no `.claude/mustard.json`: the file lives at the project root (the workspace
/// anchor), the single source of truth.
fn write_project_config(project_path: &Path, runtime: &Runtime, interactive: bool) -> Result<()> {
    let mut config = ProjectConfig::load(project_path);
    let fresh = !ProjectConfig::exists(project_path);

    if interactive || fresh {
        let facts = git_flow::probe_git(project_path);
        let choices = git_flow::collect_choices(&facts, &config, interactive)?;
        git_flow::apply_choices(&mut config, &choices, project_path);
    } else {
        println!("  mustard.json already exists - git flow preserved");
    }

    // Seed the default inject declarations only when the user has none — a
    // curated (non-empty) list is theirs and is preserved verbatim. The
    // defaults live in the core (`project_seed::default_inject_entries`).
    if config.inject.is_empty() {
        config.inject = mustard_core::default_inject_entries();
        println!("  seeded inject declarations (.claude/mustard/*.md ride the session hooks)");
    }

    config.runtime = Some(runtime.clone());
    // The stamp is the HARNESS version (plugin manifest when launched from the
    // plugin, the core line otherwise) — no longer this CLI crate's version.
    // The drift advisory + `/mustard:upsert` compare against the same source.
    config.version = Some(mustard_core::harness_version());
    config.write(project_path)?;
    println!("  wrote mustard.json");
    Ok(())
}
/// Ensure `~/.claude/settings.json` grants `Read`/`Write`/`Edit` and sets the
/// `CLAUDE_CODE_NO_FLICKER` env var. Non-destructive: only adds what is
/// missing, preserves everything else.
///
/// **Opt-in.** Mutating the user's *global* `~/.claude/settings.json` is off by
/// default — user policy is to never touch global settings unprompted. The
/// write only runs when `MUSTARD_GLOBAL_PERMISSIONS` is set to `1`/`true`;
/// otherwise this is a no-op and the project-local `.claude/settings.json` is
/// the only thing `init` writes.
fn ensure_global_permissions() -> Result<()> {
    if !global_permissions_opt_in() {
        println!(
            "  Global settings: skipped (set MUSTARD_GLOBAL_PERMISSIONS=1 to update ~/.claude/settings.json)"
        );
        return Ok(());
    }
    let Some(home) = home_dir() else {
        return Ok(());
    };
    let claude_dir = home.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    let mut settings = crate::fs_ops::read_json_object(&settings_path);

    // permissions.allow — add the generic perm, dropping path-scoped variants.
    let permissions = settings
        .entry("permissions")
        .or_insert_with(|| json!({}));
    let allow = permissions
        .as_object_mut()
        .and_then(|p| {
            p.entry("allow")
                .or_insert_with(|| json!([]))
                .as_array_mut()
        });
    let mut added = Vec::new();
    if let Some(allow) = allow {
        for perm in ["Read", "Write", "Edit"] {
            let has_generic = allow.iter().any(|v| v.as_str() == Some(perm));
            if !has_generic {
                let scoped_prefix = format!("{perm}(");
                allow.retain(|v| {
                    !v.as_str().is_some_and(|s| s.starts_with(&scoped_prefix))
                });
                allow.push(json!(perm));
                added.push(perm);
            }
        }
    }

    // env.CLAUDE_CODE_NO_FLICKER = "1"
    let env = settings.entry("env").or_insert_with(|| json!({}));
    let mut env_added = false;
    if let Some(env) = env.as_object_mut() {
        if env.get("CLAUDE_CODE_NO_FLICKER").and_then(|v| v.as_str()) != Some("1") {
            env.insert("CLAUDE_CODE_NO_FLICKER".to_string(), json!("1"));
            env_added = true;
        }
    }

    if added.is_empty() && !env_added {
        println!("  Global settings: permissions and env already configured");
        return Ok(());
    }

    mfs::create_dir_all(&claude_dir)
        .with_context(|| format!("creating {}", claude_dir.display()))?;
    let mut serialized =
        serde_json::to_string_pretty(&serde_json::Value::Object(settings))
            .context("serializing global settings")?;
    serialized.push('\n');
    mfs::write_atomic(&settings_path, serialized.as_bytes())
        .with_context(|| format!("writing {}", settings_path.display()))?;
    if !added.is_empty() {
        println!("  Global permissions: added {} to ~/.claude/settings.json", added.join(", "));
    }
    if env_added {
        println!("  Global env: set CLAUDE_CODE_NO_FLICKER in ~/.claude/settings.json");
    }
    Ok(())
}

/// Whether the user opted in to having `init` mutate the *global*
/// `~/.claude/settings.json`. Off by default; enabled by setting
/// `MUSTARD_GLOBAL_PERMISSIONS` to `1` or `true` (case-insensitive).
fn global_permissions_opt_in() -> bool {
    std::env::var("MUSTARD_GLOBAL_PERMISSIONS")
        .is_ok_and(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true"
        })
}

/// The user's home directory, cross-platform, without a `dirs` crate
/// dependency: `HOME` on Unix, `USERPROFILE` on Windows.
fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// Ensure RTK (Rust Token Killer) is installed. Best-effort and fail-open: a
/// missing RTK — and a *failed* install — never blocks `init`.
///
/// Flow: if `rtk` is already on PATH, run `rtk init -g --no-patch` and return.
/// Otherwise attempt an auto-install (see [`install_rtk`]); on success re-run
/// the `rtk init`, on failure print the manual instructions and carry on.
fn ensure_rtk() {
    // No external-tool side effects under unit tests: on a clean CI runner this
    // would shell out to `cargo install --git …rtk` (slow / network-bound).
    if cfg!(test) {
        return;
    }
    if rtk_on_path() {
        println!("  RTK detected (token economy active)");
        let _ = Command::new("rtk").args(["init", "-g", "--no-patch"]).output();
        return;
    }

    println!("  RTK not found - attempting auto-install for 60-90% token savings...");
    if install_rtk(rtk_pinned_rev().as_deref()) && rtk_on_path() {
        println!("  RTK installed (token economy active)");
        let _ = Command::new("rtk").args(["init", "-g", "--no-patch"]).output();
    } else {
        println!("  RTK auto-install skipped or unavailable - install manually:");
        if cfg!(windows) {
            println!("    Windows: cargo install --git https://github.com/rtk-ai/rtk");
            println!("         or: scoop install rtk");
        } else {
            println!("    Unix: curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh");
        }
    }
}

/// Whether `rtk --version` succeeds (RTK reachable on PATH).
fn rtk_on_path() -> bool {
    Command::new("rtk")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Probe `rtk --version` and exit hard with install instructions when it
/// fails. RTK is a mandatory dependency: the harness prefixes Bash commands
/// with `rtk`, so a Mustard install without `rtk` on `PATH` would produce a
/// `.claude/` that cannot run. We abort before touching disk rather than
/// failing later in a confusing way.
///
/// This is **not** fail-open — unlike [`ensure_rtk`], which is best-effort
/// during the install phase. The exit code is `1` so CI/Tauri callers can
/// detect the failure and surface it to the user.
fn probe_rtk() {
    // Skip the hard gate under unit tests: a clean CI runner has no `rtk`, and a
    // `process::exit` here would kill the whole test process.
    if cfg!(test) || rtk_on_path() {
        return;
    }
    eprintln!(
        "\nMustard requires RTK (Rust Token Killer) on PATH.\n\
         Could not run `rtk --version` — RTK is a mandatory dependency.\n\
         Install RTK and re-run `mustard init`:\n\
           - Unix:    curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh\n\
           - Windows: scoop install rtk   (or)   cargo install --git https://github.com/rtk-ai/rtk\n"
    );
    std::process::exit(1);
}

/// Read the RTK revision pinned in the managed-artifact manifest
/// (`<templates_dir>/.artifacts.json`, record `tool:rtk`).
///
/// Fail-open: a missing / unreadable / unparseable manifest, an absent
/// `tool:rtk` record, or a null version all yield `None`, leaving the caller
/// on the current unpinned-install behavior. Never errors or panics.
///
/// A branch name (e.g. `develop`) is treated as "unpinned": only a concrete
/// rev is usable as `cargo install --rev`, so callers receive `None` for it.
fn rtk_pinned_rev() -> Option<String> {
    let manifest_path = resolve_templates_dir().ok()?.join(".artifacts.json");
    let raw = mfs::read_to_string(&manifest_path).ok()?;
    let manifest: mustard_core::domain::model::provenance::ArtifactManifest =
        serde_json::from_str(&raw).ok()?;
    let version = manifest
        .artifacts
        .into_iter()
        .find(|record| record.id == "tool:rtk")?
        .version?;
    // A 40-char hex string is a commit SHA; anything else is a branch/tag and
    // is not safe to pass to `cargo install --rev`.
    let is_sha = version.len() == 40 && version.bytes().all(|b| b.is_ascii_hexdigit());
    is_sha.then_some(version)
}

/// Best-effort RTK auto-install. Returns `true` only when an installer command
/// exited successfully. Every spawn failure is swallowed — a host without
/// `curl`/`cargo`/`scoop` simply falls through to the manual instructions.
///
/// `pinned_rev` is the RTK commit SHA from the manifest (`rtk_pinned_rev`);
/// when present it pins the `cargo install --git` to that rev, when `None` the
/// install runs unpinned.
fn install_rtk(pinned_rev: Option<&str>) -> bool {
    let run_ok = |cmd: &mut Command| -> bool {
        cmd.output().is_ok_and(|o| o.status.success())
    };

    if cfg!(windows) {
        if run_ok(Command::new("scoop").args(["install", "rtk"])) {
            return true;
        }
        let mut cargo = Command::new("cargo");
        cargo.args(["install", "--git", "https://github.com/rtk-ai/rtk"]);
        if let Some(rev) = pinned_rev {
            cargo.args(["--rev", rev]);
        }
        run_ok(&mut cargo)
    } else {
        run_ok(Command::new("sh").arg("-c").arg(
            "curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh",
        ))
    }
}

/// Ensure ripgrep (`rg`) is installed. Best-effort and fail-open: a missing
/// `rg` — and a *failed* install — never blocks `init`.
///
/// Why: RTK's `grep`/`find` filters use `rg` as their search engine. When `rg`
/// is missing, RTK prints a fallback warning on every invocation that pollutes
/// every Bash tool output with ~50 tokens.
///
/// Flow: if `rg` is already on PATH, return silently. Otherwise attempt
/// auto-install via Scoop (Windows) or `cargo install ripgrep`; on Unix only
/// print manual instructions (the package manager varies).
fn ensure_ripgrep() {
    // No external-tool side effects under unit tests (would `cargo install
    // ripgrep` on a clean CI runner). Production keeps `cfg!(test) == false`.
    if cfg!(test) {
        return;
    }
    if rg_on_path() {
        return;
    }

    println!("  ripgrep not found - attempting auto-install (silences RTK `rg` fallback warning)...");
    if install_ripgrep() && rg_on_path() {
        println!("  ripgrep installed");
        return;
    }

    println!("  ripgrep auto-install skipped or unavailable - install manually:");
    if cfg!(windows) {
        println!("    Windows: scoop install ripgrep");
        println!("         or: cargo install ripgrep");
    } else if cfg!(target_os = "macos") {
        println!("    macOS:   brew install ripgrep");
    } else {
        println!("    Linux:   apt install ripgrep | pacman -S ripgrep | dnf install ripgrep");
    }
}

/// Whether `rg --version` succeeds (ripgrep reachable on PATH).
fn rg_on_path() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Best-effort ripgrep auto-install. Returns `true` only when an installer
/// command exited successfully. Every spawn failure is swallowed.
///
/// - Windows: try `scoop install ripgrep` first, then `cargo install ripgrep`.
/// - Unix: return `false` so the caller prints manual instructions.
fn install_ripgrep() -> bool {
    let run_ok = |cmd: &mut Command| -> bool {
        cmd.output().is_ok_and(|o| o.status.success())
    };

    if cfg!(windows) {
        if run_ok(Command::new("scoop").args(["install", "ripgrep"])) {
            return true;
        }
        return run_ok(Command::new("cargo").args(["install", "ripgrep"]));
    }
    false
}

/// Print the closing "next steps" block.
fn print_next_steps() {
    println!("\nDone!\n");
    println!("Next:");
    println!("  1. Make sure the `mustard` plugin is enabled in Claude Code (user scope):");
    println!("     /plugin marketplace add <mustard repo or local directory>  →  /plugin install mustard");
    println!("     (already installed? nothing to do — enablement lives in ~/.claude/settings.json)");
    println!("  2. Open Claude Code and run /scan to analyze your codebase.\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a minimal fake `templates/` tree and return its path. Tests point
    /// `init_with_templates` at this so they never touch the real payload. The
    /// four harness seeds (settings, injectables, `.gitignore`) come from the
    /// COMPILED-IN core constants now — this fixture only carries what the
    /// templates dir still owns for init (`.github/`, manifests) plus a
    /// `commands/` decoy: the thin init must NOT copy it into `.claude/`.
    fn fake_templates(root: &Path) -> PathBuf {
        let templates = root.join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        fs::write(templates.join("commands/feature.md"), "feature").unwrap();
        templates
    }

    /// Regression guard (2026-06-03): the legacy per-subproject guards file
    /// `.claude/commands/guards.md` (and its `patterns.md` companion) is
    /// OBSOLETE. No shipped template may point an agent at those non-existent
    /// paths. Walks the REAL bundled `templates/` payloads — the CLI's own
    /// tree AND the core seed tree (`packages/core/templates/`, where the
    /// harness seeds moved) — and fails if the obsolete path is reintroduced.
    #[test]
    fn templates_never_reference_obsolete_guards_file() {
        let templates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        assert!(
            templates.is_dir(),
            "templates payload missing at {}",
            templates.display()
        );
        let core_templates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/core/templates");
        assert!(
            core_templates.is_dir(),
            "core seed payload missing at {}",
            core_templates.display()
        );

        const FORBIDDEN: [&str; 2] = ["commands/guards.md", "commands/patterns.md"];
        let mut offenders: Vec<String> = Vec::new();

        // Iterative directory walk — no external crate.
        let mut stack = vec![templates.clone(), core_templates];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(bytes) = fs::read(&path) else {
                    continue;
                };
                let text = String::from_utf8_lossy(&bytes);
                for needle in FORBIDDEN {
                    if text.contains(needle) {
                        offenders.push(format!("{} → {needle}", path.display()));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "templates must not reference the obsolete standalone guards file:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn timestamp_slug_has_expected_shape() {
        let slug = mustard_core::time::filename_safe_now();
        // YYYY-MM-DDTHH-MM-SS
        assert_eq!(slug.len(), 19);
        assert_eq!(&slug[4..5], "-");
        assert_eq!(&slug[10..11], "T");
    }

    #[test]
    fn init_seeds_harness_and_enables_plugin() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let claude = project.join(".claude");
        // The seed files are laid down — injectables replace the planted orchestrator.
        assert!(claude.join("settings.json").exists(), ".claude/settings.json seeded");
        assert!(
            claude.join("mustard").join("orchestrator.md").exists(),
            ".claude/mustard/orchestrator.md seeded"
        );
        assert!(
            !claude.join("CLAUDE.md").exists(),
            "init must NOT plant .claude/CLAUDE.md — the orchestrator is injected now"
        );
        assert!(claude.join(".gitignore").exists(), ".claude/.gitignore seeded");

        // The content payload is the plugin's now — init must NOT copy it.
        assert!(
            !claude.join("commands").exists(),
            "commands/skills/agents/refs ship in the mustard plugin, never .claude/"
        );
        // The plugin ships `.mcp.json`; init writes no project-level copy.
        assert!(
            !project.join(".mcp.json").exists(),
            "init must not write .mcp.json — the plugin ships it"
        );

        // settings.json carries the reduced seed keys and NO plugin enablement —
        // that choice lives at user scope, never planted into the project.
        // Content now comes from the compiled-in core seed, so assert on a
        // stable key the real seed carries.
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.json"));
        assert_eq!(
            settings
                .get("env")
                .and_then(|e| e.get("MUSTARD_SPEC_SIZE_MODE"))
                .and_then(|v| v.as_str()),
            Some("warn"),
            "the compiled-in seed's env is laid down verbatim"
        );
        assert!(settings.get("statusLine").is_some(), "seed statusLine present");
        assert!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .is_none(),
            "init must not plant enabledPlugins in the project"
        );
        assert!(
            settings
                .get("extraKnownMarketplaces")
                .and_then(|m| m.get("mustard"))
                .is_none(),
            "init must not plant a marketplace entry in the project"
        );

        // .gitignore covers the ephemeral harness state.
        assert!(
            fs::read_to_string(claude.join(".gitignore")).unwrap().contains(".events/"),
            ".gitignore covers the ephemeral .events/ dir"
        );

        // The SINGLE project-root mustard.json carries git-flow, the version
        // stamp, runtime, and the language/tone defaults — and there is NO
        // .claude/mustard.json.
        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        assert_eq!(
            cfg.get("version").and_then(|v| v.as_str()),
            Some(mustard_core::harness_version().as_str()),
            "the stamp is the harness version, not the CLI crate's"
        );
        assert!(cfg.get("runtime").is_some(), "runtime block written");
        assert!(cfg.get("git").is_some(), "git-flow block written");
        assert_eq!(cfg.get("specLang").and_then(|v| v.as_str()), Some("pt-BR"));
        assert_eq!(cfg.get("tone").and_then(|v| v.as_str()), Some("didactic"));
        // The default inject declaration is seeded (orchestrator on the first
        // prompt, once). The response style is a plugin output-style now, not a
        // per-project injectable.
        let inject = cfg.get("inject").and_then(|v| v.as_array()).expect("inject seeded");
        assert_eq!(inject.len(), 1, "one default inject entry: {inject:?}");
        assert_eq!(
            inject[0].get("file").and_then(|v| v.as_str()),
            Some(".claude/mustard/orchestrator.md")
        );
        assert_eq!(inject[0].get("on").and_then(|v| v.as_str()), Some("userPromptSubmit"));
        assert_eq!(inject[0].get("once").and_then(|v| v.as_bool()), Some(true));
        assert!(
            !claude.join("mustard.json").exists(),
            "no .claude/mustard.json — config lives only at the project root"
        );

        // init seeds no entity-registry — the repo model is grain's
        // `.claude/grain.model.json`, produced on demand by `mustard-rt run scan`.
        assert!(!claude.join("entity-registry.json").exists());
    }

    #[test]
    fn init_dry_run_writes_nothing() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let dry = work.path().join("dry");
        fs::create_dir_all(&dry).unwrap();

        init_with_templates(
            &dry,
            &templates,
            &InitOptions { yes: true, dry_run: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(!dry.join(".claude").exists(), "dry-run wrote nothing");
    }

    /// Regression guard for the `.claude/.claude/` nesting bug (I1 rule): even
    /// if `templates/` carries a stray `.claude/` sub-directory, the thin init —
    /// whose harness seeds are compiled-in constants, not directory copies —
    /// must never propagate it.
    #[test]
    fn init_does_not_create_nested_claude_dir() {
        let work = tempdir().unwrap();

        let templates = work.path().join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        // Inject the offending .claude/ inside templates/.
        fs::create_dir_all(templates.join(".claude/commands")).unwrap();
        fs::write(templates.join(".claude/commands/notes.md"), "boilerplate").unwrap();

        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let nested = project.join(".claude").join(".claude");
        assert!(!nested.exists(), ".claude/.claude/ must not be created — I1 rule");
    }

    #[test]
    fn init_merge_preserves_user_injectable() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        // A user-customised injectable already present in .claude/mustard/.
        fs::create_dir_all(claude.join("mustard")).unwrap();
        fs::write(claude.join("mustard/orchestrator.md"), "USER EDIT").unwrap();

        // Non-interactive existing-dir path resolves to a merge.
        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // The user's customised injectable survives the merge untouched…
        assert_eq!(
            fs::read_to_string(claude.join("mustard/orchestrator.md")).unwrap(),
            "USER EDIT",
            "merge must not overwrite a user-customised injectable"
        );
        // …while a seed the user does not have is backfilled…
        assert!(
            claude.join(".gitignore").exists(),
            "merge backfills a missing seed"
        );
        // …and no plugin enablement is planted on the merge path either.
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.json"));
        assert!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .is_none(),
            "merge must not plant plugin enablement"
        );
    }

    #[test]
    fn init_migrates_planted_orchestrator_and_root_lines() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        fs::create_dir_all(&claude).unwrap();
        // The legacy layout: a Mustard-planted orchestrator (carries the
        // marker) + a root CLAUDE.md carrying the import and breadcrumb lines.
        fs::write(
            claude.join("CLAUDE.md"),
            "# Orchestrator Rules\n\nYou are the router.\n",
        )
        .unwrap();
        fs::write(
            project.join("CLAUDE.md"),
            "@.claude/scan-map.md\n\n# (root)\n\n> Orchestrator: [.claude/CLAUDE.md](.claude/CLAUDE.md)\n\n## Guards\n\n- keep this guard\n",
        )
        .unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // (a) the planted orchestrator is gone (and not re-planted).
        assert!(
            !claude.join("CLAUDE.md").exists(),
            "the Mustard-planted .claude/CLAUDE.md must be migrated away"
        );
        // (b) the root file lost ONLY the Mustard lines; the rest survives.
        let root_md = fs::read_to_string(project.join("CLAUDE.md")).unwrap();
        assert!(!root_md.contains("@.claude/scan-map.md"), "import line removed: {root_md}");
        assert!(!root_md.contains("> Orchestrator:"), "breadcrumb removed: {root_md}");
        assert!(root_md.contains("# (root)"), "user heading survives: {root_md}");
        assert!(root_md.contains("## Guards"), "Guards section survives: {root_md}");
        assert!(root_md.contains("- keep this guard"), "guard line survives: {root_md}");

        // A `.claude/CLAUDE.md` WITHOUT the marker is the user's — it survives
        // a re-run untouched.
        fs::write(claude.join("CLAUDE.md"), "MY OWN NOTES\n").unwrap();
        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(claude.join("CLAUDE.md")).unwrap(),
            "MY OWN NOTES\n",
            "a user-authored .claude/CLAUDE.md (no marker) must never be deleted"
        );
    }

    #[test]
    fn init_preserves_user_inject_entries() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // The user already curated their own inject list.
        fs::write(
            project.join("mustard.json"),
            r#"{"inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        let inject = cfg.get("inject").and_then(|v| v.as_array()).expect("inject present");
        assert_eq!(inject.len(), 1, "the curated list is preserved, not replaced: {inject:?}");
        assert_eq!(
            inject[0].get("file").and_then(|v| v.as_str()),
            Some("docs/my-rules.md"),
            "user entry survives verbatim"
        );
    }

    #[test]
    fn init_refuses_inside_git_repo_when_not_at_its_root() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        // `work` is a git repository root; the init target is a subdirectory.
        fs::create_dir_all(work.path().join(".git")).unwrap();
        let project = work.path().join("apps").join("dashboard");
        fs::create_dir_all(&project).unwrap();

        let err = init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("not the repository's root"),
            "refusal must be didactic, got: {msg}"
        );
        assert!(
            msg.contains("repository root:"),
            "refusal must name the repository root, got: {msg}"
        );
        // Refusal happens before any disk write.
        assert!(!project.join(".claude").exists(), "refusal wrote .claude/");
        assert!(!project.join("mustard.json").exists(), "refusal wrote mustard.json");
    }

    #[test]
    fn init_allows_at_git_repo_root() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(project.join(".git")).unwrap(); // project IS a repo root

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.json").exists());
        assert!(project.join("mustard.json").exists());
    }

    #[test]
    fn init_allows_at_submodule_root_with_git_file() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        // Outer repository root…
        fs::create_dir_all(work.path().join(".git")).unwrap();
        // …and a submodule below it: `.git` is a FILE with a `gitdir:` pointer.
        let sub = work.path().join("backend").join("service");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".git"), "gitdir: ../../.git/modules/service\n").unwrap();

        init_with_templates(
            &sub,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(
            sub.join(".claude").join("settings.json").exists(),
            "a submodule root (.git file) is a legitimate init target"
        );
    }

    #[test]
    fn init_allows_in_git_less_tree() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("plain");
        fs::create_dir_all(&project).unwrap(); // no .git anywhere up the tempdir

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.json").exists());
    }

    // The `retire_planted_plugin_enablement` unit tests moved to the core with
    // the function (`packages/core/src/platform/project_seed.rs`) — the CLI
    // only relays through `mustard_core::seed_settings`, covered above.
}
