//! `mustard init` — scaffold the `.claude/` folder into a project.
//!
//! Ported from `commands/init.ts`. The flow:
//!
//! 1. resolve the bundled `templates/` directory;
//! 2. handle an already-present `.claude/` (force-overwrite, merge, or
//!    backup-then-overwrite — interactively prompted when no flag decides it);
//! 3. recursively copy `templates/` → `.claude/`;
//! 4. copy `templates/.github/` → project-root `.github/` when a GitHub
//!    remote is detected;
//! 5. seed an empty `entity-registry.json` on a fresh install;
//! 6. ensure global Claude Code permissions in `~/.claude/settings.json`;
//! 7. install RTK (token economy) if missing — fail-open;
//! 8. write the `runtime` block **and the `version` stamp** into
//!    `.claude/mustard.json`;
//! 9. generate the project-root `mustard.json` git-flow config.
//!
//! The **`version` stamp** (step 8) is new in the Rust port: `init` records
//! `env!("CARGO_PKG_VERSION")` so the dashboard (B6) can read the installed
//! Mustard version. It is merged surgically via [`crate::fs_ops::merge_json`].

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use mustard_core::io::fs as mfs;
use serde_json::json;

use crate::commands::git_flow;
use crate::fs_ops::{copy_dir, merge_json};
use crate::runtime::RuntimeInfo;

/// Flags accepted by `mustard init`.
#[derive(Debug, Default, Clone)]
pub struct InitOptions {
    /// Overwrite an existing `.claude/` without a backup.
    pub force: bool,
    /// Accept defaults without prompting.
    pub yes: bool,
    /// Install the experimental Cursor IDE adapter.
    pub cursor: bool,
    /// Print intended actions without touching disk.
    pub dry_run: bool,
}

/// What to do with an already-present `.claude/` directory.
enum ExistingAction {
    /// Overwrite (the `--force` path, or the interactive "backup" choice once
    /// the backup has been taken).
    Overwrite,
    /// Copy only new files, leaving user edits in place.
    Merge,
    /// Abort without writing.
    Cancel,
}

/// Run `mustard init` against `project_path`.
///
/// This is the library entry point the Tauri backend (Wave 3) will call. The
/// binary passes the process working directory; a caller may pass any folder.
/// The bundled `templates/` directory is located via [`resolve_templates_dir`];
/// callers that already know its location use [`init_with_templates`].
pub fn init(project_path: &Path, options: &InitOptions) -> Result<()> {
    let templates_dir = resolve_templates_dir()?;
    init_with_templates(project_path, &templates_dir, options)
}

/// [`init`] with the `templates/` directory supplied explicitly.
///
/// Splitting this out keeps template resolution (an environment concern) out
/// of the install logic, so tests can drive a fixture tree and the Tauri
/// backend can point at its own bundled payload — no process-global env var,
/// no `unsafe` env mutation.
pub fn init_with_templates(
    project_path: &Path,
    templates_dir: &Path,
    options: &InitOptions,
) -> Result<()> {
    // RTK is a mandatory dependency of Mustard — the harness's Golden Rule
    // prefixes every Bash invocation with `rtk`, and the generated
    // `settings.json` wires every hook through `rtk mustard-rt on <Event>`.
    // Probe before touching disk: if `rtk` is missing the install would
    // produce a `.claude/` that cannot run, so we exit hard with install
    // instructions instead. Skipped in dry-run mode (no disk writes either).
    if !options.dry_run {
        probe_rtk();
    }

    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("resolving project path {}", project_path.display()))?;
    let claude_path = project_path.join(".claude");

    println!("\nMustard\n");

    let runtime = RuntimeInfo::detect();
    println!("[mustard] runtime: {} {}/{}", runtime.kind, runtime.os, runtime.arch);

    if options.dry_run {
        println!("  (dry-run) would copy templates -> {}", claude_path.display());
        println!(
            "  (dry-run) would write runtime + version to {}",
            claude_path.join("mustard.json").display()
        );
        return Ok(());
    }

    let mut fresh_install = true;

    if claude_path.exists() {
        fresh_install = false;
        match decide_existing_action(&claude_path, options)? {
            ExistingAction::Cancel => {
                println!("\n  Cancelled.\n");
                return Ok(());
            }
            ExistingAction::Merge => {
                let count = copy_dir(templates_dir, &claude_path, false, &[".github", ".claude"])?;
                let gh = install_github_templates(templates_dir, &project_path)?;
                report_copy(count, gh, false);
            }
            ExistingAction::Overwrite => {
                let count = copy_dir(templates_dir, &claude_path, true, &[".github", ".claude"])?;
                let gh = install_github_templates(templates_dir, &project_path)?;
                report_copy(count, gh, true);
            }
        }
    } else {
        let count = copy_dir(templates_dir, &claude_path, true, &[".github", ".claude"])?;
        let gh = install_github_templates(templates_dir, &project_path)?;
        report_copy(count, gh, true);
    }

    if fresh_install {
        seed_entity_registry(&claude_path)?;
    }

    ensure_global_permissions().unwrap_or_else(|err| {
        eprintln!("[mustard] warning: could not update global permissions: {err}");
    });
    ensure_rtk();
    ensure_ripgrep();

    if options.cursor {
        // The Cursor adapter shipped as `templates/adapters/cursor/adapter.js`
        // in earlier releases; the deep-refactor W5 replaced it with the
        // `mustard-rt run adapt-cursor` subcommand. Surface that to the user
        // instead of copying a no-longer-bundled file.
        println!("  --cursor flag is now served by `mustard-rt run adapt-cursor` (run it after init)");
    }

    // Persist runtime + version stamp into .claude/mustard.json (surgical).
    write_claude_mustard_json(&claude_path, &runtime)?;

    // Project-root git-flow mustard.json.
    git_flow::generate_mustard_json(&project_path, !options.yes)?;

    print_next_steps();
    Ok(())
}

/// Resolve the bundled `templates/` directory.
///
/// Resolution order:
/// 1. the `MUSTARD_TEMPLATES_DIR` environment variable (explicit override —
///    used by tests and by the Tauri backend, which knows its own layout);
/// 2. `<exe-dir>/templates` and `<exe-dir>/../templates` (installed layout —
///    the binary shipped next to its payload);
/// 3. `<CARGO_MANIFEST_DIR>/templates` (the in-repo layout, for `cargo run`).
///
/// Shared with `update`, which copies from the same payload.
pub(crate) fn resolve_templates_dir() -> Result<PathBuf> {
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

    let choices = ["Backup and overwrite", "Merge (skip existing files)", "Cancel"];
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

/// A filesystem-safe UTC timestamp slug (`YYYY-MM-DDTHH-MM-SS`).
///
/// Built from the wall clock without a date crate: seconds since the Unix
/// epoch are decomposed by hand. Used only for backup directory names, where
/// monotonic uniqueness — not calendar exactness — is what matters.
///
/// Shared with `update`, which names its backup the same way.

/// Print the post-copy summary line.
fn report_copy(count: usize, github_count: usize, fresh: bool) {
    let gh = if github_count > 0 {
        format!(" (+ {github_count} GitHub template(s) at .github/)")
    } else {
        String::new()
    };
    if fresh {
        println!("  Copied {count} files to .claude/{gh}");
    } else {
        println!("  Copied {count} new files (existing files preserved){gh}");
    }
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

/// Seed an empty `entity-registry.json` on a fresh install (only when absent).
fn seed_entity_registry(claude_path: &Path) -> Result<()> {
    let registry = claude_path.join("entity-registry.json");
    if registry.exists() {
        return Ok(());
    }
    let body = serde_json::to_string_pretty(&json!({ "_patterns": {}, "_enums": {}, "e": {} }))
        .context("serializing the empty entity registry")?;
    mfs::write_atomic(&registry, body.as_bytes())
        .with_context(|| format!("writing {}", registry.display()))?;
    Ok(())
}

/// Surgically merge the `runtime` block and the `version` stamp into
/// `.claude/mustard.json`. Reuses [`crate::fs_ops::merge_json`], so any other
/// keys already in the file are preserved.
///
/// Shared with `update`, which re-stamps the `version` (but not `runtime` —
/// see [`crate::commands::update`]).
pub(crate) fn write_claude_mustard_json(claude_path: &Path, runtime: &RuntimeInfo) -> Result<()> {
    let path = claude_path.join("mustard.json");
    merge_json(
        &path,
        &[
            ("runtime", serde_json::to_value(runtime)?),
            ("version", json!(crate::VERSION)),
        ],
    )
}

/// Ensure `~/.claude/settings.json` grants `Read`/`Write`/`Edit` and sets the
/// `CLAUDE_CODE_NO_FLICKER` env var. Non-destructive: only adds what is
/// missing, preserves everything else. Ported from `ensureGlobalPermissions`.
///
/// **Opt-in (eliminate-bun Wave 4).** Mutating the user's *global*
/// `~/.claude/settings.json` is off by default — user policy is to never
/// touch global settings unprompted. The write only runs when
/// `MUSTARD_GLOBAL_PERMISSIONS` is set to `1`/`true`; otherwise this is a
/// no-op and the project-local `.claude/settings.json` is the only thing
/// `init`/`update` write.
///
/// Shared with `update`, which re-runs the same global-settings guarantee.
pub(crate) fn ensure_global_permissions() -> Result<()> {
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

/// Whether the user opted in to having `init`/`update` mutate the *global*
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
/// missing RTK — and a *failed* install — never blocks `init`. Ported from
/// `ensureRtk` and completed in eliminate-bun Wave 4.
///
/// Flow: if `rtk` is already on PATH, run `rtk init -g --no-patch` and return.
/// Otherwise attempt an auto-install (see [`install_rtk`]); on success re-run
/// the `rtk init`, on failure print the manual instructions and carry on.
///
/// Shared with `update`, which re-runs the same RTK guarantee.
pub(crate) fn ensure_rtk() {
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
/// fails. RTK is a mandatory dependency: the generated `settings.json` wires
/// every hook through `rtk mustard-rt on <Event>`, and the `bash_guard` hook
/// denies un-prefixed Bash commands in strict mode. A Mustard install without
/// `rtk` on `PATH` would produce a `.claude/` that cannot run, so we abort
/// before touching disk rather than failing later in a confusing way.
///
/// This is **not** fail-open — unlike [`ensure_rtk`], which is best-effort
/// during the install phase. The exit code is `1` so CI/Tauri callers can
/// detect the failure and surface it to the user.
fn probe_rtk() {
    if rtk_on_path() {
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
/// on the current unpinned-install behavior. Never errors or panics — the
/// manifest is a maintainer-side artifact and `init` must not depend on it.
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
///
/// - Unix: pipe the official `install.sh` through `sh` (`curl … | sh`).
/// - Windows: try `scoop install rtk` first (fast, if Scoop is present), then
///   fall back to `cargo install --git`.
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
/// is missing (the default state on a fresh Windows install — `mustard init`
/// installs `rtk` but Scoop's `rtk` manifest does not depend on `ripgrep`),
/// RTK prints `Failed to resolve 'rg' via PATH, falling back to direct exec`
/// on every invocation and falls back to system `grep`. The warning is
/// harmless but pollutes every Bash tool output with ~50 input+output tokens.
///
/// Flow: if `rg` is already on PATH, return silently. Otherwise attempt
/// auto-install via Scoop (Windows) or `cargo install ripgrep`; on Unix only
/// print manual instructions (the package manager varies — apt/brew/pacman —
/// and `rg` ships pre-installed on most modern dev distros).
///
/// Shared with `update`, which re-runs the same ripgrep guarantee.
pub fn ensure_ripgrep() {
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
/// - Unix: return `false` so the caller prints manual instructions (no single
///   default package manager to invoke; `cargo install ripgrep` would compile
///   from source and take minutes, which is hostile in an installer flow).
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
///
/// Lists the opt-in extras shipped under `templates-extras/skills/` (W6 deep
/// refactor): foundation skills the user can install on demand via
/// `mustard add skill:<name>` (routed through `mustard-rt run skill-fetch`).
fn print_next_steps() {
    println!("\nDone!\n");
    println!("Next: open Claude Code and run /scan to analyze your codebase.\n");
    println!("Optional extras (install with `mustard add skill:<name>`):");
    println!("  hallmark             — anti-AI-slop landing pages / design audits");
    println!("  design-craft         — broad design-system generation");
    println!("  react-best-practices — React/Next.js performance + rendering rules");
    println!("  grill-me             — relentless plan-grilling interview\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a minimal fake `templates/` tree and return its path. Tests point
    /// `MUSTARD_TEMPLATES_DIR` at this so they never touch the real payload.
    fn fake_templates(root: &Path) -> PathBuf {
        let templates = root.join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        fs::write(templates.join("CLAUDE.md"), "# rules").unwrap();
        fs::write(templates.join("settings.json"), "{}").unwrap();
        fs::write(templates.join("commands/feature.md"), "feature").unwrap();
        templates
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
    fn init_creates_claude_tree_with_version_stamp() {
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
        assert!(claude.join("CLAUDE.md").exists(), ".claude/CLAUDE.md copied");
        assert!(claude.join("commands/feature.md").exists(), "nested file copied");

        // mustard.json carries the version stamp from CARGO_PKG_VERSION.
        let cfg = crate::fs_ops::read_json_object(&claude.join("mustard.json"));
        assert_eq!(cfg.get("version").and_then(|v| v.as_str()), Some(crate::VERSION));
        assert!(cfg.get("runtime").is_some(), "runtime block written");

        // Fresh install seeds the entity registry.
        assert!(claude.join("entity-registry.json").exists());

        // Project-root git-flow config exists.
        assert!(project.join("mustard.json").exists());
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

    /// Regression guard for the `.claude/.claude/` nesting bug (I1 rule).
    ///
    /// When `templates/` contains a `.claude/` sub-directory (e.g. a subproject
    /// guard added during development), a naive recursive copy propagates it into
    /// the target, producing `<project>/.claude/.claude/` — which violates the
    /// workspace model. This test fails before the fix (`.claude` missing from
    /// the exclude list) and passes after it.
    #[test]
    fn init_does_not_create_nested_claude_dir() {
        let work = tempdir().unwrap();

        // Use a real templates dir that mirrors the bug: templates/ has a .claude/
        // subdir with a file in it.
        let templates = work.path().join("templates");
        // Build the normal payload.
        fs::create_dir_all(templates.join("commands")).unwrap();
        fs::write(templates.join("CLAUDE.md"), "# rules").unwrap();
        fs::write(templates.join("settings.json"), "{}").unwrap();
        fs::write(templates.join("commands/feature.md"), "feature").unwrap();
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
        assert!(
            !nested.exists(),
            ".claude/.claude/ must not be created — I1 rule violated"
        );
    }

    #[test]
    fn init_merge_preserves_user_files() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        fs::create_dir_all(&claude).unwrap();
        // A user-edited file already present in .claude/.
        fs::write(claude.join("CLAUDE.md"), "USER EDIT").unwrap();

        // Non-interactive existing-dir path resolves to a merge.
        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(claude.join("CLAUDE.md")).unwrap(),
            "USER EDIT",
            "merge must not overwrite a user-edited file"
        );
        // …but new template files still arrive.
        assert!(claude.join("commands/feature.md").exists());
    }
}
