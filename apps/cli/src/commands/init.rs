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
//! 3. seed the harness into `.claude/`:
//!    - `settings.json` — the reduced SEED (env / permissions / statusLine /
//!      plansDirectory …) copied from the bundled template, then the
//!      `enabledPlugins` + `extraKnownMarketplaces` keys that enable the
//!      `mustard` plugin merged in;
//!    - `CLAUDE.md` — the orchestrator rules (a plugin cannot ship the project
//!      orchestrator);
//!    - `.gitignore` — covers the ephemeral harness state;
//! 4. copy `templates/.github/` → project-root `.github/` when a GitHub remote
//!    is detected (project-level scaffolding, not part of the plugin);
//! 5. ensure global Claude Code permissions in `~/.claude/settings.json` (opt-in);
//! 6. install RTK + ripgrep (token economy) if missing — fail-open;
//! 7. write the single project-root `mustard.json`: git-flow + agnostically
//!    detected build/test/lint/type-check commands + spec language + tone +
//!    the `runtime`/`version` stamp.
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
use serde_json::{Map, Value, json};

use crate::commands::git_flow;
use crate::fs_ops::copy_dir;
use mustard_core::{ProjectConfig, Runtime};

/// Marketplace name the `mustard` plugin is published under (the key in
/// `settings.json#extraKnownMarketplaces` and the `@`-suffix of the plugin id).
const PLUGIN_MARKETPLACE: &str = "mustard";

/// `settings.json#enabledPlugins` key that turns the harness on: `<plugin>@<marketplace>`.
const PLUGIN_ID: &str = "mustard@mustard";

/// Git URL of the private marketplace that distributes the `mustard` plugin.
///
/// **Placeholder** — a real deploy replaces this constant with the actual repo
/// URL (or a future flag overrides it). It is intentionally obvious so it is
/// never mistaken for a live endpoint: an `init` run against an unconfigured
/// build writes this literal into `settings.json#extraKnownMarketplaces`, where
/// it is inert until swapped for the real URL.
const MARKETPLACE_REPO_URL: &str = "REPLACE_WITH_MUSTARD_PLUGIN_MARKETPLACE_GIT_URL";

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
        println!("    settings.json  — reduced seed + enable the `mustard` plugin (enabledPlugins + extraKnownMarketplaces)");
        println!("    CLAUDE.md      — orchestrator rules");
        println!("    .gitignore     — ephemeral harness state");
        println!(
            "  (dry-run) would write git-flow + commands + runtime/version to {}",
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

    // (a)+(e) settings.json: the reduced seed + the plugin-enable keys.
    write_settings_seed(&claude_path, templates_dir, overwrite)?;
    // (b) orchestrator rules — a plugin cannot ship the project orchestrator.
    seed_file(templates_dir, &claude_path, "CLAUDE.md", overwrite)?;
    // (c) ephemeral-state .gitignore.
    seed_file(templates_dir, &claude_path, ".gitignore", overwrite)?;

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

/// Seed `.claude/settings.json`: copy the reduced template SEED, then merge in
/// the keys that enable the `mustard` plugin.
///
/// - Fresh / overwrite (a backup was taken): the seed is the base.
/// - Merge: the user's existing `settings.json` is the base and any seed key it
///   lacks is backfilled — user edits are never clobbered.
///
/// In every case the `enabledPlugins` + `extraKnownMarketplaces` keys are merged
/// in (never clobbering another marketplace / plugin the user declared), so the
/// harness is enabled even when merging into an existing project. The seed's
/// byte content is not depended on — whatever keys the template ships are copied
/// verbatim through the JSON round-trip.
fn write_settings_seed(claude_path: &Path, templates_dir: &Path, overwrite: bool) -> Result<()> {
    let dest = claude_path.join("settings.json");
    let seed = crate::fs_ops::read_json_object(&templates_dir.join("settings.json"));

    let mut settings = if overwrite {
        seed
    } else {
        let mut existing = crate::fs_ops::read_json_object(&dest);
        for (key, value) in seed {
            existing.entry(key).or_insert(value);
        }
        existing
    };

    enable_mustard_plugin(&mut settings);

    let mut serialized = serde_json::to_string_pretty(&Value::Object(settings))
        .context("serializing .claude/settings.json")?;
    serialized.push('\n');
    mfs::write_atomic(&dest, serialized.as_bytes())
        .with_context(|| format!("writing {}", dest.display()))?;
    println!("  wrote .claude/settings.json (mustard plugin enabled)");
    Ok(())
}

/// Merge the `mustard` plugin enablement into a `settings.json` object.
///
/// Adds `extraKnownMarketplaces.mustard = { source: { source: "git", url } }`
/// (only when absent — a user-declared mustard marketplace is preserved) and
/// sets `enabledPlugins."mustard@mustard" = true`. Other marketplaces and
/// plugins in those objects are left untouched.
fn enable_mustard_plugin(settings: &mut Map<String, Value>) {
    let marketplaces = settings
        .entry("extraKnownMarketplaces")
        .or_insert_with(|| json!({}));
    if let Some(obj) = marketplaces.as_object_mut() {
        obj.entry(PLUGIN_MARKETPLACE).or_insert_with(|| {
            json!({ "source": { "source": "git", "url": MARKETPLACE_REPO_URL } })
        });
    }

    let plugins = settings
        .entry("enabledPlugins")
        .or_insert_with(|| json!({}));
    if let Some(obj) = plugins.as_object_mut() {
        obj.insert(PLUGIN_ID.to_string(), json!(true));
    }
}

/// Copy a single seed file (`CLAUDE.md`, `.gitignore`) from `templates_dir` into
/// `.claude/`. On merge (`overwrite == false`) an existing file is preserved —
/// a user-edited orchestrator survives. Fail-open: a seed missing from the
/// template is skipped with a warning, never an abort.
fn seed_file(templates_dir: &Path, claude_path: &Path, name: &str, overwrite: bool) -> Result<()> {
    let src = templates_dir.join(name);
    if !src.is_file() {
        eprintln!("  warning: seed {} missing from templates — skipped", src.display());
        return Ok(());
    }
    let dest = claude_path.join(name);
    if !overwrite && dest.exists() {
        return Ok(());
    }
    let bytes = mfs::read(&src).with_context(|| format!("reading seed {}", src.display()))?;
    mfs::write_atomic(&dest, &bytes)
        .with_context(|| format!("writing {}", dest.display()))?;
    println!("  wrote .claude/{name}");
    Ok(())
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

    config.runtime = Some(runtime.clone());
    config.version = Some(crate::VERSION.to_string());
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
    println!("Next: open Claude Code and run /scan to analyze your codebase.\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a minimal fake `templates/` tree and return its path. Tests point
    /// `init_with_templates` at this so they never touch the real payload. The
    /// `commands/` dir is a decoy: the thin init must NOT copy it into `.claude/`.
    fn fake_templates(root: &Path) -> PathBuf {
        let templates = root.join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        fs::write(templates.join("CLAUDE.md"), "# orchestrator").unwrap();
        // A reduced seed with an env + a permission the init must copy verbatim.
        fs::write(
            templates.join("settings.json"),
            r#"{"env":{"MUSTARD_TEST":"1"},"permissions":{"allow":["Read"]}}"#,
        )
        .unwrap();
        fs::write(templates.join("commands/feature.md"), "feature").unwrap();
        fs::write(templates.join(".gitignore"), "spec/*/.events/\n").unwrap();
        templates
    }

    /// Regression guard (2026-06-03): the legacy per-subproject guards file
    /// `.claude/commands/guards.md` (and its `patterns.md` companion) is
    /// OBSOLETE. No shipped template may point an agent at those non-existent
    /// paths. Walks the REAL bundled `templates/` payload and fails if the
    /// obsolete path is reintroduced.
    #[test]
    fn templates_never_reference_obsolete_guards_file() {
        let templates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        assert!(
            templates.is_dir(),
            "templates payload missing at {}",
            templates.display()
        );

        const FORBIDDEN: [&str; 2] = ["commands/guards.md", "commands/patterns.md"];
        let mut offenders: Vec<String> = Vec::new();

        // Iterative directory walk — no external crate.
        let mut stack = vec![templates.clone()];
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
        // The three seed files are laid down.
        assert!(claude.join("settings.json").exists(), ".claude/settings.json seeded");
        assert!(claude.join("CLAUDE.md").exists(), ".claude/CLAUDE.md seeded");
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

        // settings.json carries the reduced seed keys AND the plugin enablement.
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.json"));
        assert_eq!(
            settings
                .get("env")
                .and_then(|e| e.get("MUSTARD_TEST"))
                .and_then(|v| v.as_str()),
            Some("1"),
            "the reduced seed's env is copied verbatim"
        );
        assert_eq!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .and_then(|v| v.as_bool()),
            Some(true),
            "the mustard plugin is enabled"
        );
        let url = settings
            .get("extraKnownMarketplaces")
            .and_then(|m| m.get("mustard"))
            .and_then(|e| e.get("source"))
            .and_then(|s| s.get("url"))
            .and_then(|v| v.as_str());
        assert!(url.is_some(), "the marketplace source url is seeded");

        // .gitignore covers the ephemeral harness state.
        assert!(
            fs::read_to_string(claude.join(".gitignore")).unwrap().contains(".events/"),
            ".gitignore covers the ephemeral .events/ dir"
        );

        // The SINGLE project-root mustard.json carries git-flow, the version
        // stamp, runtime, and the language/tone defaults — and there is NO
        // .claude/mustard.json.
        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        assert_eq!(cfg.get("version").and_then(|v| v.as_str()), Some(crate::VERSION));
        assert!(cfg.get("runtime").is_some(), "runtime block written");
        assert!(cfg.get("git").is_some(), "git-flow block written");
        assert_eq!(cfg.get("specLang").and_then(|v| v.as_str()), Some("pt-BR"));
        assert_eq!(cfg.get("tone").and_then(|v| v.as_str()), Some("didactic"));
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
    /// which reads only the three named seed files — must never propagate it.
    #[test]
    fn init_does_not_create_nested_claude_dir() {
        let work = tempdir().unwrap();

        let templates = work.path().join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        fs::write(templates.join("CLAUDE.md"), "# orchestrator").unwrap();
        fs::write(templates.join("settings.json"), "{}").unwrap();
        fs::write(templates.join(".gitignore"), ".events/\n").unwrap();
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
    fn init_merge_preserves_user_orchestrator_and_enables_plugin() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        fs::create_dir_all(&claude).unwrap();
        // A user-edited orchestrator already present in .claude/.
        fs::write(claude.join("CLAUDE.md"), "USER EDIT").unwrap();

        // Non-interactive existing-dir path resolves to a merge.
        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // The user's orchestrator survives the merge untouched.
        assert_eq!(
            fs::read_to_string(claude.join("CLAUDE.md")).unwrap(),
            "USER EDIT",
            "merge must not overwrite a user-edited orchestrator"
        );
        // …but the plugin is still enabled (settings.json is seeded on merge too).
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.json"));
        assert_eq!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .and_then(|v| v.as_bool()),
            Some(true),
            "merge still enables the mustard plugin"
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

    #[test]
    fn enable_mustard_plugin_preserves_other_marketplaces() {
        // A user with their own marketplace + plugin: init adds mustard without
        // clobbering theirs.
        let mut settings: Map<String, Value> = serde_json::from_str(
            r#"{"extraKnownMarketplaces":{"acme":{"source":{"source":"git","url":"x"}}},
                "enabledPlugins":{"acme@acme":true}}"#,
        )
        .unwrap();

        enable_mustard_plugin(&mut settings);

        // Theirs survive.
        assert!(settings["extraKnownMarketplaces"].get("acme").is_some());
        assert_eq!(settings["enabledPlugins"]["acme@acme"], json!(true));
        // Ours are added.
        assert!(settings["extraKnownMarketplaces"].get("mustard").is_some());
        assert_eq!(settings["enabledPlugins"]["mustard@mustard"], json!(true));
    }
}
